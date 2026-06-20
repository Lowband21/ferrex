package com.ferrex.android.core.library

import com.ferrex.android.core.api.ServerConfig
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

interface LibrarySyncTransport {
    suspend fun fetchLibraries(): LibrarySyncResult<ByteArray>
    suspend fun syncMovieBatches(libraryId: String, cachedVersions: Map<Int, Long>): LibrarySyncResult<MovieBatchSyncPlan>
    suspend fun fetchMovieBatch(libraryId: String, batchId: Int): LibrarySyncResult<ByteArray>
    suspend fun syncSeriesBundles(libraryId: String, cachedVersions: Map<String, Long>): LibrarySyncResult<SeriesBundleSyncPlan>
    suspend fun fetchSeriesBundles(libraryId: String, seriesIds: List<String>): LibrarySyncResult<ByteArray>
    suspend fun fetchSeriesBundle(libraryId: String, seriesId: String): LibrarySyncResult<ByteArray> =
        fetchSeriesBundles(libraryId, listOf(seriesId))
}

class OkHttpLibrarySyncTransport(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : LibrarySyncTransport {
    override suspend fun fetchLibraries(): LibrarySyncResult<ByteArray> = executeBinary(
        Request.Builder()
            .url(url(Routes.LIBRARIES))
            .get()
            .header("Accept", FLATBUFFERS_MIME)
            .build(),
    )

    override suspend fun syncMovieBatches(
        libraryId: String,
        cachedVersions: Map<Int, Long>,
    ): LibrarySyncResult<MovieBatchSyncPlan> {
        val request = binaryPost(
            path = Routes.movieBatchesSync(libraryId),
            body = LibraryFlatBuffers.buildBatchSyncRequest(cachedVersions),
        )
        return when (val result = executeBinary(request)) {
            is LibrarySyncResult.Success -> runCatching {
                LibraryFlatBuffers.parseBatchSyncResponse(result.value)
            }.fold(
                onSuccess = { LibrarySyncResult.Success(it) },
                onFailure = { LibrarySyncResult.Failure(LibrarySyncFailure.Parse(it.message ?: "Invalid movie sync response")) },
            )
            is LibrarySyncResult.Failure -> result
        }
    }

    override suspend fun fetchMovieBatch(libraryId: String, batchId: Int): LibrarySyncResult<ByteArray> = executeBinary(
        binaryPost(
            path = Routes.movieBatchesFetch(libraryId),
            body = LibraryFlatBuffers.buildBatchFetchRequest(listOf(batchId)),
        ),
    )

    override suspend fun syncSeriesBundles(
        libraryId: String,
        cachedVersions: Map<String, Long>,
    ): LibrarySyncResult<SeriesBundleSyncPlan> {
        val request = binaryPost(
            path = Routes.seriesBundlesSync(libraryId),
            body = LibraryFlatBuffers.buildSeriesBundleSyncRequest(cachedVersions),
        )
        return when (val result = executeBinary(request)) {
            is LibrarySyncResult.Success -> runCatching {
                LibraryFlatBuffers.parseSeriesBundleSyncResponse(result.value)
            }.fold(
                onSuccess = { LibrarySyncResult.Success(it) },
                onFailure = { LibrarySyncResult.Failure(LibrarySyncFailure.Parse(it.message ?: "Invalid series sync response")) },
            )
            is LibrarySyncResult.Failure -> result
        }
    }

    override suspend fun fetchSeriesBundles(libraryId: String, seriesIds: List<String>): LibrarySyncResult<ByteArray> = executeBinary(
        binaryPost(
            path = Routes.seriesBundlesFetch(libraryId),
            body = LibraryFlatBuffers.buildSeriesBundleFetchRequest(seriesIds),
        ),
    )

    private fun binaryPost(path: String, body: ByteArray): Request = Request.Builder()
        .url(url(path))
        .post(body.toRequestBody(FLATBUFFERS_MEDIA_TYPE))
        .header("Accept", FLATBUFFERS_MIME)
        .header("Content-Type", FLATBUFFERS_MIME)
        .build()

    private suspend fun executeBinary(request: Request): LibrarySyncResult<ByteArray> = withContext(ioDispatcher) {
        try {
            httpClient.newCall(request).execute().use { response ->
                if (!response.isSuccessful) {
                    return@withContext LibrarySyncResult.Failure(
                        LibrarySyncFailure.Http(
                            code = response.code,
                            message = response.message.ifBlank { "HTTP ${response.code}" },
                        ),
                    )
                }
                val bytes = response.body?.bytes() ?: return@withContext LibrarySyncResult.Failure(LibrarySyncFailure.EmptyBody)
                if (bytes.isEmpty()) return@withContext LibrarySyncResult.Failure(LibrarySyncFailure.EmptyBody)
                LibrarySyncResult.Success(bytes)
            }
        } catch (e: IOException) {
            LibrarySyncResult.Failure(LibrarySyncFailure.Network(e.localizedMessage ?: "Network unavailable"))
        } catch (e: IllegalArgumentException) {
            LibrarySyncResult.Failure(LibrarySyncFailure.Network(e.localizedMessage ?: "Invalid server URL"))
        } catch (e: IllegalStateException) {
            LibrarySyncResult.Failure(LibrarySyncFailure.Network(e.localizedMessage ?: "Server URL is not configured"))
        }
    }

    private fun url(path: String): String = "${serverConfig.requireUrl()}$path"

    object Routes {
        const val LIBRARIES = "/api/v1/libraries"
        fun movieBatchesSync(libraryId: String): String = "/api/v1/libraries/$libraryId/movie-batches:sync"
        fun movieBatchesFetch(libraryId: String): String = "/api/v1/libraries/$libraryId/movie-batches:fetch"
        fun seriesBundlesSync(libraryId: String): String = "/api/v1/libraries/$libraryId/series-bundles:sync"
        fun seriesBundlesFetch(libraryId: String): String = "/api/v1/libraries/$libraryId/series-bundles:fetch"
    }

    companion object {
        const val FLATBUFFERS_MIME = "application/x-flatbuffers"
        private val FLATBUFFERS_MEDIA_TYPE = FLATBUFFERS_MIME.toMediaType()
    }
}
