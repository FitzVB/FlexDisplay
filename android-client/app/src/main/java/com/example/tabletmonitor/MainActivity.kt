package com.example.tabletmonitor

import android.content.res.Configuration
import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.graphics.Color
import android.graphics.SurfaceTexture
import android.view.MotionEvent
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.widget.FrameLayout
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import org.json.JSONObject
import java.util.concurrent.TimeUnit

class MainActivity : AppCompatActivity() {

    companion object {
        // USB profile: balance image quality with low interaction latency on CPU fallback.
        private const val STREAM_TARGET_FPS = 60
        private const val STREAM_TARGET_BITRATE_KBPS = 12000
        private const val MAX_STREAM_WIDTH = 1600.0
        private const val MAX_STREAM_HEIGHT = 900.0
        // Decoder tolerance: host can now be controlled from PC with fixed presets up to 1080p.
        // Configure MediaCodec with a stable max size so hot profile changes don't black-screen
        // when host resolution differs from the initial client-requested size.
        private const val DECODER_MAX_WIDTH = 1920
        private const val DECODER_MAX_HEIGHT = 1080
        private const val INPUT_MOVE_SEND_INTERVAL_MS = 8L
    }

    private var streamSocket: WebSocket? = null
    private var inputSocket: WebSocket? = null

    private lateinit var serverIpInput: EditText
    private lateinit var displayInput: EditText
    private lateinit var modeSpinner: Spinner
    private lateinit var connectButton: Button
    private lateinit var statusText: TextView
    private lateinit var logText: TextView
    private lateinit var topPanel: LinearLayout
    private lateinit var streamSurface: TextureView
    private lateinit var streamContainer: FrameLayout
    private lateinit var hudText: TextView
    private lateinit var logScroll: ScrollView

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
    @Volatile private var activeStreamProfile = "perfil: pendiente"
    @Volatile private var streamRestartRequested = false

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
                appendLog("Stream congelado detectado (>2.5s sin video), reconectando...")
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
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContentView(R.layout.activity_main)

        serverIpInput = findViewById(R.id.serverIpInput)
        displayInput = findViewById(R.id.displayInput)
        modeSpinner = findViewById(R.id.modeSpinner)
        connectButton = findViewById(R.id.connectButton)
        statusText = findViewById(R.id.statusText)
        logText = findViewById(R.id.logText)
        topPanel = findViewById(R.id.topPanel)
        streamSurface = findViewById(R.id.streamSurface)
        streamContainer = findViewById(R.id.streamContainer)
        hudText = findViewById(R.id.hudText)
        logScroll = findViewById(R.id.logScroll)

        streamSurface.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
            override fun onSurfaceTextureAvailable(surfaceTexture: SurfaceTexture, width: Int, height: Int) {
                decoderSurface?.release()
                decoderSurface = Surface(surfaceTexture)
                surfaceReady = true
                maybeStartPendingStream("Surface lista, iniciando video H.264...")
            }

            override fun onSurfaceTextureSizeChanged(surfaceTexture: SurfaceTexture, width: Int, height: Int) {}

            override fun onSurfaceTextureDestroyed(surfaceTexture: SurfaceTexture): Boolean {
                surfaceReady = false
                // If still logically connected, keep pendingStreamStart so surfaceCreated
                // will restart the stream (e.g. on screen rotation).
                if (connected) pendingStreamStart = true
                streamSocket?.close(1000, "surface destroyed")
                streamSocket = null
                inputSocket?.close(1000, "surface destroyed")
                inputSocket = null
                decoder?.release()
                decoder = null
                decoderSurface?.release()
                decoderSurface = null
                return true
            }

