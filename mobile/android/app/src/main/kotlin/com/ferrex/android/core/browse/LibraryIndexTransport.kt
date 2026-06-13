package com.ferrex.android.core.browse

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
import java.nio.ByteBuffer
import java.nio.ByteOrder

sealed interface LibraryIndexResult<out T> {
    data class Success<T>(val value: T) : LibraryIndexResult<T>
    data class Unsupported(val message: String) : LibraryIndexResult<Nothing>
    data class Failure(val message: String) : LibraryIndexResult<Nothing>
}

@Serializable
data class MovieFilterIndicesRequest(
    @SerialName("media_type") val mediaType: String = "movie",
    @SerialName("rating_range") val ratingRange: ScalarRange? = null,
    val sort: String? = null,
    val order: String? = null,
)

@Serializable
data class ScalarRange(
    val min: Int,
    val max: Int,
)

interface LibraryIndexTransport {
    suspend fun fetchSortedMovieIndices(
        libraryId: String,
        sort: MovieSortMode,
    ): LibraryIndexResult<List<Int>>

    suspend fun fetchFilteredMovieIndices(
        libraryId: String,
        sort: MovieSortMode,
        filter: MovieFilterMode,
    ): LibraryIndexResult<List<Int>>
}

class OkHttpLibraryIndexTransport(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val json: Json = FerrexApiClient.DefaultJson,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : LibraryIndexTransport {
    override suspend fun fetchSortedMovieIndices(
        libraryId: String,
        sort: MovieSortMode,
    ): LibraryIndexResult<List<Int>> = withContext(ioDispatcher) {
        val all = mutableListOf<Int>()
        var offset = 0
        do {
            val path = "/api/v1/libraries/$libraryId/indices/sorted" +
                "?sort=${sort.endpointSort}&order=${sort.endpointOrder}&offset=$offset&limit=$SORT_PAGE_SIZE"
            val page = executeBinary(
                Request.Builder()
                    .url(url(path))
                    .get()
                    .header("Accept", RKYV_MIME)
                    .build(),
            )
            when (page) {
                is LibraryIndexResult.Failure -> return@withContext page
                is LibraryIndexResult.Unsupported -> return@withContext page
                is LibraryIndexResult.Success -> {
                    all += page.value
                    offset += page.value.size
                    if (page.value.size < SORT_PAGE_SIZE) return@withContext LibraryIndexResult.Success(all)
                }
            }
        } while (true)
        @Suppress("UNREACHABLE_CODE")
        LibraryIndexResult.Success(all)
    }

    override suspend fun fetchFilteredMovieIndices(
        libraryId: String,
        sort: MovieSortMode,
        filter: MovieFilterMode,
    ): LibraryIndexResult<List<Int>> {
        if (filter == MovieFilterMode.All) return fetchSortedMovieIndices(libraryId, sort)
        val request = MovieFilterIndicesRequest(
            ratingRange = when (filter) {
                MovieFilterMode.HighRated -> ScalarRange(min = 70, max = 100)
                MovieFilterMode.All -> null
            },
            sort = sort.endpointSort,
            order = when (sort.endpointOrder) {
                "desc" -> "descending"
                else -> "ascending"
            },
        )
        return executeBinary(
            Request.Builder()
                .url(url("/api/v1/libraries/$libraryId/indices/filter"))
                .post(json.encodeToString(request).toRequestBody(JSON_MEDIA_TYPE))
                .header("Accept", RKYV_MIME)
                .header("Content-Type", JSON_MEDIA_TYPE.toString())
                .build(),
        )
    }

    private suspend fun executeBinary(request: Request): LibraryIndexResult<List<Int>> = withContext(ioDispatcher) {
        try {
            httpClient.newCall(request).execute().use { response ->
                if (response.code == 501) {
                    return@withContext LibraryIndexResult.Unsupported("This library is not supported by the current movie index endpoint.")
                }
                if (!response.isSuccessful) {
                    return@withContext LibraryIndexResult.Failure(response.message.ifBlank { "HTTP ${response.code}" })
                }
                val bytes = response.body?.bytes() ?: ByteArray(0)
                if (bytes.isEmpty()) return@withContext LibraryIndexResult.Failure("Index endpoint returned an empty response")
                decodeArchivedIndices(bytes).fold(
                    onSuccess = { LibraryIndexResult.Success(it) },
                    onFailure = { LibraryIndexResult.Failure(it.message ?: "Could not decode index response") },
                )
            }
        } catch (e: IOException) {
            LibraryIndexResult.Failure(e.localizedMessage ?: "Network unavailable")
        } catch (e: IllegalArgumentException) {
            LibraryIndexResult.Failure(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            LibraryIndexResult.Failure(e.localizedMessage ?: "Server URL is not configured")
        }
    }

    private fun url(path: String): String = "${serverConfig.requireUrl()}$path"

    companion object {
        private const val SORT_PAGE_SIZE = 500
        private const val RKYV_MIME = "application/octet-stream"
        private val JSON_MEDIA_TYPE = "application/json; charset=utf-8".toMediaType()

        /**
         * Decode Ferrex's rkyv-archived `IndicesResponse { content_version: u32, indices: Vec<u32> }`.
         *
         * Server builds use little-endian, 32-bit relative pointers. For this specific archive shape,
         * the root struct occupies the final 12 bytes: content version, vec relative pointer, vec len.
         */
        fun decodeArchivedIndices(bytes: ByteArray): Result<List<Int>> = runCatching {
            require(bytes.size >= 12) { "Index archive is too short" }
            val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            val rootStart = bytes.size - 12
            val _contentVersion = buffer.getInt(rootStart)
            val relativePointer = buffer.getInt(rootStart + 4)
            val length = buffer.getInt(rootStart + 8)
            require(length >= 0) { "Index archive length is negative" }
            val dataStart = rootStart + 4 + relativePointer
            val dataBytes = length * Int.SIZE_BYTES
            require(dataStart >= 0 && dataStart + dataBytes <= bytes.size) { "Index archive vector is out of bounds" }
            List(length) { index -> buffer.getInt(dataStart + index * Int.SIZE_BYTES) }
        }
    }
}
