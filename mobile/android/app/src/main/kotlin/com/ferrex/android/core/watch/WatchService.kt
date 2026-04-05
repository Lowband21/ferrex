package com.ferrex.android.core.watch

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.diagnostics.DiagnosticLog
import com.ferrex.android.core.library.toUuidString
import ferrex.watch.ContinueWatchingEntry
import ferrex.watch.ContinueWatchingList
import ferrex.watch.SeriesWatchStatus
import ferrex.watch.WatchState
import ferrex.watch.WatchStateEntry
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
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
                    .addHeader("Accept", "application/x-flatbuffers")
                    .get()
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    return@withContext ApiResult.HttpError(response.code, response.message)
                }

                val bytes = response.body?.bytes()
                    ?: return@withContext ApiResult.HttpError(0, "Empty response")

                val list = ContinueWatchingList.getRootAsContinueWatchingList(
                    ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
                )

                val entries = (0 until list.itemsLength).mapNotNull { i ->
                    val entry = list.items(i) ?: return@mapNotNull null
                    ContinueWatchingItem(
                        mediaId = entry.mediaId.toUuidString(),
                        mediaType = entry.mediaType,
                        title = entry.title ?: "Unknown",
                        position = entry.position,
                        duration = entry.duration,
                        posterIid = entry.posterIid?.toUuidString(),
                        progress = if (entry.duration > 0) (entry.position / entry.duration).toFloat()
                            .coerceIn(0f, 1f) else 0f,
                    )
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
}

// ── Data classes ─────────────────────────────────────────────────────

data class ContinueWatchingData(
    val items: List<ContinueWatchingItem>,
)

data class ContinueWatchingItem(
    val mediaId: String,
    val mediaType: Byte,
    val title: String,
    val position: Double,
    val duration: Double,
    val posterIid: String?,
    val progress: Float,
)

data class WatchProgress(
    val mediaId: String,
    val position: Double,
    val duration: Double,
    val completed: Boolean,
    val progress: Float,
)
