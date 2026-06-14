package com.ferrex.android.core.watch

import com.ferrex.android.core.api.ApiEnvelope
import com.ferrex.android.core.api.FerrexApiClient
import com.ferrex.android.core.api.ServerConfig
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException

class OkHttpContinueWatchingTransport(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val json: Json = FerrexApiClient.DefaultJson,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : ContinueWatchingTransport {
    override suspend fun fetchContinueWatching(): ContinueWatchingResult<List<ContinueWatchingApiItem>> = withContext(ioDispatcher) {
        val request = Request.Builder()
            .url("${serverConfig.requireUrl()}${Routes.CONTINUE_WATCHING}")
            .get()
            .header("Accept", JSON_MIME)
            .build()
        try {
            httpClient.newCall(request).execute().use { response ->
                if (!response.isSuccessful) {
                    return@withContext ContinueWatchingResult.Failure(response.message.ifBlank { "HTTP ${response.code}" })
                }
                val body = response.body?.string().orEmpty()
                if (body.isBlank()) return@withContext ContinueWatchingResult.Failure("Continue Watching returned an empty response")
                val envelope = try {
                    json.decodeFromString<ApiEnvelope<List<ContinueWatchingApiItem>>>(body)
                } catch (e: SerializationException) {
                    return@withContext ContinueWatchingResult.Failure(e.message ?: "Invalid Continue Watching JSON")
                } catch (e: IllegalArgumentException) {
                    return@withContext ContinueWatchingResult.Failure(e.message ?: "Invalid Continue Watching JSON")
                }
                if (envelope.status != null && envelope.status != "success") {
                    return@withContext ContinueWatchingResult.Failure(envelope.error ?: envelope.message ?: "Continue Watching failed")
                }
                ContinueWatchingResult.Success(envelope.data.orEmpty())
            }
        } catch (e: IOException) {
            ContinueWatchingResult.Failure(e.localizedMessage ?: "Network unavailable")
        } catch (e: IllegalArgumentException) {
            ContinueWatchingResult.Failure(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            ContinueWatchingResult.Failure(e.localizedMessage ?: "Server URL is not configured")
        }
    }

    object Routes {
        const val CONTINUE_WATCHING = "/api/v1/watch/continue"
    }

    companion object {
        private const val JSON_MIME = "application/json"
    }
}
