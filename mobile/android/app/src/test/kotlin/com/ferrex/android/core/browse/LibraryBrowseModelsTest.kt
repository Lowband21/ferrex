package com.ferrex.android.core.browse

import com.ferrex.android.core.library.CachedMovieLibrary
import com.ferrex.android.core.library.CachedSeriesLibrary
import com.ferrex.android.core.library.LibraryFlatBuffers
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.LibraryKind
import com.ferrex.android.core.library.MovieLibraryAccessor
import com.ferrex.android.core.library.RetryClassification
import com.ferrex.android.core.library.SeriesLibraryAccessor
import com.ferrex.android.core.library.toFlatBufferUuid
import com.google.flatbuffers.FlatBufferBuilder
import ferrex.details.EnhancedSeriesDetails
import ferrex.library.BatchFetchResponse
import ferrex.library.MediaBatchData
import ferrex.library.SeriesBundleData
import ferrex.library.SeriesBundleFetchResponse
import ferrex.media.Media
import ferrex.media.MediaVariant
import ferrex.media.MovieReference
import ferrex.media.SeriesReference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

@OptIn(ExperimentalUnsignedTypes::class)
class LibraryBrowseModelsTest {
    @Test
    fun movieGridUsesEveryMovieAcrossAllCachedBatches() {
        val library = movieLibrary()
        val batches = LibraryFlatBuffers.parseMoviePayload(movieBatches(library.id, 2 to 3, 1 to 2).wrap()).getOrThrow()
        val accessor = MovieLibraryAccessor(batches)

        val cards = LibraryBrowseModels.movieGridCards(library, accessor)

        assertEquals(5, accessor.movieCount)
        assertEquals(5, cards.size)
        assertEquals(
            setOf("Movie 1-0", "Movie 1-1", "Movie 2-0", "Movie 2-1", "Movie 2-2"),
            cards.map { it.title }.toSet(),
        )
        assertTrue(cards.all { it.route.libraryId == library.id })
    }

    @Test
    fun seriesGridUsesEveryCachedSeriesBundle() {
        val library = seriesLibrary()
        val firstSeries = uuid(201).toString()
        val secondSeries = uuid(202).toString()
        val bundles = LibraryFlatBuffers.parseSeriesPayload(seriesBundle(library.id, firstSeries, "Alpha").wrap()).getOrThrow() +
            LibraryFlatBuffers.parseSeriesPayload(seriesBundle(library.id, secondSeries, "Beta").wrap()).getOrThrow()
        val accessor = SeriesLibraryAccessor(bundles)

        val cards = LibraryBrowseModels.seriesGridCards(library, accessor)

        assertEquals(2, accessor.bundleCount)
        assertEquals(2, accessor.seriesReferenceCount)
        assertEquals(listOf("Alpha", "Beta"), cards.map { it.title }.sorted())
        assertTrue(cards.all { it.route.mediaType == BrowseMediaType.Series })
    }

    @Test
    fun staleOfflineAndRetryableCopiesAreExplicit() {
        val stale = LibraryBrowseModels.libraryStatusCopy(
            LibraryFreshness.StaleOffline(
                message = "network down",
                itemCount = 8,
                lastSyncedAtMillis = null,
            ),
        )
        val retryable = LibraryBrowseModels.libraryStatusCopy(
            LibraryFreshness.ErrorRetryable(
                message = "unauthorized",
                classification = RetryClassification.AuthRequired,
            ),
        )

        assertTrue(stale.isStale)
        assertTrue(stale.title.contains("Stale/offline"))
        assertTrue(stale.detail.contains("Cached 8 item(s)"))
        assertTrue(stale.detail.contains("expected, pending, and failed counts are unknown"))
        assertTrue(stale.detail.contains("network down"))
        assertTrue(retryable.isRecoverableError)
        assertTrue(retryable.title.contains("sign-in"))
        assertTrue(retryable.detail.contains("failed"))
    }

    @Test
    fun incompleteSeriesStatusReportsExpectedCachedPendingAndFailedCounts() {
        val copy = LibraryBrowseModels.libraryStatusCopy(
            LibraryFreshness.SeriesCacheIncomplete(
                message = "network interrupted",
                completedBundles = 36,
                expectedBundles = 400,
                remainingBundleIds = (0 until 364).map { "series-$it" },
                itemCount = 172,
                classification = RetryClassification.Retryable,
                failedBundleCount = 16,
            ),
        )

        assertTrue(copy.isStale)
        assertTrue(copy.title.contains("incomplete"))
        assertTrue(copy.detail.contains("Cached 172 item(s)"))
        assertTrue(copy.detail.contains("series bundles cached 36/400 expected"))
        assertTrue(copy.detail.contains("pending 364"))
        assertTrue(copy.detail.contains("failed 16"))
        assertTrue(copy.detail.contains("selected series library repair"))
    }

