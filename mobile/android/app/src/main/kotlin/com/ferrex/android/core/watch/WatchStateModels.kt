package com.ferrex.android.core.watch

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.messageOrFallback
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

sealed interface WatchMutationKind {
    val label: String

    data object Movie : WatchMutationKind {
        override val label: String = "movie"
    }

    data object Episode : WatchMutationKind {
        override val label: String = "episode"
    }

    data object Series : WatchMutationKind {
        override val label: String = "series"
    }
}

data class WatchMediaProgress(
    val mediaId: String,
    val positionSeconds: Double = 0.0,
    val durationSeconds: Double = 0.0,
    val percentage: Double = 0.0,
    val isCompleted: Boolean = false,
    val pendingMutation: Boolean = false,
) {
    val progressRatio: Float = when {
        isCompleted -> 1f
        durationSeconds > 0.0 -> (positionSeconds / durationSeconds).coerceIn(0.0, 1.0).toFloat()
        percentage > 0.0 -> (percentage / 100.0).coerceIn(0.0, 1.0).toFloat()
        else -> 0f
    }

    val isStarted: Boolean = progressRatio > 0f && !isCompleted

    companion object {
        fun unwatched(mediaId: String, pendingMutation: Boolean = false): WatchMediaProgress = WatchMediaProgress(
            mediaId = mediaId,
            pendingMutation = pendingMutation,
        )

        fun completed(mediaId: String, pendingMutation: Boolean = false): WatchMediaProgress = WatchMediaProgress(
            mediaId = mediaId,
            percentage = 100.0,
            isCompleted = true,
            pendingMutation = pendingMutation,
        )
    }
}

data class WatchStateSnapshot(
    val inProgress: Map<String, WatchMediaProgress> = emptyMap(),
    val completed: Set<String> = emptySet(),
) {
    fun progressFor(mediaId: String): WatchMediaProgress = when {
        mediaId in completed -> WatchMediaProgress.completed(mediaId)
        else -> inProgress[mediaId] ?: WatchMediaProgress.unwatched(mediaId)
    }
}

data class WatchEpisodeKey(
    val tmdbSeriesId: Long,
    val seasonNumber: Int,
    val episodeNumber: Int,
)

enum class WatchEpisodeState {
    Unwatched,
    InProgress,
    Completed,
}

data class WatchEpisodeStatus(
    val state: WatchEpisodeState,
    val progress: Float = 0f,
) {
    val isCompleted: Boolean get() = state == WatchEpisodeState.Completed
}

data class WatchSeasonStatus(
    val seasonNumber: Int,
    val total: Int,
    val watched: Int,
    val inProgress: Int,
    val isCompleted: Boolean,
    val episodes: Map<Int, WatchEpisodeStatus>,
)

data class WatchNextEpisode(
    val key: WatchEpisodeKey,
    val playableMediaId: String?,
    val reason: String,
)

data class WatchSeriesStatus(
    val tmdbSeriesId: Long,
    val totalEpisodes: Int,
    val watched: Int,
    val inProgress: Int,
    val seasons: Map<Int, WatchSeasonStatus>,
    val nextEpisode: WatchNextEpisode?,
    val pendingMutation: Boolean = false,
) {
    val progressRatio: Float = if (totalEpisodes > 0) (watched.toFloat() / totalEpisodes.toFloat()).coerceIn(0f, 1f) else 0f
    val isCompleted: Boolean = totalEpisodes > 0 && watched >= totalEpisodes

    fun episodeStatus(seasonNumber: Int, episodeNumber: Int): WatchEpisodeStatus =
        seasons[seasonNumber]?.episodes?.get(episodeNumber) ?: WatchEpisodeStatus(WatchEpisodeState.Unwatched)

    companion object {
        fun optimistic(tmdbSeriesId: Long, watched: Boolean, previous: WatchSeriesStatus?): WatchSeriesStatus {
            val total = previous?.totalEpisodes ?: 0
            return WatchSeriesStatus(
                tmdbSeriesId = tmdbSeriesId,
                totalEpisodes = total,
                watched = if (watched) total else 0,
                inProgress = 0,
                seasons = previous?.seasons.orEmpty().mapValues { (_, season) ->
                    season.copy(
                        watched = if (watched) season.total else 0,
                        inProgress = 0,
                        isCompleted = watched && season.total > 0,
                        episodes = season.episodes.mapValues {
                            WatchEpisodeStatus(if (watched) WatchEpisodeState.Completed else WatchEpisodeState.Unwatched)
                        },
                    )
                },
                nextEpisode = previous?.nextEpisode,
                pendingMutation = true,
            )
        }
    }
}

data class WatchRepositoryState(
    val media: Map<String, WatchMediaProgress> = emptyMap(),
    val series: Map<Long, WatchSeriesStatus> = emptyMap(),
    val nextEpisodes: Map<Long, WatchNextEpisode?> = emptyMap(),
    val lastError: String? = null,
) {
    fun mediaProgress(mediaId: String?): WatchMediaProgress? = mediaId?.let { media[it] }
    fun seriesStatus(tmdbSeriesId: Long?): WatchSeriesStatus? = tmdbSeriesId?.let { series[it] }
}

data class WatchStateInvalidation(
    val reason: String,
)

class WatchStateInvalidationBus {
    private val _events = MutableSharedFlow<WatchStateInvalidation>(
        replay = 0,
        extraBufferCapacity = 8,
    )
    val events: SharedFlow<WatchStateInvalidation> = _events.asSharedFlow()

    fun notifyWatchStateChanged(reason: String) {
        _events.tryEmit(WatchStateInvalidation(reason))
    }
}

