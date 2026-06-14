package com.ferrex.android.core.watch

import com.ferrex.android.core.api.ApiEnvelope
import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.FerrexApiClient
import com.ferrex.android.core.api.ServerConfig
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerialName
import kotlinx.serialization.SerializationException
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

class OkHttpWatchStateTransport(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val json: Json = FerrexApiClient.DefaultJson,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : WatchStateTransport {
    override suspend fun fetchMediaProgress(mediaId: String): ApiResult<WatchMediaProgress?> = getNullable<MediaProgressApi>(
        Routes.mediaProgress(mediaId),
    ).mapSuccessNullable { it?.toDomain(mediaId) }

    override suspend fun fetchWatchState(): ApiResult<WatchStateSnapshot> = when (val result = get<WatchStateApi>(Routes.WATCH_STATE)) {
        is ApiResult.Success -> ApiResult.Success(result.data.toDomain())
        is ApiResult.HttpError -> result
        is ApiResult.ServerError -> result
        ApiResult.EmptyBody -> ApiResult.EmptyBody
        is ApiResult.ParseError -> result
        is ApiResult.NetworkError -> result
    }

    override suspend fun fetchSeriesWatchStatus(tmdbSeriesId: Long): ApiResult<WatchSeriesStatus> =
        when (val result = get<SeriesWatchStatusApi>(Routes.seriesState(tmdbSeriesId))) {
            is ApiResult.Success -> ApiResult.Success(result.data.toDomain())
            is ApiResult.HttpError -> result
            is ApiResult.ServerError -> result
            ApiResult.EmptyBody -> ApiResult.EmptyBody
            is ApiResult.ParseError -> result
            is ApiResult.NetworkError -> result
        }

    override suspend fun fetchSeriesNextEpisode(tmdbSeriesId: Long): ApiResult<WatchNextEpisode?> = getNullable<NextEpisodeApi>(
        Routes.seriesNext(tmdbSeriesId),
    ).mapSuccessNullable { it?.toDomain() }

    override suspend fun markMovieWatched(mediaId: String, watched: Boolean): ApiResult<Unit> = mutate(
        path = Routes.movieWatched(mediaId),
        watched = watched,
    )

    override suspend fun markEpisodeWatched(mediaId: String, watched: Boolean): ApiResult<Unit> = mutate(
        path = Routes.episodeWatched(mediaId),
        watched = watched,
    )

    override suspend fun markSeriesWatched(tmdbSeriesId: Long, watched: Boolean): ApiResult<Unit> = mutate(
        path = Routes.seriesWatched(tmdbSeriesId),
        watched = watched,
    )

    private suspend inline fun <reified T> get(path: String): ApiResult<T> = withContext(ioDispatcher) {
        executeJson<T>(requestBuilder(path).get().build(), allowNullData = false)
    }

    private suspend inline fun <reified T> getNullable(path: String): ApiResult<T?> = withContext(ioDispatcher) {
        executeJson<T?>(requestBuilder(path).get().build(), allowNullData = true)
    }

    private suspend fun mutate(path: String, watched: Boolean): ApiResult<Unit> = withContext(ioDispatcher) {
        val builder = requestBuilder(path)
        val request = if (watched) {
            builder.post(ByteArray(0).toRequestBody(JSON_MEDIA_TYPE)).build()
        } else {
            builder.delete().build()
        }
        try {
            httpClient.newCall(request).execute().use { response ->
                if (!response.isSuccessful) {
                    ApiResult.HttpError(response.code, response.message.ifBlank { "HTTP ${response.code}" })
                } else {
                    ApiResult.Success(Unit)
                }
            }
        } catch (e: IOException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Network unavailable")
        } catch (e: IllegalArgumentException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Server URL is not configured")
        }
    }

    private inline fun <reified T> executeJson(request: Request, allowNullData: Boolean): ApiResult<T> {
        try {
            httpClient.newCall(request).execute().use { response ->
                if (!response.isSuccessful) {
                    return ApiResult.HttpError(response.code, response.message.ifBlank { "HTTP ${response.code}" })
                }
                val body = response.body?.string() ?: return ApiResult.EmptyBody
                if (body.isBlank()) return ApiResult.EmptyBody
                val envelope = try {
                    json.decodeFromString<ApiEnvelope<T>>(body)
                } catch (e: SerializationException) {
                    return ApiResult.ParseError(e.message ?: "Invalid watch JSON")
                } catch (e: IllegalArgumentException) {
                    return ApiResult.ParseError(e.message ?: "Invalid watch JSON")
                }
                if (envelope.status != null && envelope.status != "success") {
                    return ApiResult.ServerError(envelope.error ?: envelope.message ?: "Watch request failed")
                }
                val data = envelope.data
                if (data == null && !allowNullData) {
                    return ApiResult.ParseError("Response did not include data")
                }
                @Suppress("UNCHECKED_CAST")
                return ApiResult.Success(data as T)
            }
        } catch (e: IOException) {
            return ApiResult.NetworkError(e.localizedMessage ?: "Network unavailable")
        } catch (e: IllegalArgumentException) {
            return ApiResult.NetworkError(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            return ApiResult.NetworkError(e.localizedMessage ?: "Server URL is not configured")
        }
    }

    private fun requestBuilder(path: String): Request.Builder = Request.Builder()
        .url("${serverConfig.requireUrl()}$path")
        .header("Accept", JSON_MIME)

    object Routes {
        const val WATCH_STATE = "/api/v1/watch/state"
        fun mediaProgress(mediaId: String): String = "/api/v1/media/$mediaId/progress"
        fun movieWatched(mediaId: String): String = "/api/v1/watch/movies/$mediaId/watched"
        fun episodeWatched(mediaId: String): String = "/api/v1/watch/episodes/$mediaId/watched"
        fun seriesWatched(tmdbSeriesId: Long): String = "/api/v1/watch/series/$tmdbSeriesId/watched"
        fun seriesState(tmdbSeriesId: Long): String = "/api/v1/watch/series/$tmdbSeriesId"
        fun seriesNext(tmdbSeriesId: Long): String = "/api/v1/watch/series/$tmdbSeriesId/next"
    }

    companion object {
        private const val JSON_MIME = "application/json"
        private val JSON_MEDIA_TYPE = JSON_MIME.toMediaType()
    }
}

private inline fun <T, R> ApiResult<T?>.mapSuccessNullable(transform: (T?) -> R): ApiResult<R> = when (this) {
    is ApiResult.Success -> ApiResult.Success(transform(data))
    is ApiResult.HttpError -> this
    is ApiResult.ServerError -> this
    ApiResult.EmptyBody -> ApiResult.EmptyBody
    is ApiResult.ParseError -> this
    is ApiResult.NetworkError -> this
}

@Serializable
private data class MediaProgressApi(
    @SerialName("media_id") val mediaId: String,
    val position: Double = 0.0,
    val duration: Double = 0.0,
    val percentage: Double = 0.0,
    @SerialName("is_completed") val isCompleted: Boolean = false,
) {
    fun toDomain(requestedMediaId: String): WatchMediaProgress = WatchMediaProgress(
        mediaId = mediaId.ifBlank { requestedMediaId },
        positionSeconds = position,
        durationSeconds = duration,
        percentage = percentage,
        isCompleted = isCompleted,
    )
}

@Serializable
private data class WatchStateApi(
    @SerialName("in_progress") val inProgress: List<InProgressApi> = emptyList(),
    val completed: Set<String> = emptySet(),
) {
    fun toDomain(): WatchStateSnapshot = WatchStateSnapshot(
        inProgress = inProgress.associate { item -> item.mediaId to item.toDomain() },
        completed = completed,
    )
}

@Serializable
private data class InProgressApi(
    @SerialName("media_id") val mediaId: String,
    val position: Double = 0.0,
    val duration: Double = 0.0,
    @SerialName("last_watched") val lastWatched: Long = 0L,
) {
    fun toDomain(): WatchMediaProgress = WatchMediaProgress(
        mediaId = mediaId,
        positionSeconds = position,
        durationSeconds = duration,
        percentage = if (duration > 0.0) (position / duration) * 100.0 else 0.0,
        isCompleted = false,
    )
}

@Serializable
private data class SeriesWatchStatusApi(
    @SerialName("tmdb_series_id") val tmdbSeriesId: Long,
    @SerialName("total_episodes") val totalEpisodes: Int = 0,
    val watched: Int = 0,
    @SerialName("in_progress") val inProgress: Int = 0,
    val seasons: Map<String, SeasonWatchStatusApi> = emptyMap(),
    @SerialName("next_episode") val nextEpisode: NextEpisodeApi? = null,
) {
    fun toDomain(): WatchSeriesStatus = WatchSeriesStatus(
        tmdbSeriesId = tmdbSeriesId,
        totalEpisodes = totalEpisodes,
        watched = watched,
        inProgress = inProgress,
        seasons = seasons.mapNotNull { (key, value) ->
            val seasonNumber = key.toIntOrNull() ?: value.key?.seasonNumber ?: return@mapNotNull null
            seasonNumber to value.toDomain(seasonNumber)
        }.toMap(),
        nextEpisode = nextEpisode?.toDomain(),
    )
}

@Serializable
private data class SeasonWatchStatusApi(
    val key: SeasonKeyApi? = null,
    val total: Int = 0,
    val watched: Int = 0,
    @SerialName("in_progress") val inProgress: Int = 0,
    @SerialName("is_completed") val isCompleted: Boolean = false,
    val episodes: Map<String, EpisodeStatusApi> = emptyMap(),
) {
    fun toDomain(seasonNumber: Int): WatchSeasonStatus = WatchSeasonStatus(
        seasonNumber = seasonNumber,
        total = total,
        watched = watched,
        inProgress = inProgress,
        isCompleted = isCompleted,
        episodes = episodes.mapNotNull { (key, value) ->
            val episodeNumber = key.toIntOrNull() ?: return@mapNotNull null
            episodeNumber to value.toDomain()
        }.toMap(),
    )
}

@Serializable
private data class SeasonKeyApi(
    @SerialName("tmdb_series_id") val tmdbSeriesId: Long = 0,
    @SerialName("season_number") val seasonNumber: Int = 0,
)

@Serializable
private data class EpisodeStatusApi(
    val state: String = "unwatched",
    val progress: Float = 0f,
) {
    fun toDomain(): WatchEpisodeStatus = when (state.lowercase()) {
        "completed" -> WatchEpisodeStatus(WatchEpisodeState.Completed, 1f)
        "in_progress" -> WatchEpisodeStatus(WatchEpisodeState.InProgress, progress.coerceIn(0f, 1f))
        else -> WatchEpisodeStatus(WatchEpisodeState.Unwatched, 0f)
    }
}

@Serializable
private data class NextEpisodeApi(
    val key: EpisodeKeyApi,
    @SerialName("playable_media_id") val playableMediaId: String? = null,
    val reason: String = "first_unwatched",
) {
    fun toDomain(): WatchNextEpisode = WatchNextEpisode(
        key = key.toDomain(),
        playableMediaId = playableMediaId,
        reason = reason,
    )
}

@Serializable
private data class EpisodeKeyApi(
    @SerialName("tmdb_series_id") val tmdbSeriesId: Long,
    @SerialName("season_number") val seasonNumber: Int,
    @SerialName("episode_number") val episodeNumber: Int,
) {
    fun toDomain(): WatchEpisodeKey = WatchEpisodeKey(
        tmdbSeriesId = tmdbSeriesId,
        seasonNumber = seasonNumber,
        episodeNumber = episodeNumber,
    )
}
