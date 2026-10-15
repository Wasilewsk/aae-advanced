package io.github.aaron_gh.aae.remote

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Process

/**
 * Plays the attached device's sound as it arrives, through a jitter buffer.
 *
 * Sound is buffered before it starts, and after a gap, to cover Wi-Fi
 * jitter; a break in the middle of sound makes the buffer longer for a
 * while. The emulator sends audio up to 1% slower than real time, so
 * playback runs slightly slower or faster to keep the buffer level, rather
 * than run dry every few seconds. Wi-Fi power saving is off while it plays.
 */
class SoundOut(context: Context?) {
    private val track: AudioTrack
    private val thread: Thread
    @Volatile private var stopped = false

    /** Incoming stereo samples, oldest first, from [start] for [count]. */
    private val buffer = ShortArray(RATE * 2)
    private var start = 0
    private var count = 0
    private val lock = Object()

    private val wifi: WifiManager.WifiLock? =
        (context?.applicationContext?.getSystemService(Context.WIFI_SERVICE) as? WifiManager)?.let {
            @Suppress("DEPRECATION")
            val mode = if (Build.VERSION.SDK_INT >= 29) WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            else WifiManager.WIFI_MODE_FULL_HIGH_PERF
            it.createWifiLock(mode, "AAE Remote audio").apply {
                setReferenceCounted(false)
                acquire()
            }
        }

    init {
        val format = AudioFormat.Builder()
            .setSampleRate(RATE)
            .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .build()
        val minimum = AudioTrack.getMinBufferSize(RATE, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_16BIT)
        track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build()
            )
            .setAudioFormat(format)
            // The jitter buffer is ahead of it; this only covers this thread's scheduling.
            .setBufferSizeInBytes(maxOf(minimum, frames(TRACK_MS) * 4))
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            .build()
        track.play()
        thread = Thread(::play, "AAE Remote audio").apply { start() }
    }

    /** 16-bit stereo samples at 48 kHz, little-endian. */
    fun write(bytes: ByteArray, offset: Int, length: Int) {
        val samples = (length - length % 4) / 2
        synchronized(lock) {
            if (count + samples > buffer.size) {
                // Far behind: drop the oldest.
                val drop = count + samples - buffer.size
                start = (start + drop) % buffer.size
                count -= drop
            }
            var end = (start + count) % buffer.size
            for (i in 0 until samples) {
                val at = offset + i * 2
                buffer[end] = ((bytes[at].toInt() and 0xff) or (bytes[at + 1].toInt() shl 8)).toShort()
                end = if (end + 1 == buffer.size) 0 else end + 1
            }
            count += samples
        }
    }

    fun release() {
        stopped = true
        thread.join(500)
        track.pause()
        track.flush()
        track.release()
        wifi?.release()
    }

    private fun play() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        val out = ShortArray(CHUNK * 2)
        var playing = false
        // Extra buffering after a break mid-sound, in frames.
        var extra = 0
        var steady = 0
        var level = 0.0
        var position = 1.0
        val from = FloatArray(2)
        val to = FloatArray(2)
        val follow = CHUNK / (RATE * DRIFT_SMOOTHING)
        while (!stopped) {
            synchronized(lock) {
                val buffered = count / 2
                val target = frames(TARGET_MS) + extra
                if (buffered > target + frames(MAX_EXTRA_MS)) {
                    // Fell behind, as after a stall: catch up.
                    val drop = (buffered - target) * 2
                    start = (start + drop) % buffer.size
                    count -= drop
                    level = target.toDouble()
                }
                if (!playing && count / 2 >= frames(PREFILL_MS) + extra) {
                    playing = true
                    level = (count / 2).toDouble()
                    position = 1.0
                    from.fill(0f)
                    to.fill(0f)
                }
                level += follow * (count / 2 - level)
                val step = 1.0 + ((level - target) / target * MAX_DRIFT).coerceIn(-MAX_DRIFT, MAX_DRIFT)
                for (i in 0 until CHUNK) {
                    while (playing && position >= 1.0) {
                        if (count < 2) {
                            // Ran dry. Buffer more from now on, and refill first.
                            playing = false
                            extra = minOf(extra + frames(EXTRA_STEP_MS), frames(MAX_EXTRA_MS))
                            steady = 0
                            break
                        }
                        from[0] = to[0]
                        from[1] = to[1]
                        to[0] = buffer[start].toFloat()
                        to[1] = buffer[start + 1].toFloat()
                        start = (start + 2) % buffer.size
                        count -= 2
                        position -= 1.0
                    }
                    if (playing) {
                        val t = position.toFloat()
                        out[i * 2] = (from[0] + (to[0] - from[0]) * t).toInt().toShort()
                        out[i * 2 + 1] = (from[1] + (to[1] - from[1]) * t).toInt().toShort()
                        position += step
                    } else {
                        out[i * 2] = 0
                        out[i * 2 + 1] = 0
                    }
                }
                if (playing && extra > 0 && ++steady >= frames(SETTLE_MS) / CHUNK) {
                    extra = maxOf(0, extra - frames(EXTRA_STEP_MS) / 2)
                    steady = 0
                }
            }
            // Blocks until the track has room, which paces this loop.
            if (track.write(out, 0, out.size, AudioTrack.WRITE_BLOCKING) < 0) break
        }
    }

    companion object {
        const val RATE = 48_000
        /** Frames written to the track at a time: 5 ms. */
        private const val CHUNK = 240
        private const val TRACK_MS = 20
        /** Buffered before sound starts, for Wi-Fi jitter. */
        private const val PREFILL_MS = 50
        /** The level playback steers towards. */
        private const val TARGET_MS = 60
        /** How much a break mid-sound adds to both, up to [MAX_EXTRA_MS]. */
        private const val EXTRA_STEP_MS = 20
        private const val MAX_EXTRA_MS = 140
        /** How long without a break before the extra buffering shrinks. */
        private const val SETTLE_MS = 10_000
        private const val MAX_DRIFT = 0.02
        private const val DRIFT_SMOOTHING = 0.25

        private fun frames(ms: Int) = RATE * ms / 1000
    }
}
