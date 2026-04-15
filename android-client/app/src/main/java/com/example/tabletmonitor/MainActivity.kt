package com.example.tabletmonitor

import android.content.res.Configuration
import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.widget.FrameLayout
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import com.example.tabletmonitor.signal.SignalMessage
import com.example.tabletmonitor.signal.SignalingClient
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import java.util.concurrent.TimeUnit

class MainActivity : AppCompatActivity() {

    private lateinit var signalingClient: SignalingClient
    private var streamSocket: WebSocket? = null

    private lateinit var serverIpInput: EditText
    private lateinit var displayInput: EditText
    private lateinit var connectButton: Button
    private lateinit var statusText: TextView
    private lateinit var logText: TextView
    private lateinit var topPanel: LinearLayout
    private lateinit var streamSurface: SurfaceView
    private lateinit var streamContainer: FrameLayout
    private lateinit var hudText: TextView
    private lateinit var logScroll: ScrollView

    private val reconnectHandler = Handler(Looper.getMainLooper())
    private var reconnectAttempts = 0

    private val okHttpClient = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .build()

    private var connected = false
    private var decoder: H264Decoder? = null
    private var surfaceReady = false
    private var pendingStreamStart = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContentView(R.layout.activity_main)

        serverIpInput = findViewById(R.id.serverIpInput)
        displayInput = findViewById(R.id.displayInput)
        connectButton = findViewById(R.id.connectButton)
        statusText = findViewById(R.id.statusText)
        logText = findViewById(R.id.logText)
        topPanel = findViewById(R.id.topPanel)
        streamSurface = findViewById(R.id.streamSurface)
        streamContainer = findViewById(R.id.streamContainer)
        hudText = findViewById(R.id.hudText)
        logScroll = findViewById(R.id.logScroll)

        signalingClient = SignalingClient(
            onEvent = { event -> runOnUiThread { appendLog(event) } },
            onMessage = { message -> runOnUiThread { appendLog("RX: $message") } }
        )

