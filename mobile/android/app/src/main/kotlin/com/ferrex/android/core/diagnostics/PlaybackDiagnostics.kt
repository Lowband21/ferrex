package com.ferrex.android.core.diagnostics

import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.source.LoadEventInfo
import androidx.media3.exoplayer.source.MediaLoadData

/**
 * ExoPlayer [AnalyticsListener] that feeds detailed playback telemetry
 * into [DiagnosticLog].
 *
 * Attach to the player immediately after construction:
 * ```
 * val diag = PlaybackDiagnostics()
 * exoPlayer.addAnalyticsListener(diag)
 * ```
 *
 * When a crash occurs, the retained DiagnosticLog entries are written to
 * the crash file by [CrashHandler], giving a clear picture of what
 * ExoPlayer was doing in the seconds before the crash.
 */
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class PlaybackDiagnostics : AnalyticsListener {

    private val tag = "Playback"

    // ── Loading ─────────────────────────────────────────────────

    override fun onLoadStarted(
        eventTime: AnalyticsListener.EventTime,
        loadEventInfo: LoadEventInfo,
        mediaLoadData: MediaLoadData,
    ) {
        val uri = loadEventInfo.uri
        val type = mediaLoadData.dataType.dataTypeName()
        DiagnosticLog.d(tag, "Load started: type=$type uri=$uri")
    }

    override fun onLoadCompleted(
        eventTime: AnalyticsListener.EventTime,
        loadEventInfo: LoadEventInfo,
        mediaLoadData: MediaLoadData,
    ) {
        val bytes = loadEventInfo.bytesLoaded
        val durationMs = loadEventInfo.loadDurationMs
        val type = mediaLoadData.dataType.dataTypeName()
        val speed = if (durationMs > 0) "%.1f MB/s".format(bytes / 1024.0 / 1024.0 / (durationMs / 1000.0)) else "?"
        DiagnosticLog.i(tag, "Load complete: type=$type bytes=$bytes time=${durationMs}ms ($speed)")
    }

    override fun onLoadError(
        eventTime: AnalyticsListener.EventTime,
        loadEventInfo: LoadEventInfo,
        mediaLoadData: MediaLoadData,
        error: java.io.IOException,
        wasCanceled: Boolean,
    ) {
        val type = mediaLoadData.dataType.dataTypeName()
        val uri = loadEventInfo.uri
        DiagnosticLog.e(tag,
            "Load error: type=$type canceled=$wasCanceled uri=$uri",
            error,
        )
    }

    // ── Bandwidth ───────────────────────────────────────────────

    override fun onBandwidthEstimate(
        eventTime: AnalyticsListener.EventTime,
        totalLoadTimeMs: Int,
        totalBytesLoaded: Long,
        bitrateEstimate: Long,
    ) {
        val mbps = bitrateEstimate / 1_000_000.0
        DiagnosticLog.d(tag, "Bandwidth estimate: %.1f Mbps (loaded %.1f MB in %dms)".format(
            mbps,
            totalBytesLoaded / (1024.0 * 1024.0),
            totalLoadTimeMs,
        ))
    }

    // ── Playback state ──────────────────────────────────────────

    override fun onPlaybackStateChanged(
        eventTime: AnalyticsListener.EventTime,
        state: Int,
    ) {
        val name = when (state) {
            Player.STATE_IDLE -> "IDLE"
            Player.STATE_BUFFERING -> "BUFFERING"
            Player.STATE_READY -> "READY"
            Player.STATE_ENDED -> "ENDED"
            else -> "UNKNOWN($state)"
        }
        DiagnosticLog.i(tag, "State → $name")
    }

    override fun onIsPlayingChanged(
        eventTime: AnalyticsListener.EventTime,
        isPlaying: Boolean,
    ) {
        DiagnosticLog.d(tag, "isPlaying=$isPlaying")
    }

    override fun onIsLoadingChanged(
        eventTime: AnalyticsListener.EventTime,
        isLoading: Boolean,
    ) {
        DiagnosticLog.d(tag, "isLoading=$isLoading")
    }

    // ── Decoders ────────────────────────────────────────────────

    override fun onVideoDecoderInitialized(
        eventTime: AnalyticsListener.EventTime,
        decoderName: String,
        initializedTimestampMs: Long,
        initializationDurationMs: Long,
    ) {
        DiagnosticLog.i(tag,
            "Video decoder: $decoderName (init ${initializationDurationMs}ms)")
    }

    override fun onAudioDecoderInitialized(
        eventTime: AnalyticsListener.EventTime,
        decoderName: String,
        initializedTimestampMs: Long,
        initializationDurationMs: Long,
    ) {
        DiagnosticLog.i(tag,
            "Audio decoder: $decoderName (init ${initializationDurationMs}ms)")
    }

    override fun onVideoInputFormatChanged(
        eventTime: AnalyticsListener.EventTime,
        format: androidx.media3.common.Format,
        decoderReuseEvaluation: DecoderReuseEvaluation?,
    ) {
        DiagnosticLog.i(tag,
            "Video format: ${format.sampleMimeType} ${format.width}x${format.height} " +
                "bitrate=${format.bitrate} codecs=${format.codecs}")
    }

    override fun onAudioInputFormatChanged(
        eventTime: AnalyticsListener.EventTime,
        format: androidx.media3.common.Format,
        decoderReuseEvaluation: DecoderReuseEvaluation?,
    ) {
        DiagnosticLog.i(tag,
            "Audio format: ${format.sampleMimeType} ${format.channelCount}ch " +
                "${format.sampleRate}Hz bitrate=${format.bitrate}")
    }

    // ── Rendering quality ───────────────────────────────────────

    override fun onDroppedVideoFrames(
        eventTime: AnalyticsListener.EventTime,
        droppedFrames: Int,
        elapsedMs: Long,
    ) {
        DiagnosticLog.w(tag, "Dropped $droppedFrames video frames in ${elapsedMs}ms")
    }

    override fun onAudioUnderrun(
        eventTime: AnalyticsListener.EventTime,
        bufferSize: Int,
        bufferSizeMs: Long,
        elapsedSinceLastFeedMs: Long,
    ) {
        DiagnosticLog.w(tag,
            "Audio underrun: bufferSize=$bufferSize bufferMs=$bufferSizeMs " +
                "sinceLastFeed=${elapsedSinceLastFeedMs}ms")
    }

    // ── Errors ──────────────────────────────────────────────────

    override fun onPlayerError(
        eventTime: AnalyticsListener.EventTime,
        error: PlaybackException,
    ) {
        val rt = Runtime.getRuntime()
        val usedMb = (rt.totalMemory() - rt.freeMemory()) / (1024 * 1024)
        val maxMb = rt.maxMemory() / (1024 * 1024)

        DiagnosticLog.e(tag,
            "PLAYER ERROR [code=${error.errorCode}] heap=${usedMb}/${maxMb}MB: " +
                "${error.message}",
            error,
        )
    }

    // ── Helpers ──────────────────────────────────────────────────

    private fun Int.dataTypeName(): String = when (this) {
        1 -> "MEDIA"
        2 -> "MANIFEST"
        3 -> "TIME_SYNC"
        4 -> "DRM"
        else -> "TYPE_$this"
    }
}
