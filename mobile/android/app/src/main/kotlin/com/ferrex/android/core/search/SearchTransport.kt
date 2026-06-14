package com.ferrex.android.core.search

import com.ferrex.android.core.api.ApiEnvelope
import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerializationException
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

interface MediaSearchTransport {
    suspend fun queryMedia(searchText: String, limit: Int = SearchLimits.DEFAULT): ApiResult<List<SearchMediaWithStatus>>
}

class OkHttpMediaSearchTransport(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
    private val json: Json = SearchJson,
) : MediaSearchTransport {
    override suspend fun queryMedia(searchText: String, limit: Int): ApiResult<List<SearchMediaWithStatus>> = withContext(ioDispatcher) {
        val body = SearchMediaQuery(
            search = SearchQuery(text = searchText.trim()),
            pagination = SearchPagination(limit = SearchLimits.normalize(limit)),
        )
        val request = try {
            Request.Builder()
                .url("${serverConfig.requireUrl()}${Routes.MEDIA_QUERY}")
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
                if (!response.isSuccessful) {
                    return@withContext ApiResult.HttpError(
                        response.code,
                        response.message.ifBlank { "HTTP ${response.code}" },
                    )
                }

                val responseBody = response.body?.string() ?: return@withContext ApiResult.EmptyBody
                if (responseBody.isBlank()) return@withContext ApiResult.EmptyBody

                val envelope = try {
                    json.decodeFromString<ApiEnvelope<List<SearchMediaWithStatus>>>(responseBody)
                } catch (e: SerializationException) {
                    return@withContext ApiResult.ParseError(e.message ?: "Invalid JSON")
                } catch (e: IllegalArgumentException) {
                    return@withContext ApiResult.ParseError(e.message ?: "Invalid JSON")
                }

                if (envelope.status != null && envelope.status != "success") {
                    return@withContext ApiResult.ServerError(
                        envelope.error ?: envelope.message ?: "Server reported an error",
                    )
                }
                val data = envelope.data ?: return@withContext ApiResult.ParseError("Response did not include data")
                ApiResult.Success(data)
            }
        } catch (e: IOException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Network unavailable")
        } catch (e: IllegalArgumentException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Server URL is not configured")
        }
    }

    object Routes {
        const val MEDIA_QUERY = "/api/v1/media/query"
    }

    companion object {
        const val JSON_MIME = "application/json"
        private val JSON_MEDIA_TYPE = "$JSON_MIME; charset=utf-8".toMediaType()

        val SearchJson: Json = Json {
            ignoreUnknownKeys = true
            explicitNulls = true
            encodeDefaults = true
        }
    }
}
