package com.ferrex.android.core.library

import com.google.flatbuffers.FlatBufferBuilder
import ferrex.common.LibraryType
import ferrex.library.BatchFetchResponse
import ferrex.library.Library
import ferrex.library.LibraryList
import ferrex.library.MediaBatchData
import ferrex.library.SeriesBundleData
import ferrex.library.SeriesBundleFetchRequest
import ferrex.library.SeriesBundleFetchResponse
import ferrex.library.SeriesBundleSyncRequest
import ferrex.media.Media
import ferrex.media.MediaVariant
import ferrex.media.MovieReference
import ferrex.media.SeriesReference
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.util.UUID

@OptIn(ExperimentalUnsignedTypes::class)
class LibraryRepositoryTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun movieSyncSendsCachedVersionsFetchesStaleAndPrunesDeletedBatches() = runTest {
        val fixture = Fixture()
        val library = movieLibrary()
        fixture.cache.writeMovieBatch(fixture.scope, library.id, 1, 10L, movieBatchResponse(batchId = 1, version = 10L, movieCount = 1))
        fixture.cache.writeMovieBatch(fixture.scope, library.id, 9, 1L, movieBatchResponse(batchId = 9, version = 1L, movieCount = 1))
        fixture.transport.movieSync = { cached ->
            fixture.capturedMovieVersions = cached
            LibrarySyncResult.Success(
                MovieBatchSyncPlan(
                    staleBatchIds = listOf(2),
                    deletedBatchIds = listOf(9),
                    serverVersions = mapOf(2 to 20L),
                ),
            )
        }
        fixture.transport.movieFetches[2] = LibrarySyncResult.Success(movieBatchResponse(batchId = 2, version = 20L, movieCount = 2))

        val state = fixture.repository.syncMovieLibrary(fixture.scope, library)

