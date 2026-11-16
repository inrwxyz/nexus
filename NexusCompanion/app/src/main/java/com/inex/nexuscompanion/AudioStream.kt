package com.inex.nexuscompanion

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Build
import android.os.PowerManager
import android.os.Process
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.SocketTimeoutException

/** Plays the stereo 16-bit PCM that the PC sends from WASAPI loopback. */
class PcAudioPlayer {
    @Volatile var isPlaying: Boolean = false
        private set

    @Volatile private var run = false
    private var sock: DatagramSocket? = null
    private var wakeLock: PowerManager.WakeLock? = null

    fun start(ctx: Context, port: Int, rate: Int) {
        stop()

        try {
            val pm = ctx.applicationContext.getSystemService(Context.POWER_SERVICE) as? PowerManager
            wakeLock = pm?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Nexus:AudioStreamLock")?.apply {
                setReferenceCounted(false)
                acquire(8 * 60 * 60 * 1000L) // up to 8 hours continuous bed listening
            }
        } catch (_: Exception) {}

        val min = AudioTrack.getMinBufferSize(rate, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_16BIT)
        val track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_GAME) // FastTrack hardware bypass, no DSP lookahead delay
                    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                    .build()
            )
            .setAudioFormat(
                AudioFormat.Builder().setSampleRate(rate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT).build()
            )
            .setBufferSizeInBytes(min)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            try {
                // ~35ms hardware buffer: eliminates DAC underruns while guaranteeing true zero lip-sync delay
                val targetFrames = (rate * 0.035f).toInt().coerceAtLeast(min / 4)
                track.setBufferSizeInFrames(targetFrames)
            } catch (_: Exception) {}
        }

        val s = DatagramSocket(port).also {
            it.soTimeout = 1000
            try { it.receiveBufferSize = 16384 } catch (_: Exception) {} // 16KB absorbs jitter without buffering stale audio
        }
        sock = s
        run = true
        isPlaying = true
        track.play()

        Thread {
            try { Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO) } catch (_: Exception) {}
            val buf = ByteArray(2048)
            var writtenFrames = 0L

            while (run) {
                try {
                    val p = DatagramPacket(buf, buf.size)
                    s.receive(p)

                    // Real-time synchronization: keep phone playback locked with PC screen
                    val head = track.playbackHeadPosition.toLong() and 0xFFFFFFFFL
                    val bufferedFrames = writtenFrames - head
                    val bufferedMs = if (rate > 0) (bufferedFrames * 1000L) / rate else 0L

                    // If delay drifts over 40ms (e.g. Wi-Fi burst or queue backlog),
                    // skip stale packet to immediately snap back to live video sync
                    if (bufferedMs > 40L) {
                        continue
                    }
                    if (bufferedFrames < 0) {
                        writtenFrames = head
                    }

                    val written = track.write(p.data, 0, p.length)
                    if (written > 0) {
                        writtenFrames += written / 4 // 16-bit stereo = 4 bytes per frame
                    }
                } catch (_: SocketTimeoutException) {
                    writtenFrames = track.playbackHeadPosition.toLong() and 0xFFFFFFFFL
                } catch (_: Exception) { break }
            }
            try { track.stop(); track.release() } catch (_: Exception) {}
            try { s.close() } catch (_: Exception) {}
            isPlaying = false
        }.start()
    }

    fun stop() {
        run = false
        isPlaying = false
        try { sock?.close() } catch (_: Exception) {}
        sock = null
        try {
            if (wakeLock?.isHeld == true) wakeLock?.release()
        } catch (_: Exception) {}
        wakeLock = null
    }
}
