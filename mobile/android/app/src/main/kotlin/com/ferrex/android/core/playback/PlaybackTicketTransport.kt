package com.ferrex.android.core.playback

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
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException

private const val PLAYBACK_TAG = "PlaybackTicket"
const val PLAYBACK_TICKET_QUERY_PARAMETER = "access_token"

data class PlaybackTicket(
    val token: String,
    val expiresInSeconds: Long,
)

interface PlaybackTicketTransport {
    suspend fun fetchTicket(mediaId: String): ApiResult<PlaybackTicket>
}

class OkHttpPlaybackTicketTransport(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val json: Json = FerrexApiClient.DefaultJson,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : PlaybackTicketTransport {
    override suspend fun fetchTicket(mediaId: String): ApiResult<PlaybackTicket> = withContext(ioDispatcher) {
        val request = try {
            Request.Builder()
                .url(ticketUrl(mediaId))
                .header("Accept", JSON_MIME)
                .get()
                .build()
        } catch (e: IllegalArgumentException) {
            return@withContext ApiResult.NetworkError(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            return@withContext ApiResult.NetworkError(e.localizedMessage ?: "Server URL is not configured")
        }

        try {
            httpClient.newCall(request).execute().use { response ->
                if (!response.isSuccessful) {
                    return@withContext ApiResult.HttpError(response.code, response.message.ifBlank { "HTTP ${response.code}" })
                }

                val body = response.body?.string() ?: return@withContext ApiResult.EmptyBody
                if (body.isBlank()) return@withContext ApiResult.EmptyBody

                val envelope = try {
                    json.decodeFromString<ApiEnvelope<PlaybackTicketApi>>(body)
                } catch (e: SerializationException) {
                    return@withContext ApiResult.ParseError(e.message ?: "Invalid playback ticket JSON")
                } catch (e: IllegalArgumentException) {
                    return@withContext ApiResult.ParseError(e.message ?: "Invalid playback ticket JSON")
                }

                if (envelope.status != null && envelope.status != "success") {
                    return@withContext ApiResult.ServerError(envelope.error ?: envelope.message ?: "Playback ticket request failed")
                }

                val data = envelope.data ?: return@withContext ApiResult.ParseError("Response did not include a playback ticket")
                val token = data.accessToken.trim()
                if (token.isBlank()) {
                    return@withContext ApiResult.ParseError("Playback ticket was empty")
                }
                PlaybackDiagnosticLog.info(
                    PLAYBACK_TAG,
                    "Fetched playback ticket for media=$mediaId expiresIn=${data.expiresInSeconds}s",
                )
                ApiResult.Success(PlaybackTicket(token = token, expiresInSeconds = data.expiresInSeconds))
            }
        } catch (e: IOException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Network unavailable")
        } catch (e: IllegalArgumentException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Server URL is not configured")
        }
    }

    private fun ticketUrl(mediaId: String): String = serverConfig.requireUrl()
        .toHttpUrl()
        .newBuilder()
        .addPathSegments("api/v1/stream")
        .addPathSegment(mediaId)
        .addPathSegment("ticket")
        .build()
        .toString()

    companion object {
        private const val JSON_MIME = "application/json"
    }
}

class PlaybackStreamUrlFactory(
    private val serverConfig: ServerConfig,
) {
    fun streamUrl(mediaId: String, ticket: PlaybackTicket): String = serverConfig.requireUrl()
        .toHttpUrl()
        .newBuilder()
        .addPathSegments("api/v1/stream")
        .addPathSegment(mediaId)
        .addQueryParameter(PLAYBACK_TICKET_QUERY_PARAMETER, ticket.token)
        .build()
        .toString()
}

@Serializable
private data class PlaybackTicketApi(
    @SerialName("access_token") val accessToken: String,
    @SerialName("expires_in") val expiresInSeconds: Long = 0,
)
