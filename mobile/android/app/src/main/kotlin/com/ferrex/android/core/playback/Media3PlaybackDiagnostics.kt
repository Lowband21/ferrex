package com.ferrex.android.core.playback

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.Tracks
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.source.LoadEventInfo
import androidx.media3.exoplayer.source.MediaLoadData

private const val MEDIA3_PLAYBACK_TAG = "Media3Playback"

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class Media3PlaybackDiagnostics : AnalyticsListener {
    private var lastTracksSummary: String? = null

    override fun onLoadStarted(
        eventTime: AnalyticsListener.EventTime,
        loadEventInfo: LoadEventInfo,
        mediaLoadData: MediaLoadData,
    ) {
        PlaybackDiagnosticLog.debug(
            MEDIA3_PLAYBACK_TAG,
            "Load started type=${mediaLoadData.dataType.dataTypeName()} uri=${PlaybackDiagnosticLog.redact(loadEventInfo.uri.toString())}",
        )
    }

    override fun onLoadCompleted(
        eventTime: AnalyticsListener.EventTime,
        loadEventInfo: LoadEventInfo,
        mediaLoadData: MediaLoadData,
    ) {
        PlaybackDiagnosticLog.info(
            MEDIA3_PLAYBACK_TAG,
            "Load complete type=${mediaLoadData.dataType.dataTypeName()} bytes=${loadEventInfo.bytesLoaded} durationMs=${loadEventInfo.loadDurationMs}",
        )
    }

    override fun onLoadError(
        eventTime: AnalyticsListener.EventTime,
        loadEventInfo: LoadEventInfo,
        mediaLoadData: MediaLoadData,
        error: java.io.IOException,
        wasCanceled: Boolean,
    ) {
        PlaybackDiagnosticLog.error(
            MEDIA3_PLAYBACK_TAG,
            "Load error type=${mediaLoadData.dataType.dataTypeName()} canceled=$wasCanceled uri=${PlaybackDiagnosticLog.redact(loadEventInfo.uri.toString())}",
            error,
        )
    }

    override fun onPlaybackStateChanged(eventTime: AnalyticsListener.EventTime, state: Int) {
        val name = when (state) {
            Player.STATE_IDLE -> "IDLE"
            Player.STATE_BUFFERING -> "BUFFERING"
            Player.STATE_READY -> "READY"
            Player.STATE_ENDED -> "ENDED"
            else -> "UNKNOWN($state)"
        }
        PlaybackDiagnosticLog.info(MEDIA3_PLAYBACK_TAG, "State -> $name")
    }

    override fun onTracksChanged(eventTime: AnalyticsListener.EventTime, tracks: Tracks) {
        val summary = tracks.describeForDiagnostics()
        if (summary != lastTracksSummary) {
            lastTracksSummary = summary
            PlaybackDiagnosticLog.info(MEDIA3_PLAYBACK_TAG, summary)
        }
    }

    override fun onVideoInputFormatChanged(
        eventTime: AnalyticsListener.EventTime,
        format: Format,
        decoderReuseEvaluation: DecoderReuseEvaluation?,
    ) {
        PlaybackDiagnosticLog.info(
            MEDIA3_PLAYBACK_TAG,
            "Video format mime=${format.sampleMimeType ?: "unknown"} size=${format.width.valueOrUnknown()}x${format.height.valueOrUnknown()} bitrate=${format.bitrate.valueOrUnknown()}",
        )
    }

    override fun onAudioInputFormatChanged(
        eventTime: AnalyticsListener.EventTime,
        format: Format,
        decoderReuseEvaluation: DecoderReuseEvaluation?,
    ) {
        PlaybackDiagnosticLog.info(
            MEDIA3_PLAYBACK_TAG,
            "Audio format mime=${format.sampleMimeType ?: "unknown"} channels=${format.channelCount.valueOrUnknown()} sampleRate=${format.sampleRate.valueOrUnknown()}",
        )
    }

    override fun onPlayerError(eventTime: AnalyticsListener.EventTime, error: PlaybackException) {
        PlaybackDiagnosticLog.error(
            MEDIA3_PLAYBACK_TAG,
            "Player error code=${error.errorCode}: ${PlaybackDiagnosticLog.redact(error.message ?: "Playback error")}",
            error,
        )
    }

    private fun Tracks.describeForDiagnostics(): String {
        val mediaGroups = toPlaybackTrackGroupSnapshots().filter { group ->
            group.type == C.TRACK_TYPE_AUDIO || group.type == C.TRACK_TYPE_TEXT || group.type == C.TRACK_TYPE_VIDEO
        }
        return PlaybackTrackOptions.describeTracksForDiagnostics(mediaGroups)
    }

    private fun Int.dataTypeName(): String = when (this) {
        1 -> "MEDIA"
        2 -> "MANIFEST"
        3 -> "TIME_SYNC"
        4 -> "DRM"
        else -> "TYPE_$this"
    }

    private fun Int.valueOrUnknown(): String = if (this == Format.NO_VALUE) "?" else toString()
}
