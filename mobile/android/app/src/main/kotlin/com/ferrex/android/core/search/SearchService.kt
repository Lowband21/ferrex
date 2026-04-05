package com.ferrex.android.core.search

import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.toUuidString
import ferrex.watch.WatchState
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
 * The server's media/query endpoint accepts a MediaQuery JSON body and
 * returns WatchState in FlatBuffers mode — a list of (media_id UUID +
 * watch progress) entries. The full media details are resolved from the
 * locally cached batch data.
 *
 * Debounce and cancellation are handled by the ViewModel layer via
 * Flow operators (debounce + flatMapLatest).
 */
@Singleton
class SearchService @Inject constructor(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val repository: LibraryRepository,
) {
    /**
     * Execute a search query. Returns a list of [SearchHit] resolved
     * from the local cache, or an error.
     */
    suspend fun search(query: String): SearchResult = withContext(Dispatchers.IO) {
        try {
            // Build a MediaQuery JSON matching the server's expected format
            val jsonBody = """
                {
                    "search": {
                        "text": ${escapeJson(query)},
                        "fields": ["All"],
                        "fuzzy": true
                    },
                    "filters": {},
                    "sort": {"primary": "Title", "order": "Ascending"},
                    "pagination": {"offset": 0, "limit": 50}
                }
            """.trimIndent()

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

            // Parse as WatchState — contains media_id UUIDs
            val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            val watchState = WatchState.getRootAsWatchState(buffer)

            // Resolve each UUID against the local batch cache
            val hits = buildList {
                for (i in 0 until watchState.itemsLength) {
                    val entry = watchState.items(i) ?: continue
                    val mediaId = entry.mediaId ?: continue
                    val uuidString = mediaId.toUuidString()

                    // Try to find in cached movies first, then series
                    val movie = repository.findMovieByUuid(uuidString)
                    if (movie != null) {
                        add(SearchHit.Movie(uuidString, movie))
                        continue
                    }
                    val series = repository.findSeriesByUuid(uuidString)
                    if (series != null) {
                        add(SearchHit.Series(uuidString, series))
                    }
                    // If not found in cache, skip (could be an episode/season)
                }
            }

            SearchResult.Success(hits)
        } catch (e: Exception) {
            SearchResult.Error(e.localizedMessage ?: "Search failed")
        }
    }

    private fun escapeJson(s: String): String {
        val escaped = s.replace("\\", "\\\\").replace("\"", "\\\"")
        return "\"$escaped\""
    }
}

sealed interface SearchResult {
    data class Success(val hits: List<SearchHit>) : SearchResult
    data class Error(val message: String) : SearchResult
}

sealed interface SearchHit {
    val mediaId: String

    data class Movie(
        override val mediaId: String,
        val movie: ferrex.media.MovieReference,
    ) : SearchHit

    data class Series(
        override val mediaId: String,
        val series: ferrex.media.SeriesReference,
    ) : SearchHit
}