            override fun onSurfaceTextureUpdated(surfaceTexture: SurfaceTexture) {}
        }

        // If TextureView was already available before listener registration,
        // bind the surface immediately to avoid getting stuck waiting forever.
        ensureTextureSurfaceReady()

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
                    statusText.text = "Estado: conectando ($mode, $displayMode)..."
                    connectButton.text = "Disconnect"
                    appendLog("$mode: conectando a $ip en modo $displayMode")
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

    private fun startH264Stream() {
        clearStreamSurface()
        streamContainer.visibility = View.VISIBLE
        logScroll.visibility = View.GONE
        topPanel.visibility = View.GONE
        setImmersiveMode(true)

        // Re-check now that container is visible; on some devices TextureView becomes
        // available only after this layout pass.
        ensureTextureSurfaceReady()

        if (!surfaceReady) {
            pendingStreamStart = true
            appendLog("Surface no lista todavia, esperando...")
            streamSurface.post {
                if (ensureTextureSurfaceReady()) {
                    maybeStartPendingStream("Surface lista tras layout, iniciando video H.264...")
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

        // USB profile cap: keep high detail while staying within common decoder limits.
        val scale = minOf(MAX_STREAM_WIDTH / rawW, MAX_STREAM_HEIGHT / rawH, 1.0)
        val targetW = (rawW * scale).toInt().coerceAtLeast(320) and 0x7FFFFFF0
        val targetH = (rawH * scale).toInt().coerceAtLeast(240) and 0x7FFFFFF0

        val ip = serverIpInput.text.toString().trim().ifBlank { "127.0.0.1" }
        val displayMode = normalizedDisplayMode()
        val displayQuery = displayInput.text.toString().trim().toIntOrNull()?.coerceIn(0, 9)
            ?.let { "&display=$it" }
            ?: ""
        val baseUrl = "ws://$ip:9001"
        ensureInputSocket(baseUrl, displayMode, displayQuery)

        val surface = decoderSurface
        if (surface == null) {
            pendingStreamStart = true
            appendLog("Surface de textura no disponible, reintentando...")
            return
        }

        synchronized(decoderLock) {
            decoder?.release()
            decoder = createDecoder(surface)
        }

        val streamUrl = "$baseUrl/h264?w=$targetW&h=$targetH&fps=$STREAM_TARGET_FPS&bitrate_kbps=$STREAM_TARGET_BITRATE_KBPS&fit=cover&mode=$displayMode$displayQuery"
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
                activeStreamProfile = "perfil: esperando cfg del host"
                runOnUiThread { statusText.text = "Estado: video H.264 activo" }
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
                    val newProfile = "perfil host: " + text.removePrefix("CFG:")
                    val changed = newProfile != activeStreamProfile
                    activeStreamProfile = newProfile
                    if (changed) {
                        runOnUiThread { appendLog(activeStreamProfile) }
                    }
                    return
                }

                if (text == "RESET") {
                    if (connected) {
                        requestStreamReconnect("reinicio de stream en host")
                    }
                    return
                }

                val msgObj = try {
                    JSONObject(text)
                } catch (_: Exception) {
                    null
                }
                if (msgObj?.optString("type") == "error") {
                    val msg = msgObj.optString("message", "error de stream")
                    runOnUiThread {
                        appendLog("STREAM ERROR: $msg")
                        statusText.text = "Estado: $msg"
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
                        appendLog("Stream cerrado: $reason")
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
            statusText.text = "Estado: iniciando video H.264..."
        }
        startH264Stream()
    }

    private fun stopVisualStreamingState() {
        clearStreamSurface()
        streamContainer.visibility = View.GONE
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
        streamSocket?.close(1000, "desconectado por usuario")
        streamSocket = null
        inputSocket?.close(1000, "desconectado por usuario")
        inputSocket = null
        pendingStreamStart = false
        synchronized(decoderLock) {
            decoder?.release()
            decoder = null
        }
        decoderSurface?.release()
        decoderSurface = null
        connected = false
        runOnUiThread {
            stopVisualStreamingState()
            statusText.text = "Estado: desconectado"
            connectButton.text = "Connect"
        }
    }

    private fun setImmersiveMode(enabled: Boolean) {
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
        statusText.text = "Estado: reconectando (${reconnectAttempts}) en ${delayMs/1000}s..."
        appendLog("Reconectando en ${delayMs}ms (intento $reconnectAttempts)")
        reconnectHandler.postDelayed(streamReconnectRunnable, delayMs)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        // Screen rotated — close the current stream so surfaceDestroyed/surfaceCreated
        // fires and restarts it with the new screen dimensions.
        if (connected && streamSocket != null) {
            streamSocket?.close(1000, "orientation change")
            streamSocket = null
            pendingStreamStart = true
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
            "$channel error: ConnectException (falta adb reverse tcp:9001 tcp:9001 o el host no esta abierto)"
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
                    appendLog("Canal de input activo")
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
                                val displayMode = normalizedDisplayMode()
                                val displayQuery = displayInput.text.toString().trim()
                                    .toIntOrNull()?.coerceIn(0, 9)?.let { "&display=$it" } ?: ""
                                ensureInputSocket("ws://$ip:9001", displayMode, displayQuery)
                            }
                        }, 3000L)
                    }
                }
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                runOnUiThread {
                    appendLog("Input cerrado: $reason")
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
        // TextureView does not support background drawables on some devices.
        // Paint the parent container instead to keep a black backdrop safely.
        streamContainer.setBackgroundColor(Color.BLACK)
    }

    private fun ensureTextureSurfaceReady(): Boolean {
        val st = streamSurface.surfaceTexture
        if (st != null && streamSurface.isAvailable) {
            if (decoderSurface == null) {
                decoderSurface = Surface(st)
            }
            surfaceReady = true
            return true
        }
        return false
    }

    private fun normalizedDisplayMode(): String {
        val raw = modeSpinner.selectedItem?.toString()?.trim()?.lowercase() ?: "mirror"
        return if (raw == "extended") "extended" else "mirror"
    }

    private fun createDecoder(surface: Surface): H264Decoder {
        return H264Decoder(surface, DECODER_MAX_WIDTH, DECODER_MAX_HEIGHT) { fps, latencyMs, outW, outH, rxKbps ->
            runOnUiThread {
                val e2e = emaE2eMs.toLong()
                val rtt = inputRttMs
                val rttStr = if (rtt >= 0L) "  •  ${rtt}ms rtt" else ""
                val videoLine = if (e2e in 1L..5000L)
                    "%.0f fps  •  %dms dec  •  %dms e2e$rttStr".format(fps, latencyMs, e2e)
                else
                    "%.0f fps  •  %dms dec$rttStr".format(fps, latencyMs)
                val streamLine = if (outW > 0 && outH > 0)
                    "${outW}x${outH}  •  ${rxKbps} kbps rx"
                else
                    "resolucion pendiente  •  ${rxKbps} kbps rx"
                hudText.text = "$videoLine\n$streamLine\n$activeStreamProfile"
            }
        }
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
            statusText.text = "Estado: reabriendo stream..."
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
    private val onHudUpdate: (fps: Float, latencyMs: Long, outW: Int, outH: Int, rxKbps: Long) -> Unit
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
            if (!tryStartCodec(pendingSps!!, pendingPps!!)) {
                codecStartFailed = true
                android.util.Log.e("H264Decoder", "Codec permanently failed to start; video disabled for this session")
                return
            }
        }

        if (!codecStarted && (pendingSps == null || pendingPps == null)) {
            // Some streams/devices can delay or omit in-band SPS/PPS.
            // Try a guarded start without csd after a short warmup.
            if (!missingParamWarned && nalCount >= 30) {
                android.util.Log.w("H264Decoder", "Still waiting SPS/PPS after $nalCount NALs; trying no-csd start")
                missingParamWarned = true
            }
            if (!triedNoCsdStart && nalCount >= 30) {
                triedNoCsdStart = true
                if (!tryStartCodecWithoutCsd()) {
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

    private fun tryStartCodec(sps: ByteArray, pps: ByteArray): Boolean {
        val attempts = listOf(true, false)
        for (fullConfig in attempts) {
            try {
                val format = MediaFormat.createVideoFormat("video/avc", width, height)
                format.setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 2 * 1024 * 1024)
                if (fullConfig) {
                    format.setInteger(MediaFormat.KEY_PRIORITY, 0)
                    format.setFloat(MediaFormat.KEY_OPERATING_RATE, 60f)
                    format.setInteger(MediaFormat.KEY_FRAME_RATE, 60)
                }
                format.setByteBuffer("csd-0", java.nio.ByteBuffer.wrap(startCode + sps))
                format.setByteBuffer("csd-1", java.nio.ByteBuffer.wrap(startCode + pps))
                codec.configure(format, surface, null, 0)
                codec.start()
                codecStarted = true
                android.util.Log.i(
                    "H264Decoder",
                    "Codec started (${if (fullConfig) "full" else "fallback"}) with SPS(${sps.size}B)+PPS(${pps.size}B)"
                )
                return true
            } catch (e: Exception) {
                android.util.Log.e(
                    "H264Decoder",
                    "Codec start failed (${if (fullConfig) "full" else "fallback"}): ${e.javaClass.simpleName}: ${e.message}"
                )
                try {
                    codec.reset()
                } catch (_: Throwable) {
                }
            }
        }
        return false
    }

    private fun tryStartCodecWithoutCsd(): Boolean {
        val attempts = listOf(true, false)
        for (fullConfig in attempts) {
            try {
                val format = MediaFormat.createVideoFormat("video/avc", width, height)
                format.setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 2 * 1024 * 1024)
                if (fullConfig) {
                    format.setInteger(MediaFormat.KEY_PRIORITY, 0)
                    format.setFloat(MediaFormat.KEY_OPERATING_RATE, 60f)
                    format.setInteger(MediaFormat.KEY_FRAME_RATE, 60)
                }
                codec.configure(format, surface, null, 0)
                codec.start()
                codecStarted = true
                android.util.Log.i(
                    "H264Decoder",
                    "Codec started (${if (fullConfig) "no-csd-full" else "no-csd-fallback"}) at ${width}x${height}"
                )
                return true
            } catch (e: Exception) {
                android.util.Log.e(
                    "H264Decoder",
                    "Codec no-csd start failed (${if (fullConfig) "full" else "fallback"}): ${e.javaClass.simpleName}: ${e.message}"
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
                    outputWidth = if (fmt.containsKey(MediaFormat.KEY_WIDTH)) fmt.getInteger(MediaFormat.KEY_WIDTH) else -1
                    outputHeight = if (fmt.containsKey(MediaFormat.KEY_HEIGHT)) fmt.getInteger(MediaFormat.KEY_HEIGHT) else -1
                    android.util.Log.d("H264Decoder", "Output format: ${codec.outputFormat}")
                }
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
                        onHudUpdate(emaFps, emaLatencyMs.toLong(), outputWidth, outputHeight, instantRxKbps)
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
