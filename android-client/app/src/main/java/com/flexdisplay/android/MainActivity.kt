package com.flexdisplay.android

import android.content.Context
import android.content.res.Configuration
import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.graphics.Color
import android.view.Gravity
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.text.Html
import android.widget.CompoundButton
import android.widget.FrameLayout
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.ArrayAdapter
import android.widget.Switch
import android.widget.TextView
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AlertDialog
import okhttp3.Call
import okhttp3.Callback
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.TimeUnit

class MainActivity : AppCompatActivity() {

    companion object {
        // Adaptive stream sizing: native device resolution capped by transport (host refines per encoder).
        private const val USB_MAX_STREAM_WIDTH = 1920.0
        private const val USB_MAX_STREAM_HEIGHT = 1200.0
        private const val WIFI_MAX_STREAM_WIDTH = 1280.0
        private const val WIFI_MAX_STREAM_HEIGHT = 720.0
        private const val STREAM_FPS_USB = 60
        private const val STREAM_FPS_WIFI = 30
        private const val STREAM_BITRATE_USB_KBPS = 8000
        private const val STREAM_BITRATE_WIFI_KBPS = 5000
        // Decoder tolerance for adaptive profiles up to 1920×1200.
        private const val DECODER_MAX_WIDTH = 1920
        private const val DECODER_MAX_HEIGHT = 1200
        private const val INPUT_MOVE_SEND_INTERVAL_MS = 4L
        private const val PREFS_NAME = "tablet_monitor_prefs"
        private const val PREF_LANGUAGE = "app_language"
    }

    private var streamSocket: WebSocket? = null
    private var inputSocket: WebSocket? = null

    private lateinit var serverIpInput: EditText
    private lateinit var displayInput: EditText
    private lateinit var modeSpinner: Spinner
    private lateinit var connectButton: Button
    private lateinit var helpButton: Button
    private lateinit var languageButton: Button
    private lateinit var menuButton: Button
    private lateinit var statusText: TextView
    private lateinit var logText: TextView
    private lateinit var topPanel: LinearLayout
    private lateinit var setupScroll: ScrollView
    private lateinit var streamSurface: SurfaceView
    private lateinit var streamContainer: FrameLayout
    private lateinit var hudText: TextView
    private lateinit var hudToggle: Switch
    private lateinit var logScroll: ScrollView
    private var hudEnabled = true

    private val reconnectHandler = Handler(Looper.getMainLooper())
    private var reconnectAttempts = 0
    // EMA-smoothed end-to-end latency estimate from host timestamps (T: frames).
    // We normalize with the minimum observed host->client clock delta to work even
    // when device and PC clocks are not perfectly synchronized.
    @Volatile private var emaE2eMs = 0f
    @Volatile private var minObservedClockDeltaMs = Long.MAX_VALUE
    @Volatile private var lastVideoChunkAtMs = 0L
    @Volatile private var hasReceivedVideoChunk = false
    @Volatile private var queuedMovePayload: String? = null
    @Volatile private var moveFlushScheduled = false
    @Volatile private var lastMoveSentAtMs = 0L
    // Input RTT: round-trip time of the input WebSocket channel measured via ping/pong.
    // Written from OkHttp thread, read from main thread — @Volatile is sufficient.
    @Volatile private var inputRttMs = -1L
    @Volatile private var activeStreamProfile = ""
    @Volatile private var streamRestartRequested = false

    // Dimensions used to configure the active decoder (set from requestedW/H in startH264Stream).
    private var decoderTargetW = DECODER_MAX_WIDTH
    private var decoderTargetH = DECODER_MAX_HEIGHT
    // Last picture/buffer dimensions received from INFO_OUTPUT_FORMAT_CHANGED.
    private var lastPicW = 0; private var lastPicH = 0
    private var lastBufW = 0; private var lastBufH = 0
    private var lastCropL = 0; private var lastCropT = 0
    // Sends a ping every 2 s to measure input round-trip time.
    private val inputPingRunnable = object : Runnable {
        override fun run() {
            val socket = inputSocket ?: return
            if (!connected) return
            val tsMs = System.currentTimeMillis()
            socket.send("""{"type":"ping","ts_ms":$tsMs}""")
            reconnectHandler.postDelayed(this, 2000L)
        }
    }

    private val inputMoveFlushRunnable = object : Runnable {
        override fun run() {
            moveFlushScheduled = false
            val payload = queuedMovePayload ?: return
            queuedMovePayload = null
            val socket = inputSocket ?: return
            if (socket.send(payload)) {
                lastMoveSentAtMs = SystemClock.elapsedRealtime()
            }
        }
    }

    private val streamStallWatchdog = object : Runnable {
        override fun run() {
            val now = SystemClock.elapsedRealtime()
            val last = lastVideoChunkAtMs
            // Only watch for stalls after at least one real video chunk arrived.
            // Before first frame, ffmpeg may still be probing encoders/fallbacks.
            val isStalled = connected && streamSocket != null && !pendingStreamStart && hasReceivedVideoChunk && last > 0L && (now - last) > 2500L
            if (isStalled) {
                appendLog("Detected frozen stream (>2.5s without video), reconnecting...")
                // Throttle repeated triggers while close handshake completes.
                lastVideoChunkAtMs = now
                hasReceivedVideoChunk = false
                streamSocket?.close(1011, "stream stalled")
                streamSocket = null
                scheduleReconnect()
            }
            reconnectHandler.postDelayed(this, 500L)
        }
    }

    private val okHttpClient = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .build()