        streamSurface.holder.addCallback(object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) {
                surfaceReady = true
                maybeStartPendingStream("Surface lista, iniciando video H.264...")
            }
            override fun surfaceChanged(holder: SurfaceHolder, format: Int, w: Int, h: Int) {}
            override fun surfaceDestroyed(holder: SurfaceHolder) {
                surfaceReady = false
                // If still logically connected, keep pendingStreamStart so surfaceCreated
                // will restart the stream (e.g. on screen rotation).
                if (connected) pendingStreamStart = true
                streamSocket?.close(1000, "surface destroyed")
                streamSocket = null
                decoder?.release()
                decoder = null
            }
        })

        connectButton.setOnClickListener {
            if (!connected) {
                val ip = serverIpInput.text.toString().trim().ifBlank { "127.0.0.1" }
                val mode = if (ip == "127.0.0.1" || ip == "localhost") "USB" else "Wi-Fi"
                signalingClient.connect("ws://$ip:9001/ws")
                signalingClient.send(SignalMessage.Join("monitor", role = "android_client"))
                reconnectAttempts = 0
                connected = true
                statusText.text = "Estado: conectando ($mode)..."
                connectButton.text = "Disconnect"
                appendLog("$mode: conectando a $ip")
                startH264Stream()
            } else {
                stopAll()
            }
        }
    }

    private fun startH264Stream() {
        streamContainer.visibility = View.VISIBLE
        logScroll.visibility = View.GONE
        topPanel.visibility = View.GONE
        setImmersiveMode(true)

        if (!surfaceReady) {
            pendingStreamStart = true
            appendLog("Surface no lista todavia, esperando...")
            return
        }

        pendingStreamStart = false
        if (streamSocket != null) {
            return
        }

        val metrics = resources.displayMetrics
        val rawW = metrics.widthPixels.coerceAtLeast(1)
        val rawH = metrics.heightPixels.coerceAtLeast(1)

        // Scale to fit within 1920×1080 (H.264 Level 4.1 limit: ~2M pixels).
        // Native res (e.g. 2000×1142) exceeds Qualcomm AVC decoder limits causing
        // AMEDIA_ERROR_UNSUPPORTED. Keep aspect ratio, cap each dimension independently.
        val scale = minOf(1920.0 / rawW, 1080.0 / rawH, 1.0)
        val targetW = (rawW * scale).toInt().coerceAtLeast(320) and 0x7FFFFFFE
        val targetH = (rawH * scale).toInt().coerceAtLeast(240) and 0x7FFFFFFE

        val ip = serverIpInput.text.toString().trim().ifBlank { "127.0.0.1" }
        val displayIdx = displayInput.text.toString().toIntOrNull()?.coerceIn(0, 9) ?: 0
        val baseUrl = "ws://$ip:9001"

        decoder?.release()
        decoder = H264Decoder(streamSurface.holder.surface, targetW, targetH) { fps, latencyMs ->
            runOnUiThread { hudText.text = "%.0f fps  •  %dms".format(fps, latencyMs) }
        }

        val streamUrl = "$baseUrl/h264?w=$targetW&h=$targetH&fps=60&bitrate_kbps=20000&fit=cover&display=$displayIdx"
        val request = Request.Builder().url(streamUrl).build()

        streamSocket = okHttpClient.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                reconnectAttempts = 0
                runOnUiThread { statusText.text = "Estado: video H.264 activo" }
            }

            override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                decoder?.feed(bytes.toByteArray())
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                runOnUiThread { appendLog("WS: $text") }
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                runOnUiThread {
                    appendLog("Stream error: ${t.message}")
                    streamSocket = null
                    if (connected && !pendingStreamStart) scheduleReconnect()
                    else if (!connected) stopVisualStreamingState()
                }
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                runOnUiThread {
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
        streamContainer.visibility = View.GONE
        logScroll.visibility = View.VISIBLE
        topPanel.visibility = View.VISIBLE
        setImmersiveMode(false)
    }

    private fun stopAll() {
        reconnectHandler.removeCallbacksAndMessages(null)
        reconnectAttempts = 0
        streamSocket?.close(1000, "desconectado por usuario")
        streamSocket = null
        pendingStreamStart = false
        signalingClient.close()
        decoder?.release()
        decoder = null
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

    override fun onDestroy() {
        stopAll()
        okHttpClient.dispatcher.executorService.shutdown()
        super.onDestroy()
    }

    private fun scheduleReconnect() {
        if (!connected) return
        // Exponential backoff: 1s, 2s, 4s, 8s … capped at 30s
        val delayMs = minOf(1000L shl reconnectAttempts.coerceAtMost(4), 30_000L)
        reconnectAttempts++
        statusText.text = "Estado: reconectando (${reconnectAttempts}) en ${delayMs/1000}s..."
        appendLog("Reconectando en ${delayMs}ms (intento $reconnectAttempts)")
        reconnectHandler.postDelayed({ if (connected) startH264Stream() }, delayMs)
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
}

private class H264Decoder(
    private val surface: Surface,
    private val width: Int,
    private val height: Int,
    private val onHudUpdate: (fps: Float, latencyMs: Long) -> Unit
) {
    private val codec: MediaCodec = MediaCodec.createDecoderByType("video/avc")
    private val parser = AnnexBParser { nal -> queueNal(nal) }

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

    // NAL submission queue: decouples the OkHttp WebSocket callback thread from the
    // codec's dequeueInputBuffer. Without this, non-blocking dequeueInputBuffer(0) on
    // the WebSocket thread silently drops P-frames whenever the decoder's input buffer
    // pool is momentarily full, causing systematic 2-5fps drops.
    private val nalQueue = java.util.concurrent.ArrayBlockingQueue<ByteArray>(180) // 3s @60fps
    private val submitThread = Thread {
        android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_AUDIO)
        while (!Thread.currentThread().isInterrupted) {
            try {
                val nal = nalQueue.take() // blocks until NAL available — 0 wait vs poll(1ms)
                submitNal(nal)
            } catch (_: InterruptedException) { break }
        }
    }.apply { isDaemon = true; name = "h264-submit"; start() }

    // Codec is NOT started yet — we wait for SPS+PPS from the stream so we can
    // configure it with csd-0/csd-1. Many Qualcomm hardware decoders require
    // explicit parameter sets in the MediaFormat and ignore inline SPS/PPS.
    @Volatile private var codecStarted = false
    private var pendingSps: ByteArray? = null
    private var pendingPps: ByteArray? = null
    // NALs that arrive before the codec is started (between SPS and first IDR)
    private val pendingNals = ArrayDeque<ByteArray>()

    private var framesRendered = 0
    private var totalBytesReceived = 0L
    private var hudLastMs = System.currentTimeMillis()
    private var hudFrameCount = 0
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
        android.util.Log.v("H264Decoder", "NAL type=$nalType size=${nal.size}")

        if (!codecStarted) {
            // Accumulate SPS and PPS; queue everything else as pending.
            when (nalType) {
                7 -> { pendingSps = nal; android.util.Log.i("H264Decoder", "Got SPS (${nal.size}B)") }
                8 -> { pendingPps = nal; android.util.Log.i("H264Decoder", "Got PPS (${nal.size}B)") }
                else -> pendingNals.addLast(nal)
            }
            val sps = pendingSps ?: return
            val pps = pendingPps ?: return
            // Both parameter sets available — configure and start the codec.
            try {
                val format = MediaFormat.createVideoFormat("video/avc", width, height)
                format.setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 2 * 1024 * 1024)
                // KEY_PRIORITY 0 = real-time: allocate full HW decoder capacity.
                // KEY_OPERATING_RATE: hint that we need sustained 60 fps throughput.
                // Without these, Qualcomm decoders may apply power-saving throttling
                // that drops frames when content complexity rises (e.g. 60fps video).
                format.setInteger(MediaFormat.KEY_PRIORITY, 0)
                // setFloat is required: the framework calls findFloat() internally;
                // passing an integer is silently ignored on most Qualcomm drivers.
                format.setFloat(MediaFormat.KEY_OPERATING_RATE, 60f)
                // csd-0/csd-1: Android requires Annex-B start-code prefix.
                val sc = byteArrayOf(0, 0, 0, 1)
                format.setByteBuffer("csd-0", java.nio.ByteBuffer.wrap(sc + sps))
                format.setByteBuffer("csd-1", java.nio.ByteBuffer.wrap(sc + pps))
                codec.configure(format, surface, null, 0)
                codec.start()
                codecStarted = true
                android.util.Log.i("H264Decoder",
                    "Codec started with SPS(${sps.size}B)+PPS(${pps.size}B), " +
                    "flushing ${pendingNals.size} pending NAL(s)")
                for (pending in pendingNals) nalQueue.offer(pending)
                pendingNals.clear()
            } catch (e: Exception) {
                android.util.Log.e("H264Decoder", "Codec start failed: ${e.message}")
            }
            return
        }

        // Codec is running — skip redundant inline SPS/PPS (already in csd-0/csd-1).
        if (nalType == 7 || nalType == 8) return
        if (!nalQueue.offer(nal)) {
            android.util.Log.w("H264Decoder", "NAL queue full, dropping type=$nalType")
        }
    }

    private val startCode = byteArrayOf(0, 0, 0, 1)

    private fun submitNal(nal: ByteArray) {
        val nalType = nal[0].toInt() and 0x1F
        val withSC = startCode + nal
        // Block up to 4ms to get an input buffer slot — runs on the dedicated submit
        // thread, not the WebSocket thread, so blocking here is safe and prevents
        // the P-frame drops that caused systematic sub-60fps delivery.
        val index = codec.dequeueInputBuffer(4000)
        if (index < 0) {
            android.util.Log.w("H264Decoder", "NAL type=$nalType dropped — no input buffer in 4ms")
            return
        }
        val input = codec.getInputBuffer(index) ?: run {
            codec.queueInputBuffer(index, 0, 0, 0, 0); return
        }
        if (withSC.size > input.capacity()) {
            android.util.Log.w("H264Decoder",
                "NAL type=$nalType too large: ${withSC.size} > ${input.capacity()}")
            codec.queueInputBuffer(index, 0, 0, 0, 0)
            return
        }
        input.clear()
        input.put(withSC)
        codec.queueInputBuffer(index, 0, withSC.size, System.nanoTime() / 1000, 0)
    }

    private fun drainOutput() {
        val info = MediaCodec.BufferInfo()
        // Block up to 4ms waiting for the next decoded frame — avoids CPU spinning.
        // When the HW decoder has output ready it returns immediately; if nothing is
        // ready within 4ms we loop back and block again. Then drain any further 
        // already-decoded frames non-blocking so bursts are fully consumed in one pass.
        var timeout = 4000L // µs — first call blocks
        while (true) {
            when (val outIndex = codec.dequeueOutputBuffer(info, timeout)) {
                MediaCodec.INFO_TRY_AGAIN_LATER -> break
                MediaCodec.INFO_OUTPUT_FORMAT_CHANGED ->
                    android.util.Log.d("H264Decoder", "Output format: ${codec.outputFormat}")
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
                        val instantFps = hudFrameCount * 1000f / (nowMs - hudLastMs)
                        val alpha = 0.25f
                        emaFps = if (emaFps < 1f) instantFps else alpha * instantFps + (1f - alpha) * emaFps
                        emaLatencyMs = if (emaLatencyMs < 1f) latencyMs.toFloat()
                                       else alpha * latencyMs + (1f - alpha) * emaLatencyMs
                        onHudUpdate(emaFps, emaLatencyMs.toLong())
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