    @Test
    fun retryAllTargetsStayScopedToActiveMediaType() {
        val movie = movieLibrary(id = uuid(301).toString())
        val firstSeries = seriesLibrary(id = uuid(302).toString())
        val secondSeries = seriesLibrary(id = uuid(303).toString())

        val seriesPlan = LibraryBrowseModels.retryAllTargetPlan(HomeLibraryTab.Series, listOf(movie, firstSeries, secondSeries))
        val moviePlan = LibraryBrowseModels.retryAllTargetPlan(HomeLibraryTab.Movies, listOf(movie, firstSeries))

        assertEquals("Retry all series libraries", seriesPlan.label)
        assertEquals(listOf(firstSeries, secondSeries), seriesPlan.libraries)
        assertEquals(listOf(movie), moviePlan.libraries)
    }

    @Test
    fun unsupportedSeriesSortFilterCopyExplainsLimitation() {
        val copy = LibraryBrowseModels.unsupportedSeriesControlsCopy()

        assertTrue(copy.contains("Series sort and filters are disabled"))
        assertTrue(copy.contains("only support movie libraries"))
        assertTrue(copy.contains("full cached series grid"))
    }

    @Test
    fun recoveryActionsExposeRequiredEscapesWhenLibrarySelected() {
        val visible = LibraryBrowseModels.recoveryActionVisibility(selectedLibraryId = "library-1")
        val noSelection = LibraryBrowseModels.recoveryActionVisibility(selectedLibraryId = null)

        assertTrue(visible.retry)
        assertTrue(visible.clearSelectedCache)
        assertTrue(visible.changeServer)
        assertTrue(visible.resetConnection)
        assertFalse(noSelection.clearSelectedCache)
    }

    @Test
    fun routeArgumentsCarryTypeIdLibraryAndSourceSurface() {
        val route = MediaRouteArgs(
            mediaType = BrowseMediaType.Movie,
            mediaId = "movie-id",
            libraryId = "library-id",
            sourceSurface = BrowseSourceSurface.LibraryGrid,
        )

        assertEquals("media/movie/movie-id?source=library_grid&libraryId=library-id", route.toRouteString())
        assertEquals("movie:movie-id:library-id:library_grid", route.stableKey)
    }

    @Test
    fun homeShelfPreviewLimitDoesNotCapFullLibraryGrid() {
        val library = movieLibrary()
        val accessor = MovieLibraryAccessor(
            LibraryFlatBuffers.parseMoviePayload(movieBatch(library.id, batchId = 1, movieCount = 15).wrap()).getOrThrow(),
        )
        val cached = CachedMovieLibrary(library, accessor)

        val shelves = LibraryBrowseModels.homeShelves(
            movieLibraries = listOf(cached),
            seriesLibraries = emptyList(),
            previewLimit = 4,
        )
        val fullGrid = LibraryBrowseModels.movieGridCards(cached)

        assertEquals(15, fullGrid.size)
        assertEquals(15, shelves.single().fullItemCount)
        assertEquals(4, shelves.single().items.size)
        assertTrue(shelves.single().limitCopy.contains("Shelf preview limit 4 of 15"))
    }

    @Test
    fun endpointIndicesAppendMissingMoviesForSortButNotFilter() {
        val cards = (0 until 4).map { index ->
            LibraryMediaCard(
                stableKey = "movie:$index",
                title = "Movie $index",
                subtitle = "Movie",
                libraryName = "Movies",
                route = MediaRouteArgs(BrowseMediaType.Movie, "movie-$index", "library", BrowseSourceSurface.LibraryGrid),
                imageKey = null,
                publicFallbackPath = null,
                releaseDate = null,
            )
        }

        val sorted = LibraryBrowseModels.applyMovieIndices(cards, indices = listOf(2, 99, 0), appendMissing = true)
        val filtered = LibraryBrowseModels.applyMovieIndices(cards, indices = listOf(2, 99, 0), appendMissing = false)

        assertEquals(listOf("Movie 2", "Movie 0", "Movie 1", "Movie 3"), sorted.cards.map { it.title })
        assertEquals(1, sorted.invalidIndexCount)
        assertEquals(2, sorted.appendedMissingCount)
        assertEquals(listOf("Movie 2", "Movie 0"), filtered.cards.map { it.title })
        assertEquals(0, filtered.appendedMissingCount)
    }