    private var connected = false
    private var decoder: H264Decoder? = null
    private val decoderLock = Any()
    private var surfaceReady = false
    private var pendingStreamStart = false
    private var decoderSurface: Surface? = null
    private val streamReconnectRunnable = Runnable {
        if (connected && streamSocket == null && !pendingStreamStart) {
            startH264Stream()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        applyLanguage(selectedLanguageCode())
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContentView(R.layout.activity_main)

        serverIpInput = findViewById(R.id.serverIpInput)
        displayInput = findViewById(R.id.displayInput)
        modeSpinner = findViewById(R.id.modeSpinner)
        setupModeSpinner()
        connectButton = findViewById(R.id.connectButton)
        helpButton = findViewById(R.id.helpButton)
        languageButton = findViewById(R.id.languageButton)
        menuButton = findViewById(R.id.menuButton)
        statusText = findViewById(R.id.statusText)
        logText = findViewById(R.id.logText)
        topPanel = findViewById(R.id.topPanel)
        setupScroll = findViewById(R.id.setupScroll)
        streamSurface = findViewById(R.id.streamSurface)
        streamContainer = findViewById(R.id.streamContainer)
        hudText = findViewById(R.id.hudText)
        hudToggle = findViewById(R.id.hudToggle)
        logScroll = findViewById(R.id.logScroll)
        activeStreamProfile = getString(R.string.profile_pending)

        helpButton.setOnClickListener { showFaqDialog() }
        languageButton.setOnClickListener { toggleLanguage() }
        menuButton.setOnClickListener { showQuickMenu() }
        hudToggle.setOnCheckedChangeListener { _: CompoundButton, checked: Boolean ->
            hudEnabled = checked
            hudText.visibility = if (checked && connected) View.VISIBLE else View.GONE
        }

        updateLanguageButtonLabel()

        streamSurface.holder.addCallback(object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) {
                decoderSurface = holder.surface
                surfaceReady = true
                maybeStartPendingStream("Surface ready, starting H.264 video...")
            }

            override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
                applyVideoTransform()
            }

            override fun surfaceDestroyed(holder: SurfaceHolder) {
                surfaceReady = false
                if (connected) pendingStreamStart = true
                streamSocket?.close(1000, "surface destroyed")
                streamSocket = null
                inputSocket?.close(1000, "surface destroyed")
                inputSocket = null
                decoder?.release()
                decoder = null
                decoderSurface = null
            }
        })

        // If SurfaceView was already created before callback registration,
        // bind it immediately to avoid getting stuck waiting forever.
        ensureSurfaceReady()

        streamContainer.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
            if (connected && surfaceReady) {
                applyVideoTransform()
            }
        }

        streamSurface.setOnTouchListener { _, event ->
            if (!connected) {
                return@setOnTouchListener false
            }
            sendPointerEvent(event)
            true
        }

        setupBackHandler()

        connectButton.setOnClickListener {
            try {
                if (!connected) {
                    val ip = serverIpInput.text.toString().trim().ifBlank { "127.0.0.1" }
                    val mode = if (ip == "127.0.0.1" || ip == "localhost") "USB" else "Wi-Fi"
                    val displayMode = normalizedDisplayMode()
                    reconnectAttempts = 0
                    connected = true
                    statusText.text = getString(R.string.status_connecting, mode, displayMode)
                    connectButton.text = getString(R.string.btn_disconnect)
                    appendLog("$mode: connecting to $ip in $displayMode mode")
                    startH264Stream()
                } else {
                    stopAll()
                }
            } catch (e: Exception) {
                android.util.Log.e("MainActivity", "onClick error: ${e.message}", e)
                appendLog("ERROR en onClick: ${e.javaClass.simpleName}: ${e.message}")
            }
        }
    }

    private fun isUsbTransport(hostIp: String): Boolean {
        val ip = hostIp.trim()
        return ip == "127.0.0.1" ||
            ip.equals("localhost", ignoreCase = true) ||
            ip == "::1"
    }

    private fun computeAdaptiveStreamSize(rawW: Int, rawH: Int, usb: Boolean): Pair<Int, Int> {
        val maxW = if (usb) USB_MAX_STREAM_WIDTH else WIFI_MAX_STREAM_WIDTH
        val maxH = if (usb) USB_MAX_STREAM_HEIGHT else WIFI_MAX_STREAM_HEIGHT
        val scale = minOf(maxW / rawW, maxH / rawH, 1.0)
        val w = (rawW * scale).toInt().coerceAtLeast(320) and 0x7FFFFFF0
        val h = (rawH * scale).toInt().coerceAtLeast(240) and 0x7FFFFFF0
        return w to h
    }

    private fun startH264Stream() {
        clearStreamSurface()
        streamContainer.visibility = View.VISIBLE
        setupScroll.visibility = View.GONE
        logScroll.visibility = View.GONE
        topPanel.visibility = View.GONE
        hudText.visibility = if (hudEnabled) View.VISIBLE else View.GONE
        setImmersiveMode(true)

        // Re-check now that container is visible; on some devices SurfaceView becomes
        // available only after this layout pass.
        ensureSurfaceReady()

        if (!surfaceReady) {
            pendingStreamStart = true
            appendLog("Surface not ready yet, waiting...")
            streamSurface.post {
                if (ensureSurfaceReady()) {
                    maybeStartPendingStream("Surface ready after layout, starting H.264 video...")
                }
            }
            return
        }

        pendingStreamStart = false
        if (streamSocket != null) {
            return
        }

        val metrics = resources.displayMetrics
        val rawW = metrics.widthPixels.coerceAtLeast(1)
        val rawH = metrics.heightPixels.coerceAtLeast(1)

        val ip = serverIpInput.text.toString().trim().ifBlank { "127.0.0.1" }
        val usbTransport = isUsbTransport(ip)
        val (targetW, targetH) = computeAdaptiveStreamSize(rawW, rawH, usbTransport)
        val targetFps = if (usbTransport) STREAM_FPS_USB else STREAM_FPS_WIFI
        val targetBitrate = if (usbTransport) STREAM_BITRATE_USB_KBPS else STREAM_BITRATE_WIFI_KBPS
        val transportParam = if (usbTransport) "usb" else "wifi"

        val displayMode = normalizedDisplayMode()
        val displayQuery = displayInput.text.toString().trim().toIntOrNull()?.coerceIn(0, 9)
            ?.let { "&display=$it" }
            ?: ""
        val baseUrl = "ws://$ip:9001"
        ensureInputSocket(baseUrl, displayMode, displayQuery)

        val surface = decoderSurface
        if (surface == null) {
            pendingStreamStart = true
            appendLog("Texture surface unavailable, retrying...")
            return
        }

        // Configure decoder at the exact stream dimensions so the buffer has no extra padding.
        // This eliminates the "top-left corner only" issue caused by a 1920x1088 buffer for a
        // 1280x720 stream being stretched to fill the view.
        decoderTargetW = targetW
        decoderTargetH = targetH
        lastPicW = 0; lastPicH = 0; lastBufW = 0; lastBufH = 0; lastCropL = 0; lastCropT = 0

        synchronized(decoderLock) {
            decoder?.release()
            decoder = createDecoder(surface)
        }

        val fitMode = if (displayMode == "mirror") "contain" else "cover"
        appendLog("Adaptive stream request: ${targetW}x${targetH} @ ${targetFps}fps (${transportParam}, device ${rawW}x${rawH})")
        val streamUrl = "$baseUrl/h264?w=$targetW&h=$targetH&fps=$targetFps&bitrate_kbps=$targetBitrate&fit=$fitMode&mode=$displayMode&profile=auto&transport=$transportParam$displayQuery"
        val request = Request.Builder().url(streamUrl).build()

        reconnectHandler.removeCallbacks(streamStallWatchdog)
        minObservedClockDeltaMs = Long.MAX_VALUE
        emaE2eMs = 0f
        hasReceivedVideoChunk = false
        lastVideoChunkAtMs = SystemClock.elapsedRealtime()
        streamRestartRequested = false
        reconnectHandler.postDelayed(streamStallWatchdog, 500L)

        streamSocket = okHttpClient.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                reconnectAttempts = 0
                hasReceivedVideoChunk = false
                lastVideoChunkAtMs = SystemClock.elapsedRealtime()
                activeStreamProfile = getString(R.string.profile_waiting_host_cfg)
                runOnUiThread { statusText.text = getString(R.string.status_stream_active) }
            }

            override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                hasReceivedVideoChunk = true
                lastVideoChunkAtMs = SystemClock.elapsedRealtime()
                synchronized(decoderLock) {
                    decoder?.feed(bytes.toByteArray())
                }
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                if (text.startsWith("T:")) {
                    val hostUs = text.removePrefix("T:").toLongOrNull() ?: return
                    val observedDeltaMs = System.currentTimeMillis() - hostUs / 1000L
                    // Track best (smallest) observed delta as clock-offset baseline.
                    // Remaining delta approximates transport + encode/decode latency.
                    if (observedDeltaMs < minObservedClockDeltaMs) {
                        minObservedClockDeltaMs = observedDeltaMs
                    }
                    val e2eMs = (observedDeltaMs - minObservedClockDeltaMs).coerceAtLeast(0L)
                    val alpha = 0.3f
                    emaE2eMs = if (emaE2eMs < 1f) e2eMs.toFloat()
                               else alpha * e2eMs.toFloat() + (1f - alpha) * emaE2eMs
                    return
                }

                if (text.startsWith("CFG:")) {
                    val newProfile = formatHostProfile(text.removePrefix("CFG:"))
                    val changed = newProfile != activeStreamProfile
                    activeStreamProfile = newProfile
                    if (changed) {
                        runOnUiThread { appendLog(getString(R.string.profile_changed_log, activeStreamProfile)) }
                    }
                    return
                }

                if (text == "RESET") {
                    if (connected) {
                        requestStreamReconnect("host requested stream restart")
                    }
                    return
                }

                val msgObj = try {
                    JSONObject(text)
                } catch (_: Exception) {
                    null
                }
                if (msgObj?.optString("type") == "error") {
                    val msg = msgObj.optString("message", "stream error")
                    runOnUiThread {
                        appendLog("STREAM ERROR: $msg")
                        statusText.text = getString(R.string.status_stream_error, msg)
                        if (msg.contains("No H.264 encoder available")) {
                            // Keep the session alive and retry automatically so profile
                            // hot-apply does not force the user to go back to Connect.
                            streamSocket = null
                            scheduleReconnect()
                        }
                    }
                } else {
                    runOnUiThread { appendLog("WS: $text") }
                }
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                runOnUiThread {
                    if (streamSocket !== webSocket) return@runOnUiThread
                    appendLog(formatSocketFailure("Stream", currentServerHost(), t))
                    streamSocket = null
                    if (connected && !pendingStreamStart) scheduleReconnect()
                    else if (!connected) stopVisualStreamingState()
                }
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                runOnUiThread {
                    if (streamSocket !== webSocket) return@runOnUiThread
                    streamSocket = null
                    if (!connected) {
                        stopVisualStreamingState()
                    } else if (!pendingStreamStart) {
                        // Unexpected close (not from surface destroy/orientation change)
                        appendLog("Stream closed: $reason")
                        scheduleReconnect()
                    }
                    // If pendingStreamStart, surfaceCreated will call maybeStartPendingStream
                }
            }
        })
    }

    private fun maybeStartPendingStream(logMessage: String) {
        if (!connected || !pendingStreamStart || streamSocket != null) {
            return
        }
        runOnUiThread {
            appendLog(logMessage)
            statusText.text = getString(R.string.status_starting_video)
        }
        startH264Stream()
    }

    private fun stopVisualStreamingState() {
        clearStreamSurface()
        streamContainer.visibility = View.GONE
        setupScroll.visibility = View.VISIBLE
        logScroll.visibility = View.VISIBLE
        topPanel.visibility = View.VISIBLE
        setImmersiveMode(false)
    }

    private fun stopAll() {
        reconnectHandler.removeCallbacksAndMessages(null)
        reconnectAttempts = 0
        streamRestartRequested = false
        queuedMovePayload = null
        moveFlushScheduled = false
        lastMoveSentAtMs = 0L
        inputRttMs = -1L
        reconnectHandler.removeCallbacks(inputMoveFlushRunnable)
        reconnectHandler.removeCallbacks(inputPingRunnable)
        hasReceivedVideoChunk = false
        lastVideoChunkAtMs = 0L
        streamSocket?.close(1000, "disconnected by user")
        streamSocket = null
        inputSocket?.close(1000, "disconnected by user")
        inputSocket = null
        pendingStreamStart = false
        synchronized(decoderLock) {
            decoder?.release()
            decoder = null
        }
        decoderSurface = null
        connected = false
        runOnUiThread {
            stopVisualStreamingState()
            statusText.text = getString(R.string.status_disconnected)
            connectButton.text = getString(R.string.btn_connect)
        }
    }

    private fun setImmersiveMode(enabled: Boolean) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(!enabled)
            val controller = window.insetsController
            if (enabled) {
                controller?.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
                controller?.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            } else {
                controller?.show(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
            }
        } else {
            @Suppress("DEPRECATION")
            if (enabled) {
                window.decorView.systemUiVisibility =
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
                        View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                        View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
                        View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or
                        View.SYSTEM_UI_FLAG_FULLSCREEN or
                        View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
            } else {
                window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_LAYOUT_STABLE
            }
        }
    }

    private fun setupBackHandler() {
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (connected) stopAll() else finishAffinity()
            }
        })
    }

    override fun onDestroy() {
        stopAll()
        okHttpClient.dispatcher.executorService.shutdown()
        super.onDestroy()
    }

    private fun scheduleReconnect() {
        if (!connected) return
        reconnectHandler.removeCallbacks(streamReconnectRunnable)
        // Exponential backoff: 1s, 2s, 4s, 8s … capped at 30s
        val delayMs = minOf(1000L shl reconnectAttempts.coerceAtMost(4), 30_000L)
        reconnectAttempts++
        statusText.text = getString(R.string.status_reconnecting, reconnectAttempts, delayMs / 1000)
        appendLog("Reconnecting in ${delayMs}ms (attempt $reconnectAttempts)")
        reconnectHandler.postDelayed(streamReconnectRunnable, delayMs)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        if (!connected) {
            return
        }
        lastPicW = 0
        lastPicH = 0
        lastBufW = 0
        lastBufH = 0
        lastCropL = 0
        lastCropT = 0
        streamSocket?.close(1000, "orientation change")
        streamSocket = null
        pendingStreamStart = true
        streamContainer.post {
            if (connected && pendingStreamStart) {
                maybeStartPendingStream("Orientation changed, restarting stream...")
            }
        }
    }

    private fun appendLog(line: String) {
        logText.append("\n$line")
    }

    private fun currentServerHost(): String {
        return serverIpInput.text.toString().trim().ifBlank { "127.0.0.1" }
    }

    private fun isUsbLoopbackHost(host: String): Boolean {
        return host == "127.0.0.1" || host.equals("localhost", ignoreCase = true)
    }

    private fun formatSocketFailure(channel: String, host: String, t: Throwable): String {
        val simple = t.javaClass.simpleName
        return if (simple == "ConnectException" && isUsbLoopbackHost(host)) {
            "$channel error: ConnectException (missing adb reverse tcp:9001 tcp:9001 or host is not running)"
        } else {
            "$channel error: ${t.message ?: simple}"
        }
    }

    private fun ensureInputSocket(baseUrl: String, displayMode: String, displayQuery: String) {
        if (inputSocket != null) {
            return
        }

        val request = Request.Builder()
            .url("$baseUrl/input?mode=$displayMode$displayQuery")
            .build()

        inputSocket = okHttpClient.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                runOnUiThread {
                    appendLog("Input channel active")
                    reconnectHandler.removeCallbacks(inputPingRunnable)
                    reconnectHandler.postDelayed(inputPingRunnable, 500L)
                }
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                // Handle pong for RTT measurement; all other messages fall through to log.
                try {
                    val obj = JSONObject(text)
                    if (obj.optString("type") == "pong") {
                        val tsMs = obj.optLong("ts_ms", -1L)
                        if (tsMs > 0L) inputRttMs = System.currentTimeMillis() - tsMs
                        return
                    }
                } catch (_: Exception) {}
                runOnUiThread { appendLog("INPUT: $text") }
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                runOnUiThread {
                    // "unexpected end of stream" is normal during FFmpeg restarts; log briefly.
                    appendLog(formatSocketFailure("Input", currentServerHost(), t))
                    inputSocket = null
                    // Auto-retry after 3 s if still logically connected.
                    if (connected) {
                        reconnectHandler.postDelayed({
                            if (connected && inputSocket == null) {
                                val ip = currentServerHost()
                                val mode = normalizedDisplayMode()
                                val query = displayInput.text.toString().trim()
                                    .toIntOrNull()?.coerceIn(0, 9)?.let { "&display=$it" } ?: ""
                                ensureInputSocket("ws://$ip:9001", mode, query)
                            }
                        }, 3000L)
                    }
                }
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                runOnUiThread {
                    appendLog("Input closed: $reason")
                    inputSocket = null
                }
            }
        })
    }

    private fun sendPointerEvent(event: MotionEvent) {
        val socket = inputSocket ?: return
        val width = streamSurface.width.takeIf { it > 0 } ?: return
        val height = streamSurface.height.takeIf { it > 0 } ?: return

        val phase = when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> "down"
            MotionEvent.ACTION_MOVE -> "move"
            MotionEvent.ACTION_UP,
            MotionEvent.ACTION_CANCEL -> "up"
            else -> return
        }

        val payload = JSONObject()
            .put("phase", phase)
            .put("x_norm", (event.x / width.toFloat()).coerceIn(0f, 1f))
            .put("y_norm", (event.y / height.toFloat()).coerceIn(0f, 1f))
        val payloadText = payload.toString()

        if (phase != "move") {
            queuedMovePayload = null
            moveFlushScheduled = false
            reconnectHandler.removeCallbacks(inputMoveFlushRunnable)
            if (socket.send(payloadText)) {
                lastMoveSentAtMs = SystemClock.elapsedRealtime()
            }
            return
        }

        val now = SystemClock.elapsedRealtime()
        val elapsed = now - lastMoveSentAtMs
        if (!moveFlushScheduled && elapsed >= INPUT_MOVE_SEND_INTERVAL_MS) {
            if (socket.send(payloadText)) {
                lastMoveSentAtMs = now
            }
            return
        }

        queuedMovePayload = payloadText
        if (!moveFlushScheduled) {
            moveFlushScheduled = true
            val delay = (INPUT_MOVE_SEND_INTERVAL_MS - elapsed).coerceAtLeast(1L)
            reconnectHandler.postDelayed(inputMoveFlushRunnable, delay)
        }
    }

    private fun clearStreamSurface() {
        // Keep the parent container black so letterbox bars are clean and consistent.
        // Paint the parent container instead to keep a black backdrop safely.
        streamContainer.setBackgroundColor(Color.BLACK)
        // Reset SurfaceView to fill the container so the next connection starts fresh.
        val params = streamSurface.layoutParams as FrameLayout.LayoutParams
        params.width  = FrameLayout.LayoutParams.MATCH_PARENT
        params.height = FrameLayout.LayoutParams.MATCH_PARENT
        params.leftMargin = 0
        params.topMargin = 0
        params.rightMargin = 0
        params.bottomMargin = 0
        streamSurface.layoutParams = params
        streamSurface.x = 0f
        streamSurface.y = 0f
    }

    private fun ensureSurfaceReady(): Boolean {
        val surface = streamSurface.holder.surface
        if (surface != null && surface.isValid) {
            decoderSurface = surface
            surfaceReady = true
            return true
        }
        return false
    }

    private fun normalizedDisplayMode(): String {
        val raw = modeSpinner.selectedItem?.toString()?.trim()?.lowercase() ?: "mirror"
        return if (raw == "extended" || raw == "extendido") "extended" else "mirror"
    }

    private fun setupModeSpinner() {
        val adapter = ArrayAdapter.createFromResource(
            this,
            R.array.display_modes,
            R.layout.spinner_item_white
        )
        adapter.setDropDownViewResource(R.layout.spinner_dropdown_item_white)
        modeSpinner.adapter = adapter
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.JELLY_BEAN) {
            modeSpinner.setPopupBackgroundResource(R.drawable.spinner_popup_bg)
        }
    }

    private fun formatHostProfile(cfg: String): String {
        val map = mutableMapOf<String, String>()
        cfg.split(';').forEach { part ->
            val trimmed = part.trim()
            val eq = trimmed.indexOf('=')
            if (eq > 0 && eq < trimmed.length - 1) {
                val key = trimmed.substring(0, eq).trim().lowercase()
                val value = trimmed.substring(eq + 1).trim()
                map[key] = value
            }
        }

        val encoderRaw = map["encoder"] ?: "auto"
        val encoderLabel = when (encoderRaw) {
            "h264_amf" -> "AMF H.264"
            "h264_nvenc" -> "NVENC H.264"
            "h264_qsv" -> "QSV H.264"
            "libx264" -> "x264 (CPU)"
            else -> encoderRaw
        }
        val captureRaw = map["capture"] ?: "?"
        val captureLabel = when (captureRaw.lowercase()) {
            "ddagrab" -> "DXGI"
            "gdigrab" -> "GDI"
            else -> captureRaw
        }
        val profileMode = map["profile"] ?: "auto"
        val transport = map["transport"] ?: "?"
        val resolution = "${map["w"] ?: "?"}x${map["h"] ?: "?"}"
        val fps = map["fps"] ?: "?"
        val bitrate = map["bitrate_kbps"] ?: "?"

        return getString(
            R.string.profile_active_format_extended,
            encoderLabel,
            captureLabel,
            resolution,
            fps,
            bitrate
        ) + "  •  $profileMode/$transport"
    }

    /**
     * Applies a letterbox/pillarbox transform so the decoded video picture fills the view
     * correctly regardless of the hardware decoder's internal buffer padding.
     *
     * Must be called on the main thread after both (a) the view has been laid out
     * (streamSurface.width > 0) and (b) FORMAT_CHANGED has fired (lastPicW > 0).
     * Uses lastBufW/H, lastPicW/H, lastCropL/T stored when FORMAT_CHANGED arrived.
     *
     * Falls back to decoderTargetW/H when the decoder hasn't reported dimensions yet.
     */
    private fun applyVideoTransform() {
        if (!surfaceReady) return

        val viewW = streamContainer.width
        val viewH = streamContainer.height
        val srcW = lastPicW.takeIf { it > 0 } ?: decoderTargetW
        val srcH = lastPicH.takeIf { it > 0 } ?: decoderTargetH

        if (viewW <= 0 || viewH <= 0 || srcW <= 0 || srcH <= 0) {
            // Fallback before first layout/format: fill parent to avoid blank view.
            val fallback = streamSurface.layoutParams as FrameLayout.LayoutParams
            fallback.width = FrameLayout.LayoutParams.MATCH_PARENT
            fallback.height = FrameLayout.LayoutParams.MATCH_PARENT
            fallback.gravity = Gravity.CENTER
            fallback.leftMargin = 0
            fallback.topMargin = 0
            fallback.rightMargin = 0
            fallback.bottomMargin = 0
            streamSurface.layoutParams = fallback
            streamSurface.x = 0f
            streamSurface.y = 0f
            return
        }

        // Keep aspect ratio in the client (contain): no stretch.
        val scale = minOf(viewW.toFloat() / srcW.toFloat(), viewH.toFloat() / srcH.toFloat())
        val dstW = (srcW * scale).toInt().coerceAtLeast(1)
        val dstH = (srcH * scale).toInt().coerceAtLeast(1)

        val params = streamSurface.layoutParams as FrameLayout.LayoutParams
        params.width = dstW
        params.height = dstH
        params.gravity = Gravity.CENTER
        params.leftMargin = 0
        params.topMargin = 0
        params.rightMargin = 0
        params.bottomMargin = 0
        streamSurface.layoutParams = params
        // SurfaceView can ignore gravity on some vendor implementations when x/y were
        // previously forced to 0. Center it explicitly so letterbox bars are symmetric.
        streamSurface.x = ((viewW - dstW) / 2f).coerceAtLeast(0f)
        streamSurface.y = ((viewH - dstH) / 2f).coerceAtLeast(0f)

        android.util.Log.i(
            "VideoTransform",
            "surface=match_parent mode=${normalizedDisplayMode()} pic=${lastPicW}x${lastPicH} target=${decoderTargetW}x${decoderTargetH}"
        )
    }

    private fun createDecoder(surface: Surface): H264Decoder {
        return H264Decoder(surface, DECODER_MAX_WIDTH, DECODER_MAX_HEIGHT) { fps, latencyMs, bufW, bufH, picW, picH, cropL, cropT, rxKbps ->
            runOnUiThread {
                val e2e = emaE2eMs.toLong()
                val rtt = inputRttMs
                val rttStr = if (rtt >= 0L) "  •  ${rtt}ms rtt" else ""
                val videoLine = if (e2e in 1L..5000L)
                    "%.0f dec fps  •  %dms dec  •  %dms e2e$rttStr".format(fps, latencyMs, e2e)
                else
                    "%.0f dec fps  •  %dms dec$rttStr".format(fps, latencyMs)
                if (picW > 0 && picH > 0) {
                    lastBufW = bufW; lastBufH = bufH
                    lastPicW = picW; lastPicH = picH
                    lastCropL = cropL; lastCropT = cropT
                    applyVideoTransform()
                }
                val streamLine = if (picW > 0 && picH > 0)
                    "${picW}x${picH}  •  ${rxKbps} kbps rx"
                else
                    getString(R.string.resolution_pending, rxKbps)
                if (hudEnabled) {
                    hudText.text = "$videoLine\n$streamLine\n$activeStreamProfile"
                    hudText.visibility = View.VISIBLE
                } else {
                    hudText.visibility = View.GONE
                }
            }
        }
    }

    private fun showFaqDialog() {
        val faq = Html.fromHtml(getString(R.string.help_content), Html.FROM_HTML_MODE_LEGACY)
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.help_title))
            .setMessage(faq)
            .setPositiveButton("OK", null)
            .show()
    }

    private fun toggleLanguage() {
        val newCode = if (selectedLanguageCode() == "es") "en" else "es"
        saveLanguageCode(newCode)
        if (connected) {
            stopAll()
        }
        recreate()
    }

    private fun updateLanguageButtonLabel() {
        languageButton.text = if (selectedLanguageCode() == "es") "EN" else "ES"
    }

    private fun selectedLanguageCode(): String {
        val prefs = getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        val stored = prefs.getString(PREF_LANGUAGE, null)
        if (stored == "es" || stored == "en") {
            return stored
        }
        return "en"
    }

    private fun saveLanguageCode(code: String) {
        getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .edit()
            .putString(PREF_LANGUAGE, code)
            .apply()
    }

    private fun applyLanguage(code: String) {
        val locale = java.util.Locale(code)
        java.util.Locale.setDefault(locale)
        val cfg = Configuration(resources.configuration)
        @Suppress("DEPRECATION")
        cfg.setLocale(locale)
        @Suppress("DEPRECATION")
        resources.updateConfiguration(cfg, resources.displayMetrics)
    }

    private fun showQuickMenu() {
        val options = arrayOf(
            getString(R.string.menu_quick_select_monitor),
            getString(R.string.menu_quick_reset_latency),
            getString(R.string.menu_quick_show_logs),
            getString(R.string.menu_quick_reconnect)
        )
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.menu_quick_title))
            .setItems(options) { _, which ->
                when (which) {
                    0 -> showDetectedMonitorPicker()
                    1 -> {
                        minObservedClockDeltaMs = Long.MAX_VALUE
                        emaE2eMs = 0f
                        appendLog("E2E latency baseline reset")
                    }
                    2 -> {
                        if (connected) {
                            streamContainer.visibility = View.GONE
                            setupScroll.visibility = View.VISIBLE
                            logScroll.visibility = View.VISIBLE
                            topPanel.visibility = View.VISIBLE
                            setImmersiveMode(false)
                            appendLog("Log view opened")
                        }
                    }
                    3 -> {
                        if (connected) {
                            requestStreamReconnect("menu: manual reconnect")
                        }
                    }
                }
            }
            .setNegativeButton(getString(R.string.menu_cancel), null)
            .show()
    }

    private fun showDetectedMonitorPicker() {
        val host = currentServerHost()
        val request = Request.Builder().url("http://$host:9001/displays").build()
        okHttpClient.newCall(request).enqueue(object : Callback {
            override fun onFailure(call: Call, e: IOException) {
                runOnUiThread {
                    appendLog(getString(R.string.monitor_fetch_error, e.message ?: "network error"))
                }
            }

            override fun onResponse(call: Call, response: Response) {
                val body = response.body?.string().orEmpty()
                if (!response.isSuccessful || body.isBlank()) {
                    runOnUiThread {
                        appendLog(getString(R.string.monitor_fetch_error, "http ${response.code}"))
                    }
                    return
                }

                val arr = try {
                    JSONArray(body)
                } catch (_: Exception) {
                    null
                }

                if (arr == null || arr.length() == 0) {
                    runOnUiThread {
                        appendLog(getString(R.string.monitor_none_found))
                    }
                    return
                }

                val labels = Array(arr.length()) { i ->
                    val item = arr.getJSONObject(i)
                    val idx = item.optInt("index", i)
                    val w = item.optInt("width", 0)
                    val h = item.optInt("height", 0)
                    val primary = item.optBoolean("is_primary", false)
                    val tag = if (primary) getString(R.string.monitor_primary_tag) else getString(R.string.monitor_secondary_tag)
                    "#$idx  ${w}x${h}  ($tag)"
                }

                runOnUiThread {
                    AlertDialog.Builder(this@MainActivity)
                        .setTitle(getString(R.string.monitor_picker_title))
                        .setItems(labels) { _, which ->
                            val picked = arr.getJSONObject(which).optInt("index", which)
                            displayInput.setText(picked.toString())
                            appendLog(getString(R.string.monitor_selected_log, picked))
                        }
                        .setNegativeButton(getString(R.string.menu_cancel), null)
                        .show()
                }
            }
        })
    }

    private fun requestStreamReconnect(reason: String) {
        if (!connected) return
        if (streamRestartRequested) return
        streamRestartRequested = true
        hasReceivedVideoChunk = false
        lastVideoChunkAtMs = SystemClock.elapsedRealtime()
        reconnectHandler.removeCallbacks(streamStallWatchdog)
        reconnectHandler.removeCallbacks(streamReconnectRunnable)
        runOnUiThread {
            appendLog("Reabriendo stream: $reason")
            statusText.text = getString(R.string.status_reopened_stream)
        }

        val socket = streamSocket
        streamSocket = null
        socket?.close(1012, reason)

        reconnectHandler.postDelayed(streamReconnectRunnable, 150L)
    }

}

