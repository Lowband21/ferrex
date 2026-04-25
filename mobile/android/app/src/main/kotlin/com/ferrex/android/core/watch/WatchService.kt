package com.ferrex.android.core.watch

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.diagnostics.DiagnosticLog
import com.ferrex.android.core.library.toUuidString
import ferrex.common.VideoMediaType
import ferrex.watch.SeriesWatchStatus
import ferrex.watch.WatchState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.ByteOrder
import javax.inject.Inject
import javax.inject.Singleton

private const val TAG = "WatchService"

/**
 * Service for fetching watch state from the server.
 *
 * Endpoints:
 * - GET /watch/continue → continue watching list (ordered by recency)
 * - GET /watch/state → full watch state for all items
 * - GET /watch/series/{tmdbSeriesId} → per-season/episode completion
 * - GET /watch/series/{tmdbSeriesId}/next → next episode to watch
 */
@Singleton
class WatchService @Inject constructor(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
) {
    /**
     * Fetch the continue watching list.
     * Returns items ordered by most recently watched.
     */
    suspend fun getContinueWatching(): ApiResult<ContinueWatchingData> =
        withContext(Dispatchers.IO) {
            try {
                val request = Request.Builder()
                    .url("${serverConfig.serverUrl}/api/v1/watch/continue")
                    .addHeader("Accept", "application/json")
                    .get()
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    return@withContext ApiResult.HttpError(response.code, response.message)
                }

                val body = response.body?.string()
                    ?: return@withContext ApiResult.HttpError(0, "Empty response")

                val root = JSONObject(body)
                val items = root.optJSONArray("data")
                val entries = buildList {
                    if (items != null) {
                        for (i in 0 until items.length()) {
                            val entry = items.optJSONObject(i) ?: continue
                            val mediaId = entry.optString("media_id")
                            if (mediaId.isNullOrBlank()) continue

                            val mediaType = parseVideoMediaType(entry.optString("media_type")) ?: continue
                            val title = entry.optNullableString("title") ?: continue
                            val duration = entry.optDouble("duration", 0.0)
                            val position = entry.optDouble("position", 0.0)
                            add(
                                ContinueWatchingItem(
                                    mediaId = mediaId,
                                    cardMediaId = entry.optNullableString("card_media_id"),
                                    mediaType = mediaType,
                                    title = title,
                                    subtitle = entry.optNullableString("subtitle"),
                                    actionHint = parseContinueWatchingActionHint(entry.optString("action_hint")),
                                    position = position,
                                    duration = duration,
                                    posterIid = entry.optNullableString("poster_iid"),
                                    progress = if (duration > 0) (position / duration).toFloat().coerceIn(0f, 1f) else 0f,
                                )
                            )
                        }
                    }
                }

                ApiResult.Success(ContinueWatchingData(entries))
            } catch (e: Exception) {
                DiagnosticLog.w(TAG, "Failed to fetch continue watching", e)
                ApiResult.NetworkError(e)
            }
        }

    /**
     * Fetch the full watch state for the current user.
     * Returns all in-progress and completed items with positions.
     */
    suspend fun getWatchState(): ApiResult<Map<String, WatchProgress>> =
        withContext(Dispatchers.IO) {
            try {
                val request = Request.Builder()
                    .url("${serverConfig.serverUrl}/api/v1/watch/state")
                    .addHeader("Accept", "application/x-flatbuffers")
                    .get()
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    return@withContext ApiResult.HttpError(response.code, response.message)
                }

                val bytes = response.body?.bytes()
                    ?: return@withContext ApiResult.HttpError(0, "Empty response")

                val state = WatchState.getRootAsWatchState(
                    ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
                )

                val map = mutableMapOf<String, WatchProgress>()
                for (i in 0 until state.itemsLength) {
                    val entry = state.items(i) ?: continue
                    val id = entry.mediaId.toUuidString()
                    map[id] = WatchProgress(
                        mediaId = id,
                        position = entry.position,
                        duration = entry.duration,
                        completed = entry.completed,
                        progress = if (entry.duration > 0) (entry.position / entry.duration).toFloat()
                            .coerceIn(0f, 1f) else 0f,
                    )
                }

                ApiResult.Success(map)
            } catch (e: Exception) {
                DiagnosticLog.w(TAG, "Failed to fetch watch state", e)
                ApiResult.NetworkError(e)
            }
        }

    /**
     * Fetch point-in-time progress for a single media item.
     *
     * The server resolves file ids back to logical movie/episode ids, so the
     * caller can safely pass the current playback/stream id.
     */
    suspend fun getMediaProgress(mediaId: String): ApiResult<MediaProgress?> =
        withContext(Dispatchers.IO) {
            try {
                val request = Request.Builder()
                    .url("${serverConfig.serverUrl}/api/v1/media/$mediaId/progress")
                    .addHeader("Accept", "application/json")
                    .get()
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    return@withContext ApiResult.HttpError(response.code, response.message)
                }

                val body = response.body?.string()
                    ?: return@withContext ApiResult.HttpError(0, "Empty response")

                val root = JSONObject(body)
                val data = if (root.isNull("data")) null else root.getJSONObject("data")
                val progress = data?.let {
                    MediaProgress(
                        mediaId = it.optString("media_id"),
                        position = it.optDouble("position", 0.0),
                        duration = it.optDouble("duration", 0.0),
                        percentage = it.optDouble("percentage", 0.0),
                        isCompleted = it.optBoolean("is_completed", false),
                    )
                }

                ApiResult.Success(progress)
            } catch (e: Exception) {
                DiagnosticLog.w(TAG, "Failed to fetch media progress for $mediaId", e)
                ApiResult.NetworkError(e)
            }
        }

    /**
     * Fetch series watch status with per-season/episode completion.
     */
    suspend fun getSeriesWatchStatus(tmdbSeriesId: Long): ApiResult<SeriesWatchStatus> =
        withContext(Dispatchers.IO) {
            try {
                val request = Request.Builder()
                    .url("${serverConfig.serverUrl}/api/v1/watch/series/$tmdbSeriesId")
                    .addHeader("Accept", "application/x-flatbuffers")
                    .get()
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    return@withContext ApiResult.HttpError(response.code, response.message)
                }

                val bytes = response.body?.bytes()
                    ?: return@withContext ApiResult.HttpError(0, "Empty response")

                val status = SeriesWatchStatus.getRootAsSeriesWatchStatus(
                    ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
                )

                ApiResult.Success(status)
            } catch (e: Exception) {
                DiagnosticLog.w(TAG, "Failed to fetch series watch status", e)
                ApiResult.NetworkError(e)
            }
        }

    suspend fun markMovieWatched(movieId: String): ApiResult<Unit> =
        sendWatchMutation(
            path = "/api/v1/watch/movies/$movieId/watched",
            method = WatchMutationMethod.Post,
        )

    suspend fun markMovieUnwatched(movieId: String): ApiResult<Unit> =
        sendWatchMutation(
            path = "/api/v1/watch/movies/$movieId/watched",
            method = WatchMutationMethod.Delete,
        )

    suspend fun markSeriesWatched(tmdbSeriesId: Long): ApiResult<Unit> =
        sendWatchMutation(
            path = "/api/v1/watch/series/$tmdbSeriesId/watched",
            method = WatchMutationMethod.Post,
        )

    suspend fun markSeriesUnwatched(tmdbSeriesId: Long): ApiResult<Unit> =
        sendWatchMutation(
            path = "/api/v1/watch/series/$tmdbSeriesId/watched",
            method = WatchMutationMethod.Delete,
        )

    suspend fun markEpisodeWatched(episodeId: String): ApiResult<Unit> =
        sendWatchMutation(
            path = "/api/v1/watch/episodes/$episodeId/watched",
            method = WatchMutationMethod.Post,
        )

    suspend fun markEpisodeUnwatched(episodeId: String): ApiResult<Unit> =
        sendWatchMutation(
            path = "/api/v1/watch/episodes/$episodeId/watched",
            method = WatchMutationMethod.Delete,
        )

    private suspend fun sendWatchMutation(
        path: String,
        method: WatchMutationMethod,
    ): ApiResult<Unit> = withContext(Dispatchers.IO) {
        try {
            val builder = Request.Builder()
                .url("${serverConfig.serverUrl}$path")
                .addHeader("Accept", "application/json")

            when (method) {
                WatchMutationMethod.Post -> builder.post(ByteArray(0).toRequestBody(null))
                WatchMutationMethod.Delete -> builder.delete()
            }

            val response = httpClient.newCall(builder.build()).execute()
            response.use {
                if (!it.isSuccessful) {
                    return@withContext ApiResult.HttpError(it.code, it.message)
                }
            }

            ApiResult.Success(Unit)
        } catch (e: Exception) {
            DiagnosticLog.w(TAG, "Failed watch mutation for $path", e)
            ApiResult.NetworkError(e)
        }
    }
}

