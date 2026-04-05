package com.ferrex.android.core.library

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.FerrexApiClient
import com.google.flatbuffers.FlatBufferBuilder
import ferrex.common.LibraryType
import ferrex.library.BatchFetchRequest
import ferrex.library.BatchFetchResponse
import ferrex.library.BatchSyncRequest
import ferrex.library.BatchSyncResponse
import ferrex.library.BatchVersion
import ferrex.library.Library
import ferrex.library.LibraryList
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
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
 * Repository managing library data via the batch sync protocol.
 *
 * The sync flow:
 * 1. GET /libraries → library metadata list
 * 2. POST /libraries/{id}/movie-batches:sync → compare cached batch versions
 * 3. POST /libraries/{id}/movie-batches:fetch → download stale batches
 * 4. Store raw FlatBuffer bytes to disk → memory-map on read
 *
 * Exposes [libraries] and [currentLibraryMedia] as StateFlows for
 * reactive UI binding.
 */
@Singleton
class LibraryRepository @Inject constructor(
    private val apiClient: FerrexApiClient,
    private val cache: LibraryCache,
    private val httpClient: OkHttpClient,
    private val serverConfig: com.ferrex.android.core.api.ServerConfig,
) {
    private val _libraries = MutableStateFlow<List<LibraryInfo>>(emptyList())
    val libraries: StateFlow<List<LibraryInfo>> = _libraries.asStateFlow()

    private val _currentMedia = MutableStateFlow<MediaAccessor?>(null)
    val currentMedia: StateFlow<MediaAccessor?> = _currentMedia.asStateFlow()

    private val _syncState = MutableStateFlow<SyncState>(SyncState.Idle)
    val syncState: StateFlow<SyncState> = _syncState.asStateFlow()

    // ── Library list ────────────────────────────────────────────────

    suspend fun loadLibraries(): ApiResult<List<LibraryInfo>> {
        return when (val result = apiClient.getLibraries()) {
            is ApiResult.Success -> {
                val list = result.data
                val infos = (0 until list.itemsLength).mapNotNull { i ->
                    val lib = list.items(i) ?: return@mapNotNull null
                    LibraryInfo(
                        id = lib.id.toUuidString(),
                        name = lib.name,
                        libraryType = lib.libraryType,
                    )
                }
                _libraries.value = infos
                ApiResult.Success(infos)
            }
            is ApiResult.HttpError -> result
            is ApiResult.NetworkError -> result
        }
    }

    // ── Batch sync + fetch ──────────────────────────────────────────

    /**
     * Full sync for a library: compare versions, fetch stale batches, update cache.
     */
    suspend fun syncAndFetch(libraryId: String, libraryType: Byte = LibraryType.Movies) {
        _syncState.value = SyncState.Syncing

        try {
            // Series libraries: the bundle endpoint returns series references
            // wrapped in the same BatchFetchResponse format.  Skip the
            // incremental sync protocol and fetch the full bundle directly.
            if (libraryType == LibraryType.Series) {
                val bundleBytes = fetchLibraryBundle(libraryId)
                if (bundleBytes != null) {
                    cache.writeMovieBatchVersioned(libraryId, 0, 1, bundleBytes)
                    loadFromCache(libraryId)
                    _syncState.value = SyncState.Ready
                } else {
                    _syncState.value = SyncState.Error("Failed to fetch series library")
                }
                return
            }

            // Step 1: Get cached batch versions
            val cachedVersions = cache.getCachedMovieBatchVersions(libraryId)

            // Step 2: Send sync request to server
            val syncResult = syncBatches(libraryId, cachedVersions)
            if (syncResult == null) {
                _syncState.value = SyncState.Error("Sync failed")
                return
            }

            val staleBatchIds = (0 until syncResult.staleBatchIdsLength)
                .map { syncResult.staleBatchIds(it).toInt() }

            if (staleBatchIds.isEmpty()) {
                // Everything cached is fresh — load from disk
                loadFromCache(libraryId)
                _syncState.value = SyncState.Ready
                return
            }

            // Step 3: Fetch stale batches
            val fetchResult = fetchBatches(libraryId, staleBatchIds)
            if (fetchResult == null) {
                _syncState.value = SyncState.Error("Fetch failed")
                return
            }

            // Step 4: Store to cache
            val serverVersions = (0 until syncResult.serverVersionsLength)
                .associate { i ->
                    val bv = syncResult.serverVersions(i)!!
                    bv.batchId.toInt() to bv.version.toLong()
                }

            // The fetchResult is the raw bytes — we store the entire response
            // and also update individual batch versions
            for (batchId in staleBatchIds) {
                val version = serverVersions[batchId] ?: 0L
                // Store the raw response bytes per batch
                cache.writeMovieBatchVersioned(libraryId, batchId, version, fetchResult)
            }

            // Step 5: Load everything into the accessor
            loadFromCache(libraryId)
            _syncState.value = SyncState.Ready
        } catch (e: Exception) {
            _syncState.value = SyncState.Error(e.localizedMessage ?: "Unknown error")
        }
    }

    /**
     * Look up a movie by UUID from the currently cached media.
     * Returns null if not found or no media is cached.
     */
    fun findMovieByUuid(uuidString: String): ferrex.media.MovieReference? {
        return _currentMedia.value?.findMovieByUuid(uuidString)
    }

    /**
     * Look up a series by UUID from the currently cached media.
     * Returns null if not found or no media is cached.
     */
    fun findSeriesByUuid(uuidString: String): ferrex.media.SeriesReference? {
        return _currentMedia.value?.findSeriesByUuid(uuidString)
    }

    private suspend fun loadFromCache(libraryId: String) {
        val versions = cache.getCachedMovieBatchVersions(libraryId)
        if (versions.isEmpty()) {
            _currentMedia.value = null
            return
        }

        // For now, load the first cached batch (will be expanded to merge multiple)
        val firstBatchId = versions.keys.first()
        val buffer = cache.getMovieBatchVersioned(libraryId, firstBatchId)
        if (buffer != null) {
            _currentMedia.value = MediaAccessor(buffer)
        }
    }

    // ── Bundle fetch (series libraries) ───────────────────────────

    /**
     * Fetch the full library media bundle as raw FlatBuffer bytes.
     *
     * For series libraries the server wraps series references into a
     * [BatchFetchResponse] so the same [MediaAccessor] can read them.
     */
    private suspend fun fetchLibraryBundle(libraryId: String): ByteArray? =
        withContext(Dispatchers.IO) {
            try {
                val request = Request.Builder()
                    .url("${serverConfig.serverUrl}${FerrexApiClient.Companion.Routes.movieBatchesBundle(libraryId)}")
                    .addHeader("Accept", "application/x-flatbuffers")
                    .get()
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) return@withContext null
                response.body?.bytes()
            } catch (_: Exception) {
                null
            }
        }

    // ── HTTP helpers for batch protocol ─────────────────────────────

    private suspend fun syncBatches(
        libraryId: String,
        cachedVersions: Map<Int, Long>,
    ): BatchSyncResponse? = withContext(Dispatchers.IO) {
        try {
            val builder = FlatBufferBuilder(256)

            // Build cached_versions vector
            val versionOffsets = cachedVersions.map { (batchId, version) ->
                BatchVersion.createBatchVersion(builder, batchId.toUInt(), version.toULong())
            }

            val versionsVector = BatchSyncRequest.createCachedVersionsVector(
                builder, versionOffsets.toIntArray()
            )
            BatchSyncRequest.startBatchSyncRequest(builder)
            BatchSyncRequest.addCachedVersions(builder, versionsVector)
            val root = BatchSyncRequest.endBatchSyncRequest(builder)
            builder.finish(root)

            val request = Request.Builder()
                .url("${serverConfig.serverUrl}${FerrexApiClient.Companion.Routes.movieBatchesSync(libraryId)}")
                .addHeader("Accept", "application/x-flatbuffers")
                .post(builder.sizedByteArray().toRequestBody("application/x-flatbuffers".toMediaType()))
                .build()

            val response = httpClient.newCall(request).execute()
            if (!response.isSuccessful) return@withContext null

            val bytes = response.body?.bytes() ?: return@withContext null
            BatchSyncResponse.getRootAsBatchSyncResponse(
                ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            )
        } catch (_: Exception) {
            null
        }
    }

    private suspend fun fetchBatches(
        libraryId: String,
        batchIds: List<Int>,
    ): ByteArray? = withContext(Dispatchers.IO) {
        try {
            val builder = FlatBufferBuilder(256)
            val idsVector = BatchFetchRequest.createBatchIdsVector(
                builder, batchIds.map { it.toUInt() }.toUIntArray()
            )
            BatchFetchRequest.startBatchFetchRequest(builder)
            BatchFetchRequest.addBatchIds(builder, idsVector)
            val root = BatchFetchRequest.endBatchFetchRequest(builder)
            builder.finish(root)

            val request = Request.Builder()
                .url("${serverConfig.serverUrl}${FerrexApiClient.Companion.Routes.movieBatchesFetch(libraryId)}")
                .addHeader("Accept", "application/x-flatbuffers")
                .post(builder.sizedByteArray().toRequestBody("application/x-flatbuffers".toMediaType()))
                .build()

            val response = httpClient.newCall(request).execute()
            if (!response.isSuccessful) return@withContext null

            response.body?.bytes()
        } catch (_: Exception) {
            null
        }
    }
}

data class LibraryInfo(
    val id: String,
    val name: String,
    val libraryType: Byte, // ferrex.common.LibraryType enum value
)

sealed interface SyncState {
    data object Idle : SyncState
    data object Syncing : SyncState
    data object Ready : SyncState
    data class Error(val message: String) : SyncState
}