private class H264Decoder(
    private val surface: Surface,
    private val width: Int,
    private val height: Int,
    private val onHudUpdate: (fps: Float, latencyMs: Long, bufW: Int, bufH: Int, picW: Int, picH: Int, cropL: Int, cropT: Int, rxKbps: Long) -> Unit
) {
    private val codec: MediaCodec = MediaCodec.createDecoderByType("video/avc")
    private val parser = AnnexBParser { nal -> queueNal(nal) }
    private val startCode = byteArrayOf(0, 0, 0, 1)

    // Drain thread: blocks inside dequeueOutputBuffer until a frame is ready.
    // URGENT_DISPLAY priority: Android scheduler will not demote this thread,
    // preventing the 1-2 frame stalls that caused sub-60fps bursts.
    private val drainThread = Thread {
        android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_DISPLAY)
        while (!Thread.currentThread().isInterrupted) {
            if (codecStarted) drainOutput()
            else try { Thread.sleep(5) } catch (_: InterruptedException) { break }
        }
    }.apply { isDaemon = true; name = "h264-drain"; start() }

    // Queue access units (full frame payloads) instead of individual NAL units.
    // Sending single slices separately can produce green/pink macroblock artifacts
    // on some Android hardware decoders.
    // Queue depth 1: a single slot between the parser thread and the submit thread.
    // When the slot is full, the current frame replaces the queued one (drop-oldest).
    // This removes up to one full frame (~16ms) of buffer latency from the pipeline.
    private val auQueue = java.util.concurrent.ArrayBlockingQueue<ByteArray>(1)
    private val submitThread = Thread {
        android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_AUDIO)
        while (!Thread.currentThread().isInterrupted) {
            try {
                val accessUnit = auQueue.take()
                submitAccessUnit(accessUnit)
            } catch (_: InterruptedException) { break }
        }
    }.apply { isDaemon = true; name = "h264-submit"; start() }

    // Codec is NOT started yet — we wait for SPS+PPS from the stream so we can
    // configure it with csd-0/csd-1. Many Qualcomm hardware decoders require
    // explicit parameter sets in the MediaFormat and ignore inline SPS/PPS.
    @Volatile private var codecStarted = false
    @Volatile private var codecStartFailed = false
    private var pendingSps: ByteArray? = null
    private var pendingPps: ByteArray? = null
    private val currentAu = java.io.ByteArrayOutputStream(256 * 1024)
    private var sawAud = false
    private var nalCount = 0
    private var missingParamWarned = false
    private var triedNoCsdStart = false

    private var framesRendered = 0
    private var totalBytesReceived = 0L
    private var hudLastMs = System.currentTimeMillis()
    private var hudFrameCount = 0
    private var bytesAtLastHud = 0L
    @Volatile private var outputWidth = -1
    @Volatile private var outputHeight = -1
    @Volatile private var bufferWidth = -1
    @Volatile private var bufferHeight = -1
    @Volatile private var cropLeft = 0
    @Volatile private var cropTop = 0
    @Volatile private var pictureWidth = -1
    @Volatile private var pictureHeight = -1
    // EMA-smoothed values — prevent the HUD from flickering due to measurement noise.
    // Alpha 0.25: new sample contributes 25%, history 75%. Smooths ±1-frame window jitter.
    private var emaFps = 0f
    private var emaLatencyMs = 0f

    init {
        android.util.Log.i("H264Decoder", "Decoder created ${width}x${height}, waiting for SPS+PPS")
    }

    fun feed(chunk: ByteArray) {
        totalBytesReceived += chunk.size
        if (totalBytesReceived <= chunk.size) {
            android.util.Log.i("H264Decoder", "First WebSocket chunk: ${chunk.size} bytes")
        }
        parser.push(chunk)
    }

    private fun queueNal(nal: ByteArray) {
        if (nal.isEmpty()) return
        val nalType = nal[0].toInt() and 0x1F
        nalCount++
        if (nalCount <= 20 || nalCount % 120 == 0 || nalType == 5 || nalType == 7 || nalType == 8) {
            android.util.Log.v("H264Decoder", "NAL #$nalCount type=$nalType size=${nal.size}")
        }

        if (codecStartFailed) {
            return
        }

        // AUD delimits access units (frames). Flush previous frame at each AUD.
        if (nalType == 9) {
            sawAud = true
            flushCurrentAuToQueue()
        }

        when (nalType) {
            7 -> {
                pendingSps = nal
                android.util.Log.i("H264Decoder", "Got SPS (${nal.size}B)")
                return
            }
            8 -> {
                pendingPps = nal
                android.util.Log.i("H264Decoder", "Got PPS (${nal.size}B)")
                return
            }
        }

        if (!codecStarted && pendingSps != null && pendingPps != null) {
            if (!tryStartCodec(pendingSps!!, pendingPps!!, withCsd = true)) {
                codecStartFailed = true
                android.util.Log.e("H264Decoder", "Codec permanently failed to start; video disabled for this session")
                return
            }
        }

        if (!codecStarted && (pendingSps == null || pendingPps == null)) {
            if (!missingParamWarned && nalCount >= 30) {
                android.util.Log.w("H264Decoder", "Still waiting SPS/PPS after $nalCount NALs; trying no-csd start")
                missingParamWarned = true
            }
            if (!triedNoCsdStart && nalCount >= 30) {
                triedNoCsdStart = true
                if (!tryStartCodec(null, null, withCsd = false)) {
                    codecStartFailed = true
                    android.util.Log.e("H264Decoder", "Codec failed to start without CSD; video disabled for this session")
                    return
                }
            }
            if (!codecStarted) {
                return
            }
        }

        // Append non-parameter NAL to current access unit.
        currentAu.write(startCode)
        currentAu.write(nal)

        // Do not flush on every slice when AUD is missing; multi-slice frames would be
        // split into partial access units and can produce pink/green macroblock artifacts.
        // The host emits AUD (aud=1), so frame boundaries should come from nalType 9.
    }

    private fun flushCurrentAuToQueue() {
        if (currentAu.size() <= startCode.size) {
            currentAu.reset()
            return
        }
        val accessUnit = currentAu.toByteArray()
        currentAu.reset()
        if (!codecStarted) {
            return
        }
        if (!auQueue.offer(accessUnit)) {
            auQueue.poll()
            auQueue.offer(accessUnit)
        }
    }

    private fun submitAccessUnit(accessUnit: ByteArray) {
        // Block up to 4ms to get an input buffer slot — runs on the dedicated submit
        // thread, not the WebSocket thread, so blocking here is safe and prevents
        // the P-frame drops that caused systematic sub-60fps delivery.
        val index = codec.dequeueInputBuffer(4000)
        if (index < 0) {
            android.util.Log.w("H264Decoder", "AU dropped — no input buffer in 4ms")
            return
        }
        val input = codec.getInputBuffer(index) ?: run {
            codec.queueInputBuffer(index, 0, 0, 0, 0); return
        }
        if (accessUnit.size > input.capacity()) {
            android.util.Log.w("H264Decoder",
                "AU too large: ${accessUnit.size} > ${input.capacity()}")
            codec.queueInputBuffer(index, 0, 0, 0, 0)
            return
        }
        input.clear()
        input.put(accessUnit)
        codec.queueInputBuffer(index, 0, accessUnit.size, System.nanoTime() / 1000, 0)
    }

    private fun tryStartCodec(sps: ByteArray?, pps: ByteArray?, withCsd: Boolean): Boolean {
        if (tryStartCodecAt(sps, pps, withCsd, width, height)) return true
        if (width > 1280 || height > 720) {
            android.util.Log.w("H264Decoder", "Retrying codec at 1280x720 compatibility fallback")
            return tryStartCodecAt(sps, pps, withCsd, 1280, 720)
        }
        return false
    }

    private fun tryStartCodecAt(
        sps: ByteArray?,
        pps: ByteArray?,
        withCsd: Boolean,
        decodeW: Int,
        decodeH: Int
    ): Boolean {
        val attempts = listOf(true, false)
        for (fullConfig in attempts) {
            try {
                val format = MediaFormat.createVideoFormat("video/avc", decodeW, decodeH)
                format.setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 2 * 1024 * 1024)
                if (fullConfig) {
                    format.setInteger(MediaFormat.KEY_PRIORITY, 0)
                    format.setFloat(MediaFormat.KEY_OPERATING_RATE, 60f)
                    format.setInteger(MediaFormat.KEY_FRAME_RATE, 60)
                }
                if (withCsd && sps != null && pps != null) {
                    format.setByteBuffer("csd-0", java.nio.ByteBuffer.wrap(startCode + sps))
                    format.setByteBuffer("csd-1", java.nio.ByteBuffer.wrap(startCode + pps))
                }
                codec.configure(format, surface, null, 0)
                codec.start()
                codecStarted = true
                val mode = when {
                    withCsd && fullConfig -> "full"
                    withCsd && !fullConfig -> "fallback"
                    !withCsd && fullConfig -> "no-csd-full"
                    else -> "no-csd-fallback"
                }
                android.util.Log.i("H264Decoder", "Codec started ($mode) at ${decodeW}x${decodeH}")
                return true
            } catch (e: Exception) {
                android.util.Log.e(
                    "H264Decoder",
                    "Codec start failed at ${decodeW}x${decodeH}: ${e.javaClass.simpleName}: ${e.message}"
                )
                try {
                    codec.reset()
                } catch (_: Throwable) {
                }
            }
        }
        return false
    }

    private fun drainOutput() {
        val info = MediaCodec.BufferInfo()
        // Block up to 4ms waiting for the next decoded frame — avoids CPU spinning.
        // When the HW decoder has output ready it returns immediately; if nothing is
        // ready within 4ms we loop back and block again. Then drain any further
        // already-decoded frames non-blocking so bursts are fully consumed in one pass.
        var timeout = 2000L // µs — first call blocks
        while (true) {
            when (val outIndex = codec.dequeueOutputBuffer(info, timeout)) {
                MediaCodec.INFO_TRY_AGAIN_LATER -> break
                MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                    val fmt = codec.outputFormat
                    val bW = if (fmt.containsKey(MediaFormat.KEY_WIDTH)) fmt.getInteger(MediaFormat.KEY_WIDTH) else -1
                    val bH = if (fmt.containsKey(MediaFormat.KEY_HEIGHT)) fmt.getInteger(MediaFormat.KEY_HEIGHT) else -1
                    // Some hardware decoders allocate a buffer larger than the picture and
                    // report the visible region via crop keys. If absent, the full buffer is
                    // the picture (common on software decoders and stock Qualcomm).
                    val cL = if (fmt.containsKey("crop-left")) fmt.getInteger("crop-left") else 0
                    val cT = if (fmt.containsKey("crop-top")) fmt.getInteger("crop-top") else 0
                    val cR = if (fmt.containsKey("crop-right")) fmt.getInteger("crop-right") else (if (bW > 0) bW - 1 else -1)
                    val cB = if (fmt.containsKey("crop-bottom")) fmt.getInteger("crop-bottom") else (if (bH > 0) bH - 1 else -1)
                    val pW = if (cR >= 0 && cL >= 0) cR - cL + 1 else bW
                    val pH = if (cB >= 0 && cT >= 0) cB - cT + 1 else bH
                    bufferWidth = bW; bufferHeight = bH
                    cropLeft = cL; cropTop = cT
                    pictureWidth = pW; pictureHeight = pH
                    outputWidth = pW; outputHeight = pH
                    android.util.Log.i("H264Decoder",
                        "Format changed: buf=${bW}x${bH} crop=(${cL},${cT},${cR},${cB}) pic=${pW}x${pH}")
                    if (pW > 0 && pH > 0) {
                        onHudUpdate(emaFps, emaLatencyMs.toLong(), bW, bH, pW, pH, cL, cT, 0L)
                    }
                }
                @Suppress("DEPRECATION")
                MediaCodec.INFO_OUTPUT_BUFFERS_CHANGED -> { /* no-op in API 21+ */ }
                else -> if (outIndex >= 0) {
                    framesRendered++
                    hudFrameCount++
                    val latencyMs = (System.nanoTime() - info.presentationTimeUs * 1000L) / 1_000_000L
                    if (framesRendered <= 5 || framesRendered % 300 == 0) {
                        android.util.Log.i("H264Decoder",
                            "Frame #$framesRendered  decode_latency=${latencyMs}ms")
                    }
                    val nowMs = System.currentTimeMillis()
                    if (nowMs - hudLastMs >= 1000) {
                        val windowMs = (nowMs - hudLastMs).coerceAtLeast(1)
                        val bytesDelta = (totalBytesReceived - bytesAtLastHud).coerceAtLeast(0L)
                        val instantRxKbps = (bytesDelta * 8L * 1000L) / windowMs / 1000L
                        bytesAtLastHud = totalBytesReceived

                        val instantFps = hudFrameCount * 1000f / windowMs
                        val alpha = 0.25f
                        emaFps = if (emaFps < 1f) instantFps else alpha * instantFps + (1f - alpha) * emaFps
                        emaLatencyMs = if (emaLatencyMs < 1f) latencyMs.toFloat()
                                       else alpha * latencyMs + (1f - alpha) * emaLatencyMs
                        onHudUpdate(emaFps, emaLatencyMs.toLong(), bufferWidth, bufferHeight, pictureWidth, pictureHeight, cropLeft, cropTop, instantRxKbps)
                        hudFrameCount = 0
                        hudLastMs = nowMs
                    }
                    codec.releaseOutputBuffer(outIndex, true)
                }
            }
            timeout = 0L // subsequent calls non-blocking — drain any queued-up frames
        }
    }

    fun release() {
        flushCurrentAuToQueue()
        submitThread.interrupt()
        drainThread.interrupt()
        try { submitThread.join(500) } catch (_: InterruptedException) { }
        try { drainThread.join(500) } catch (_: InterruptedException) { }
        try { codec.stop() } catch (_: Throwable) { }
        try { codec.release() } catch (_: Throwable) { }
    }
}