    private companion object {
        private fun movieLibrary(id: String = uuid(10).toString()) = LibraryInfo(id, "Movies", LibraryKind.Movies)

        private fun seriesLibrary(id: String = uuid(20).toString()) = LibraryInfo(id, "Series", LibraryKind.Series)

        private fun uuid(seed: Int): UUID = UUID(0x018f5f8d00007000L + seed, 0x8000000000000000UL.toLong() + seed)

        private fun ByteArray.wrap(): ByteBuffer = ByteBuffer.wrap(this).order(ByteOrder.LITTLE_ENDIAN)

        private fun movieBatch(libraryId: String, batchId: Int, movieCount: Int): ByteArray = movieBatches(libraryId, batchId to movieCount)

        private fun movieBatches(libraryId: String, vararg specs: Pair<Int, Int>): ByteArray {
            val builder = FlatBufferBuilder(2048)
            val libraryUuid = UUID.fromString(libraryId)
            val batchOffsets = specs.map { (batchId, movieCount) ->
                val mediaOffsets = (0 until movieCount).map { index ->
                    val title = builder.createString("Movie $batchId-$index")
                    MovieReference.startMovieReference(builder)
                    MovieReference.addBatchId(builder, batchId.toUInt())
                    MovieReference.addTitle(builder, title)
                    MovieReference.addLibraryId(builder, libraryUuid.toFlatBufferUuid(builder))
                    MovieReference.addId(builder, uuid(batchId * 100 + index).toFlatBufferUuid(builder))
                    val movie = MovieReference.endMovieReference(builder)
                    Media.createMedia(builder, MediaVariant.MovieReference, movie)
                }.toIntArray()
                val items = MediaBatchData.createItemsVector(builder, mediaOffsets)
                MediaBatchData.createMediaBatchData(builder, batchId.toUInt(), 1UL, items)
            }.toIntArray()
            val batches = BatchFetchResponse.createBatchesVector(builder, batchOffsets)
            val root = BatchFetchResponse.createBatchFetchResponse(builder, batches)
            builder.finish(root)
            return builder.sizedByteArray()
        }

        private fun seriesBundle(libraryId: String, seriesId: String, titleValue: String): ByteArray {
            val builder = FlatBufferBuilder(512)
            val libraryUuid = UUID.fromString(libraryId)
            val title = builder.createString(titleValue)
            val firstAirDate = builder.createString("2024-02-01")
            EnhancedSeriesDetails.startEnhancedSeriesDetails(builder)
            EnhancedSeriesDetails.addFirstAirDate(builder, firstAirDate)
            EnhancedSeriesDetails.addAvailableEpisodes(builder, 8.toUShort())
            val details = EnhancedSeriesDetails.endEnhancedSeriesDetails(builder)
            SeriesReference.startSeriesReference(builder)
            SeriesReference.addTitle(builder, title)
            SeriesReference.addDetails(builder, details)
            SeriesReference.addLibraryId(builder, libraryUuid.toFlatBufferUuid(builder))
            SeriesReference.addId(builder, UUID.fromString(seriesId).toFlatBufferUuid(builder))
            val series = SeriesReference.endSeriesReference(builder)
            val media = Media.createMedia(builder, MediaVariant.SeriesReference, series)
            val items = SeriesBundleData.createItemsVector(builder, intArrayOf(media))
            SeriesBundleData.startSeriesBundleData(builder)
            SeriesBundleData.addVersion(builder, 1UL)
            SeriesBundleData.addItems(builder, items)
            SeriesBundleData.addSeriesId(builder, UUID.fromString(seriesId).toFlatBufferUuid(builder))
            val bundle = SeriesBundleData.endSeriesBundleData(builder)
            val bundles = SeriesBundleFetchResponse.createBundlesVector(builder, intArrayOf(bundle))
            val root = SeriesBundleFetchResponse.createSeriesBundleFetchResponse(builder, bundles)
            builder.finish(root)
            return builder.sizedByteArray()
        }
    }
}
