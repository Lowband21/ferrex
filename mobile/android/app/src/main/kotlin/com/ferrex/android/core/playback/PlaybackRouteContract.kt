package com.ferrex.android.core.playback

import com.ferrex.android.core.browse.BrowseMediaType
import kotlin.math.roundToLong

/**
 * Shared route contract used by detail surfaces to launch ticketed playback.
 *
 * [targetMediaId] is the playable media-file id when available; [logicalMediaId]
 * remains the movie/episode id used for watch-state refresh and progress UI.
 */
data class PlaybackRouteContract(
    val targetMediaId: String,
    val logicalMediaId: String,
    val mediaType: BrowseMediaType,
    val startPositionSeconds: Double?,
    val startOver: Boolean,
    val sourceDetailRoute: String,
) {
    val initialStartPositionMs: Long
        get() = if (startOver) {
            0L
        } else {
            ((startPositionSeconds ?: 0.0) * 1000.0).roundToLong().coerceAtLeast(0L)
        }

    fun toDisplayString(): String = buildString {
        append(mediaType.routeValue)
        append(" target=")
        append(targetMediaId)
        append(" logical=")
        append(logicalMediaId)
        if (startOver) {
            append(" start-over")
        } else {
            append(" position=")
            append(startPositionSeconds ?: 0.0)
            append('s')
        }
        append(" source=")
        append(sourceDetailRoute)
    }
}
