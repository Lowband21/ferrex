package com.ferrex.android.core.playback

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.FerrexApiClient
import com.ferrex.android.core.api.ServerConfig
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

interface PlaybackProgressReporter {
    suspend fun reportProgress(
        route: PlaybackRouteContract,
        positionSeconds: Double,
        durationSeconds: Double,
    ): ApiResult<Unit>
}

class OkHttpPlaybackProgressReporter(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val json: Json = FerrexApiClient.DefaultJson,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : PlaybackProgressReporter {
    override suspend fun reportProgress(
        route: PlaybackRouteContract,
        positionSeconds: Double,
        durationSeconds: Double,
    ): ApiResult<Unit> = withContext(ioDispatcher) {
        if (durationSeconds <= 0.0) return@withContext ApiResult.Success(Unit)

        val request = try {
            val body = PlaybackProgressBody(
                mediaId = route.targetMediaId,
                mediaType = route.mediaType.routeValue,
                position = positionSeconds.coerceAtLeast(0.0),
                duration = durationSeconds.coerceAtLeast(0.0),
                lastMediaUuid = route.targetMediaId,
            )
            Request.Builder()
                .url("${serverConfig.requireUrl()}$PROGRESS_PATH")
                .header("Accept", JSON_MIME)
                .post(json.encodeToString(body).toRequestBody(JSON_MEDIA_TYPE))
                .build()
        } catch (e: IllegalArgumentException) {
            return@withContext ApiResult.NetworkError(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            return@withContext ApiResult.NetworkError(e.localizedMessage ?: "Server URL is not configured")
        }

        try {
            httpClient.newCall(request).execute().use { response ->
                if (response.isSuccessful) {
                    ApiResult.Success(Unit)
                } else {
                    ApiResult.HttpError(response.code, response.message.ifBlank { "HTTP ${response.code}" })
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

    companion object {
        private const val PROGRESS_PATH = "/api/v1/watch/progress"
        private const val JSON_MIME = "application/json"
        private val JSON_MEDIA_TYPE = JSON_MIME.toMediaType()
    }
}

@Serializable
private data class PlaybackProgressBody(
    @SerialName("media_id") val mediaId: String,
    @SerialName("media_type") val mediaType: String,
    val position: Double,
    val duration: Double,
    @SerialName("last_media_uuid") val lastMediaUuid: String,
)
