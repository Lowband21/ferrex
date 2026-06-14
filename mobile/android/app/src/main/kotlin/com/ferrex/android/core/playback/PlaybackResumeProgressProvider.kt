package com.ferrex.android.core.playback

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.watch.WatchMediaProgress
import com.ferrex.android.core.watch.WatchStateTransport
import kotlin.math.min
import kotlin.math.roundToLong

interface PlaybackResumeProgressProvider {
    suspend fun fetchResumeProgress(logicalMediaId: String): ApiResult<WatchMediaProgress?>
}

class WatchStatePlaybackResumeProgressProvider(
    private val transport: WatchStateTransport,
) : PlaybackResumeProgressProvider {
    override suspend fun fetchResumeProgress(logicalMediaId: String): ApiResult<WatchMediaProgress?> =
        transport.fetchMediaProgress(logicalMediaId)
}

object PlaybackStartPositionResolver {
    fun requiresServerResume(route: PlaybackRouteContract): Boolean =
        !route.startOver && route.startPositionSeconds == null

    fun fromServerProgress(progress: WatchMediaProgress?): Long {
        if (progress == null || progress.isCompleted || progress.positionSeconds <= 0.0) return 0L
        val boundedPositionSeconds = if (progress.durationSeconds > 0.0) {
            min(progress.positionSeconds, progress.durationSeconds)
        } else {
            progress.positionSeconds
        }
        return (boundedPositionSeconds * 1000.0).roundToLong().coerceAtLeast(0L)
    }
}