// ── Data classes ─────────────────────────────────────────────────────

data class ContinueWatchingData(
    val items: List<ContinueWatchingItem>,
)

data class MediaProgress(
    val mediaId: String,
    val position: Double,
    val duration: Double,
    val percentage: Double,
    val isCompleted: Boolean,
)

data class ContinueWatchingItem(
    val mediaId: String,
    val cardMediaId: String?,
    val mediaType: Byte,
    val title: String,
    val subtitle: String?,
    val actionHint: ContinueWatchingActionHint?,
    val position: Double,
    val duration: Double,
    val posterIid: String?,
    val progress: Float,
)

enum class ContinueWatchingActionHint {
    Resume,
    NextEpisode,
}

private enum class WatchMutationMethod {
    Post,
    Delete,
}

data class WatchProgress(
    val mediaId: String,
    val position: Double,
    val duration: Double,
    val completed: Boolean,
    val progress: Float,
)

private fun parseVideoMediaType(raw: String?): Byte? = when (raw) {
    "Movie" -> VideoMediaType.Movie
    "Series" -> VideoMediaType.Series
    "Season" -> VideoMediaType.Season
    "Episode" -> VideoMediaType.Episode
    else -> null
}

private fun parseContinueWatchingActionHint(raw: String?): ContinueWatchingActionHint? = when (raw) {
    "resume" -> ContinueWatchingActionHint.Resume
    "next_episode" -> ContinueWatchingActionHint.NextEpisode
    else -> null
}

private fun JSONObject.optNullableString(key: String): String? {
    if (isNull(key)) return null
    return optString(key).takeIf { it.isNotBlank() }
}