        assertEquals(mapOf(1 to 10L, 9 to 1L), fixture.capturedMovieVersions)
        assertEquals(listOf(2), fixture.transport.requestedMovieBatches)
        assertEquals(mapOf(1 to 10L, 2 to 20L), fixture.cache.cachedMovieBatchVersions(fixture.scope, library.id))
        assertEquals(listOf(1, 2), state.movieAccessor?.batchIds)
        assertEquals(3, state.movieAccessor?.movieCount)
        assertTrue(state.freshness is LibraryFreshness.Fresh)
    }

    @Test
    fun movieAccessorExposesEveryCachedBatchInSortedBatchOrderWhenOffline() = runTest {
        val fixture = Fixture()
        val library = movieLibrary()
        fixture.cache.writeMovieBatch(fixture.scope, library.id, 2, 2L, movieBatchResponse(batchId = 2, version = 2L, movieCount = 2))
        fixture.cache.writeMovieBatch(fixture.scope, library.id, 1, 1L, movieBatchResponse(batchId = 1, version = 1L, movieCount = 1))
        fixture.transport.movieSync = { LibrarySyncResult.Failure(LibrarySyncFailure.Network("offline")) }

        val state = fixture.repository.syncMovieLibrary(fixture.scope, library)

        assertTrue(state.freshness is LibraryFreshness.StaleOffline)
        assertEquals(listOf(1, 2), state.movieAccessor?.batchIds)
        assertEquals(3, state.movieAccessor?.movieCount)
    }

    @Test
    fun seriesBundleParserAcceptsSingleBundleRoot() {
        val seriesId = uuid(30).toString()
        val parsed = LibraryFlatBuffers.parseSeriesPayload(
            singleSeriesBundleRoot(seriesId = seriesId, version = 7L, itemCount = 1).wrap(),
            expectedSeriesId = seriesId,
        ).getOrThrow()

        val accessor = SeriesLibraryAccessor(parsed)
        assertEquals(SeriesPayloadRoot.SeriesBundleData, parsed.single().root)
        assertEquals(listOf(seriesId), accessor.seriesIds)
        assertEquals(1, accessor.bundleCount)
        assertEquals(1, accessor.seriesReferenceCount)
    }

    @Test
    fun seriesSyncCachesEveryKnownSeriesBundle() = runTest {
        val fixture = Fixture()
        val library = seriesLibrary()
        val first = uuid(41).toString()
        val second = uuid(42).toString()
        fixture.transport.seriesSync = { cached ->
            fixture.capturedSeriesVersions = cached
            LibrarySyncResult.Success(
                SeriesBundleSyncPlan(
                    staleSeriesIds = listOf(second, first),
                    deletedSeriesIds = emptyList(),
                    serverVersions = mapOf(first to 11L, second to 12L),
                ),
            )
        }
        fixture.transport.seriesFetches[first] = LibrarySyncResult.Success(seriesBundleFetchResponse(first, version = 11L, itemCount = 1))
        fixture.transport.seriesFetches[second] = LibrarySyncResult.Success(seriesBundleFetchResponse(second, version = 12L, itemCount = 1))

        val state = fixture.repository.syncSeriesLibrary(fixture.scope, library)

        assertEquals(emptyMap<String, Long>(), fixture.capturedSeriesVersions)
        assertEquals(listOf(first, second), fixture.transport.requestedSeriesBundles.sorted())
        assertEquals(listOf(first, second), state.seriesAccessor?.seriesIds)
        assertEquals(2, state.seriesAccessor?.bundleCount)
        assertTrue(state.freshness is LibraryFreshness.Fresh)
    }

    @Test
    fun cacheScopesSeparateAuthenticatedUsersOnTheSameServer() {
        val root = temporaryFolder.newFolder("cache")
        val cache = LibraryDiskCache(root)
        val server = "HTTP://Ferrex.Local/"
        val scopeA = ServerCacheScope.from(server, "user-a")
        val scopeB = ServerCacheScope.from("http://ferrex.local", "user-b")
        val library = movieLibrary()

        cache.writeMovieBatch(scopeA, library.id, 1, 1L, movieBatchResponse(batchId = 1, version = 1L, movieCount = 1))
        cache.writeMovieBatch(scopeB, library.id, 1, 2L, movieBatchResponse(batchId = 1, version = 2L, movieCount = 1))
        cache.clearSelectedLibrary(scopeA, library.id)

        assertTrue(cache.cachedMovieBatchVersions(scopeA, library.id).isEmpty())
        assertEquals(mapOf(1 to 2L), cache.cachedMovieBatchVersions(scopeB, library.id))
        assertFalse(scopeA.directoryName == scopeB.directoryName)
    }

    @Test
    fun selectedAndAllCacheClearStayInsideCurrentScope() {
        val root = temporaryFolder.newFolder("cache")
        val cache = LibraryDiskCache(root)
        val scopeA = ServerCacheScope.from("http://ferrex.local", "user-a")
        val scopeB = ServerCacheScope.from("http://other.local", "user-a")
        val firstLibrary = movieLibrary(id = uuid(60).toString())
        val secondLibrary = movieLibrary(id = uuid(61).toString())

        cache.writeMovieBatch(scopeA, firstLibrary.id, 1, 1L, movieBatchResponse(batchId = 1, version = 1L, movieCount = 1))
        cache.writeMovieBatch(scopeA, secondLibrary.id, 2, 1L, movieBatchResponse(batchId = 2, version = 1L, movieCount = 1))
        cache.writeMovieBatch(scopeB, firstLibrary.id, 1, 1L, movieBatchResponse(batchId = 1, version = 1L, movieCount = 1))
        val imageMetadata = cache.debugScopeDir(scopeA).resolve("images/manifest.properties")
        imageMetadata.parentFile?.mkdirs()
        imageMetadata.writeText("image=true")
        val searchMetadata = cache.debugScopeDir(scopeA).resolve("search/index.properties")
        searchMetadata.parentFile?.mkdirs()
        searchMetadata.writeText("search=true")

        cache.clearSelectedLibrary(scopeA, firstLibrary.id)

        assertTrue(cache.cachedMovieBatchVersions(scopeA, firstLibrary.id).isEmpty())
        assertEquals(mapOf(2 to 1L), cache.cachedMovieBatchVersions(scopeA, secondLibrary.id))
        assertTrue(imageMetadata.exists())
        assertTrue(searchMetadata.exists())

        cache.clearAllForScope(scopeA)

        assertTrue(cache.cachedMovieBatchVersions(scopeA, secondLibrary.id).isEmpty())
        assertFalse(imageMetadata.exists())
        assertFalse(searchMetadata.exists())
        assertEquals(mapOf(1 to 1L), cache.cachedMovieBatchVersions(scopeB, firstLibrary.id))
    }

    @Test
    fun corruptCachedMoviePayloadIsQuarantinedAndReportedAsRebuildable() = runTest {
        val fixture = Fixture()
        val library = movieLibrary()
        fixture.cache.writeMovieBatch(fixture.scope, library.id, 1, 1L, byteArrayOf(1, 2, 3, 4, 5))
        fixture.transport.movieSync = { LibrarySyncResult.Failure(LibrarySyncFailure.Network("offline")) }

        val state = fixture.repository.syncMovieLibrary(fixture.scope, library)

        assertTrue(state.freshness is LibraryFreshness.CorruptRebuilding)
        assertEquals(1, fixture.cache.quarantinedFiles(fixture.scope).size)
        assertTrue(fixture.cache.cachedMovieBatchVersions(fixture.scope, library.id).isEmpty())
    }

    @Test
    fun offlineFailureUsesStaleCacheAndWritesStaleMetadata() = runTest {
        val fixture = Fixture()
        val library = movieLibrary()
        fixture.cache.writeMovieBatch(fixture.scope, library.id, 1, 1L, movieBatchResponse(batchId = 1, version = 1L, movieCount = 1))
        fixture.transport.movieSync = { LibrarySyncResult.Failure(LibrarySyncFailure.Network("offline")) }

        val state = fixture.repository.syncMovieLibrary(fixture.scope, library)

        assertTrue(state.freshness is LibraryFreshness.StaleOffline)
        assertEquals(1, state.movieAccessor?.movieCount)
        assertTrue(fixture.cache.staleOfflineMetadataExists(fixture.scope))
    }

    @Test
    fun flatBufferRequestBuildersUseCurrentSeriesUuidSchema() {
        val seriesId = uuid(90).toString()

        val syncRequest = SeriesBundleSyncRequest.getRootAsSeriesBundleSyncRequest(
            LibraryFlatBuffers.buildSeriesBundleSyncRequest(mapOf(seriesId to 77L)).wrap(),
        )
        val version = syncRequest.cachedVersions(0)!!
        assertEquals(seriesId, version.seriesId.toUuidString())
        assertEquals(77L, version.version.toLong())

        val fetchRequest = SeriesBundleFetchRequest.getRootAsSeriesBundleFetchRequest(
            LibraryFlatBuffers.buildSeriesBundleFetchRequest(listOf(seriesId)).wrap(),
        )
        assertEquals(seriesId, fetchRequest.seriesIds(0)?.toUuidString())
    }

    @Test
    fun retryClassificationIsExact() {
        assertEquals(RetryClassification.Retryable, LibrarySyncFailure.Network("offline").classification)
        assertEquals(RetryClassification.Retryable, LibrarySyncFailure.Http(503, "unavailable").classification)
        assertEquals(RetryClassification.Retryable, LibrarySyncFailure.Http(429, "slow down").classification)
        assertEquals(RetryClassification.AuthRequired, LibrarySyncFailure.Http(401, "unauthorized").classification)
        assertEquals(RetryClassification.NotFound, LibrarySyncFailure.Http(404, "missing").classification)
        assertEquals(RetryClassification.NotRetryable, LibrarySyncFailure.Http(400, "bad request").classification)
        assertEquals(RetryClassification.InvalidResponse, LibrarySyncFailure.EmptyBody.classification)
        assertEquals(RetryClassification.InvalidResponse, LibrarySyncFailure.Parse("bad flatbuffer").classification)
    }

    private inner class Fixture {
        val cache = LibraryDiskCache(temporaryFolder.newFolder("cache-${System.nanoTime()}"))
        val scope = ServerCacheScope.from("http://ferrex.local/", "user-1")
        val transport = FakeLibrarySyncTransport()
        val repository = LibraryRepository(transport, cache)
        var capturedMovieVersions: Map<Int, Long> = emptyMap()
        var capturedSeriesVersions: Map<String, Long> = emptyMap()
    }

    private class FakeLibrarySyncTransport : LibrarySyncTransport {
        val movieFetches = mutableMapOf<Int, LibrarySyncResult<ByteArray>>()
        val seriesFetches = mutableMapOf<String, LibrarySyncResult<ByteArray>>()
        val requestedMovieBatches = mutableListOf<Int>()
        val requestedSeriesBundles = mutableListOf<String>()
        var libraries: LibrarySyncResult<ByteArray> = LibrarySyncResult.Failure(LibrarySyncFailure.Network("not configured"))
        var movieSync: (Map<Int, Long>) -> LibrarySyncResult<MovieBatchSyncPlan> = {
            LibrarySyncResult.Failure(LibrarySyncFailure.Network("not configured"))
        }
        var seriesSync: (Map<String, Long>) -> LibrarySyncResult<SeriesBundleSyncPlan> = {
            LibrarySyncResult.Failure(LibrarySyncFailure.Network("not configured"))
        }

        override suspend fun fetchLibraries(): LibrarySyncResult<ByteArray> = libraries

        override suspend fun syncMovieBatches(libraryId: String, cachedVersions: Map<Int, Long>): LibrarySyncResult<MovieBatchSyncPlan> =
            movieSync(cachedVersions)

        override suspend fun fetchMovieBatch(libraryId: String, batchId: Int): LibrarySyncResult<ByteArray> {
            requestedMovieBatches += batchId
            return movieFetches[batchId] ?: LibrarySyncResult.Failure(LibrarySyncFailure.Network("missing batch"))
        }

        override suspend fun syncSeriesBundles(libraryId: String, cachedVersions: Map<String, Long>): LibrarySyncResult<SeriesBundleSyncPlan> =
            seriesSync(cachedVersions)

        override suspend fun fetchSeriesBundle(libraryId: String, seriesId: String): LibrarySyncResult<ByteArray> {
            requestedSeriesBundles += seriesId
            return seriesFetches[seriesId] ?: LibrarySyncResult.Failure(LibrarySyncFailure.Network("missing bundle"))
        }
    }

    private companion object {
        private fun movieLibrary(id: String = uuid(10).toString()) = LibraryInfo(id, "Movies", LibraryKind.Movies)

        private fun seriesLibrary(id: String = uuid(20).toString()) = LibraryInfo(id, "Series", LibraryKind.Series)

        private fun uuid(seed: Int): UUID = UUID(0x018f5f8d00007000L + seed, 0x8000000000000000UL.toLong() + seed)

        private fun ByteArray.wrap() = java.nio.ByteBuffer.wrap(this).order(java.nio.ByteOrder.LITTLE_ENDIAN)

        private fun movieBatchResponse(batchId: Int, version: Long, movieCount: Int): ByteArray {
            val builder = FlatBufferBuilder(512)
            val mediaOffsets = (0 until movieCount).map { index ->
                val title = builder.createString("Movie $batchId-$index")
                MovieReference.startMovieReference(builder)
                MovieReference.addBatchId(builder, batchId.toUInt())
                MovieReference.addTitle(builder, title)
                MovieReference.addLibraryId(builder, uuid(10).toFlatBufferUuid(builder))
                MovieReference.addId(builder, uuid(batchId * 100 + index).toFlatBufferUuid(builder))
                val movie = MovieReference.endMovieReference(builder)
                Media.createMedia(builder, MediaVariant.MovieReference, movie)
            }.toIntArray()
            val items = MediaBatchData.createItemsVector(builder, mediaOffsets)
            val batch = MediaBatchData.createMediaBatchData(builder, batchId.toUInt(), version.toULong(), items)
            val batches = BatchFetchResponse.createBatchesVector(builder, intArrayOf(batch))
            val root = BatchFetchResponse.createBatchFetchResponse(builder, batches)
            builder.finish(root)
            return builder.sizedByteArray()
        }

        private fun singleSeriesBundleRoot(seriesId: String, version: Long, itemCount: Int): ByteArray {
            val builder = FlatBufferBuilder(512)
            val media = seriesMediaOffsets(builder, seriesId, itemCount)
            val items = SeriesBundleData.createItemsVector(builder, media)
            SeriesBundleData.startSeriesBundleData(builder)
            SeriesBundleData.addVersion(builder, version.toULong())
            SeriesBundleData.addItems(builder, items)
            SeriesBundleData.addSeriesId(builder, UUID.fromString(seriesId).toFlatBufferUuid(builder))
            val root = SeriesBundleData.endSeriesBundleData(builder)
            builder.finish(root)
            return builder.sizedByteArray()
        }

        private fun seriesBundleFetchResponse(seriesId: String, version: Long, itemCount: Int): ByteArray {
            val builder = FlatBufferBuilder(512)
            val media = seriesMediaOffsets(builder, seriesId, itemCount)
            val items = SeriesBundleData.createItemsVector(builder, media)
            SeriesBundleData.startSeriesBundleData(builder)
            SeriesBundleData.addVersion(builder, version.toULong())
            SeriesBundleData.addItems(builder, items)
            SeriesBundleData.addSeriesId(builder, UUID.fromString(seriesId).toFlatBufferUuid(builder))
            val bundle = SeriesBundleData.endSeriesBundleData(builder)
            val bundles = SeriesBundleFetchResponse.createBundlesVector(builder, intArrayOf(bundle))
            val root = SeriesBundleFetchResponse.createSeriesBundleFetchResponse(builder, bundles)
            builder.finish(root)
            return builder.sizedByteArray()
        }

        private fun seriesMediaOffsets(builder: FlatBufferBuilder, seriesId: String, itemCount: Int): IntArray =
            (0 until itemCount).map { index ->
                val title = builder.createString("Series $index")
                SeriesReference.startSeriesReference(builder)
                SeriesReference.addTitle(builder, title)
                SeriesReference.addLibraryId(builder, uuid(20).toFlatBufferUuid(builder))
                SeriesReference.addId(builder, UUID.fromString(seriesId).toFlatBufferUuid(builder))
                val series = SeriesReference.endSeriesReference(builder)
                Media.createMedia(builder, MediaVariant.SeriesReference, series)
            }.toIntArray()

        @Suppress("unused")
        private fun libraryListBytes(vararg libraries: LibraryInfo): ByteArray {
            val builder = FlatBufferBuilder(256)
            val offsets = libraries.map { info ->
                val name = builder.createString(info.name)
                Library.startLibrary(builder)
                Library.addName(builder, name)
                Library.addLibraryType(builder, if (info.kind == LibraryKind.Series) LibraryType.Series else LibraryType.Movies)
                Library.addId(builder, UUID.fromString(info.id).toFlatBufferUuid(builder))
                Library.endLibrary(builder)
            }.toIntArray()
            val items = LibraryList.createItemsVector(builder, offsets)
            val root = LibraryList.createLibraryList(builder, items)
            builder.finish(root)
            return builder.sizedByteArray()
        }
    }
}