interface WatchStateTransport {
    suspend fun fetchMediaProgress(mediaId: String): ApiResult<WatchMediaProgress?>
    suspend fun fetchWatchState(): ApiResult<WatchStateSnapshot>
    suspend fun fetchSeriesWatchStatus(tmdbSeriesId: Long): ApiResult<WatchSeriesStatus>
    suspend fun fetchSeriesNextEpisode(tmdbSeriesId: Long): ApiResult<WatchNextEpisode?>
    suspend fun markMovieWatched(mediaId: String, watched: Boolean): ApiResult<Unit>
    suspend fun markEpisodeWatched(mediaId: String, watched: Boolean): ApiResult<Unit>
    suspend fun markSeriesWatched(tmdbSeriesId: Long, watched: Boolean): ApiResult<Unit>
}

class WatchRepository(
    private val transport: WatchStateTransport,
    private val invalidationBus: WatchStateInvalidationBus,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) {
    private val _state = MutableStateFlow(WatchRepositoryState())
    val state: StateFlow<WatchRepositoryState> = _state.asStateFlow()

    suspend fun refreshMediaProgress(mediaId: String): WatchRepositoryState = withContext(ioDispatcher) {
        when (val result = transport.fetchMediaProgress(mediaId)) {
            is ApiResult.Success -> {
                val progress = result.data ?: WatchMediaProgress.unwatched(mediaId)
                publish(_state.value.copy(media = _state.value.media + (mediaId to progress), lastError = null))
            }
            else -> publish(_state.value.copy(lastError = result.messageOrFallback("Unable to refresh watch progress")))
        }
    }

    suspend fun refreshWatchState(): WatchRepositoryState = withContext(ioDispatcher) {
        when (val result = transport.fetchWatchState()) {
            is ApiResult.Success -> {
                val merged = buildMap {
                    putAll(result.data.inProgress)
                    result.data.completed.forEach { put(it, WatchMediaProgress.completed(it)) }
                }
                publish(_state.value.copy(media = _state.value.media + merged, lastError = null))
            }
            else -> publish(_state.value.copy(lastError = result.messageOrFallback("Unable to refresh watch state")))
        }
    }

    suspend fun refreshSeries(tmdbSeriesId: Long): WatchRepositoryState = withContext(ioDispatcher) {
        val status = transport.fetchSeriesWatchStatus(tmdbSeriesId)
        val next = transport.fetchSeriesNextEpisode(tmdbSeriesId)
        val current = _state.value
        val updated = when (status) {
            is ApiResult.Success -> current.copy(series = current.series + (tmdbSeriesId to status.data), lastError = null)
            else -> current.copy(lastError = status.messageOrFallback("Unable to refresh series watch state"))
        }
        val nextUpdated = when (next) {
            is ApiResult.Success -> updated.copy(nextEpisodes = updated.nextEpisodes + (tmdbSeriesId to next.data), lastError = updated.lastError)
            else -> updated.copy(lastError = next.messageOrFallback(updated.lastError ?: "Unable to refresh next episode"))
        }
        publish(nextUpdated)
    }

    suspend fun markMovieWatched(mediaId: String, watched: Boolean): ApiResult<Unit> = mutateMedia(
        mediaId = mediaId,
        watched = watched,
        kind = WatchMutationKind.Movie,
    ) { transport.markMovieWatched(mediaId, watched) }

    suspend fun markEpisodeWatched(mediaId: String, watched: Boolean): ApiResult<Unit> = mutateMedia(
        mediaId = mediaId,
        watched = watched,
        kind = WatchMutationKind.Episode,
    ) { transport.markEpisodeWatched(mediaId, watched) }

    suspend fun markSeriesWatched(tmdbSeriesId: Long, watched: Boolean): ApiResult<Unit> = withContext(ioDispatcher) {
        val previous = _state.value
        val previousSeries = previous.series[tmdbSeriesId]
        publish(
            previous.copy(
                series = previous.series + (tmdbSeriesId to WatchSeriesStatus.optimistic(tmdbSeriesId, watched, previousSeries)),
                lastError = null,
            ),
        )
        when (val result = transport.markSeriesWatched(tmdbSeriesId, watched)) {
            is ApiResult.Success -> {
                val committed = _state.value.series[tmdbSeriesId]?.copy(pendingMutation = false)
                if (committed != null) publish(_state.value.copy(series = _state.value.series + (tmdbSeriesId to committed)))
                invalidationBus.notifyWatchStateChanged("series ${if (watched) "watched" else "unwatched"}:$tmdbSeriesId")
                result
            }
            else -> {
                publish(previous.copy(lastError = result.messageOrFallback("Unable to update series watch state")))
                result
            }
        }
    }

    private suspend fun mutateMedia(
        mediaId: String,
        watched: Boolean,
        kind: WatchMutationKind,
        call: suspend () -> ApiResult<Unit>,
    ): ApiResult<Unit> = withContext(ioDispatcher) {
        val previous = _state.value
        val optimistic = if (watched) {
            WatchMediaProgress.completed(mediaId, pendingMutation = true)
        } else {
            WatchMediaProgress.unwatched(mediaId, pendingMutation = true)
        }
        publish(previous.copy(media = previous.media + (mediaId to optimistic), lastError = null))
        when (val result = call()) {
            is ApiResult.Success -> {
                publish(
                    _state.value.copy(
                        media = _state.value.media + (mediaId to optimistic.copy(pendingMutation = false)),
                        lastError = null,
                    ),
                )
                invalidationBus.notifyWatchStateChanged("${kind.label} ${if (watched) "watched" else "unwatched"}:$mediaId")
                result
            }
            else -> {
                publish(previous.copy(lastError = result.messageOrFallback("Unable to update ${kind.label} watch state")))
                result
            }
        }
    }

    private fun publish(state: WatchRepositoryState): WatchRepositoryState {
        _state.value = state
        return state
    }
}