private class AnnexBParser(private val onNal: (ByteArray) -> Unit) {
    private var stash = ByteArray(0)

    fun push(chunk: ByteArray) {
        if (chunk.isEmpty()) return
        val merged = ByteArray(stash.size + chunk.size)
        System.arraycopy(stash, 0, merged, 0, stash.size)
        System.arraycopy(chunk, 0, merged, stash.size, chunk.size)
        stash = merged

        var current = findStartCode(stash, 0) ?: return
        var next = findStartCode(stash, current.first + current.second)

        while (next != null) {
            val nalStart = current.first + current.second
            val nalEnd = next.first
            if (nalEnd > nalStart) {
                val nal = stash.copyOfRange(nalStart, nalEnd)
                onNal(nal)
            }
            current = next
            next = findStartCode(stash, current.first + current.second)
        }

        stash = stash.copyOfRange(current.first, stash.size)
        if (stash.size > 2_000_000) {
            stash = stash.copyOfRange(stash.size - 200_000, stash.size)
        }
    }

    private fun findStartCode(data: ByteArray, from: Int): Pair<Int, Int>? {
        var i = from
        while (i + 2 < data.size) {
            if (data[i] == 0.toByte() && data[i + 1] == 0.toByte()) {
                if (data[i + 2] == 1.toByte()) {
                    return i to 3
                }
                if (i + 3 < data.size && data[i + 2] == 0.toByte() && data[i + 3] == 1.toByte()) {
                    return i to 4
                }
            }
            i++
        }
        return null
    }
}
