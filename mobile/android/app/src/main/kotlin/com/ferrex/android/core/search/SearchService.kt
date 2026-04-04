package com.ferrex.android.core.search

import com.ferrex.android.core.api.ServerConfig
import ferrex.library.BatchFetchResponse
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.nio.ByteBuffer
import java.nio.ByteOrder
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Search service wrapping POST /api/v1/media/query.
 *
 * The debounce and cancellation logic lives in the ViewModel via
 * Flow operators (debounce + flatMapLatest). This service handles
 * only the network call and FlatBuffer parsing.
 */
@Singleton
class SearchService @Inject constructor(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
) {
    /**
     * Execute a search query. Returns raw FlatBuffer bytes for the results,
     * or null on failure.
     *
     * Note: The exact request format depends on the server's /media/query
     * endpoint contract. For v1, we send a simple JSON query string since
     * the search request body is small and infrequent.
     */
    suspend fun search(query: String): SearchResult = withContext(Dispatchers.IO) {
        try {
            // For search, use JSON for the request body (small, infrequent)
            // and FlatBuffers for the response (potentially large result set)
            val jsonBody = """{"query":"${query.replace("\"", "\\\"")}"}"""

            val request = Request.Builder()
                .url("${serverConfig.serverUrl}/api/v1/media/query")
                .addHeader("Accept", "application/x-flatbuffers")
                .post(jsonBody.toByteArray().toRequestBody("application/json".toMediaType()))
                .build()

            val response = httpClient.newCall(request).execute()
            if (!response.isSuccessful) {
                return@withContext SearchResult.Error("Search failed: ${response.code}")
            }

            val bytes = response.body?.bytes()
                ?: return@withContext SearchResult.Error("Empty response")

            val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            SearchResult.Success(buffer, bytes)
        } catch (e: Exception) {
            SearchResult.Error(e.localizedMessage ?: "Search failed")
        }
    }
}

sealed interface SearchResult {
    data class Success(val buffer: ByteBuffer, val rawBytes: ByteArray) : SearchResult
    data class Error(val message: String) : SearchResult
}
