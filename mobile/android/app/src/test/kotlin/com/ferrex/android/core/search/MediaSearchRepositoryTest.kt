package com.ferrex.android.core.search

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.library.CachedMediaReference
import com.ferrex.android.core.library.CachedMediaResyncSummary
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.ServerCacheScope
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.ArrayDeque

class MediaSearchRepositoryTest {
    @Test
    fun resolvesMovieAndSeriesHitsAgainstCachedReferences() = runTest {
        val movieId = mediaId(SearchMediaType.Movie, 1)
        val seriesId = mediaId(SearchMediaType.Series, 2)
        val cache = FakeSearchCache().apply {
            references[movieId] = CachedMediaReference.Movie(
                id = movieId.id,
                libraryId = libraryId(1),
                title = "Alien",
                imageKey = ImageRequestKey(uuid(50), BrowseImageCategory.Poster),
                publicFallbackPath = "/alien.jpg",
            )
            references[seriesId] = CachedMediaReference.Series(
                id = seriesId.id,
                libraryId = libraryId(2),
                title = "The Expanse",
                imageKey = null,
                publicFallbackPath = null,
            )
        }
        val repository = MediaSearchRepository(
            transport = FakeSearchTransport(ApiResult.Success(listOf(SearchMediaWithStatus(movieId), SearchMediaWithStatus(seriesId)))),
            cache = cache,
        )

        val outcome = repository.search(scope, "space")

        assertTrue(outcome is MediaSearchOutcome.Results)
        val rows = (outcome as MediaSearchOutcome.Results).rows.filterIsInstance<SearchResultRow.Resolved>()
        assertEquals(2, rows.size)
        assertEquals("Alien", rows[0].title)
        assertEquals(SearchDetailTarget(SearchMediaType.Movie, movieId.id, libraryId(1)), rows[0].target)
        assertEquals("The Expanse", rows[1].title)
        assertEquals(SearchDetailTarget(SearchMediaType.Series, seriesId.id, libraryId(2)), rows[1].target)
    }

    @Test
    fun episodeAndSeasonHitsRouteToSeriesWhenBundleIsCached() = runTest {
        val episodeId = mediaId(SearchMediaType.Episode, 3)
        val seasonId = mediaId(SearchMediaType.Season, 4)
        val parentSeriesId = uuid(40)
        val cache = FakeSearchCache().apply {
            references[episodeId] = CachedMediaReference.Episode(
                id = episodeId.id,
                libraryId = libraryId(3),
                title = "CQB",
                imageKey = ImageRequestKey(uuid(51), BrowseImageCategory.Episode),
                publicFallbackPath = "/still.jpg",
                seriesId = parentSeriesId,
                seasonId = seasonId.id,
                seasonNumber = 1,
                episodeNumber = 4,
            )
            references[seasonId] = CachedMediaReference.Season(
                id = seasonId.id,
                libraryId = libraryId(3),
                title = "Season 1",
                imageKey = null,
                publicFallbackPath = null,
                seriesId = parentSeriesId,
                seasonNumber = 1,
            )
        }
        val repository = MediaSearchRepository(
            transport = FakeSearchTransport(ApiResult.Success(listOf(SearchMediaWithStatus(episodeId), SearchMediaWithStatus(seasonId)))),
            cache = cache,
        )

        val rows = (repository.search(scope, "cqb") as MediaSearchOutcome.Results).rows.filterIsInstance<SearchResultRow.Resolved>()

        assertEquals(SearchDetailTarget(SearchMediaType.Series, parentSeriesId, libraryId(3)), rows[0].target)
        assertEquals("S1 E4 • Opens series detail", rows[0].subtitle)
        assertEquals(SearchDetailTarget(SearchMediaType.Series, parentSeriesId, libraryId(3)), rows[1].target)
        assertEquals("Season 1 • Opens series detail", rows[1].subtitle)
    }

    @Test
    fun cacheMissesStayVisibleAfterBoundedResyncInsteadOfBeingDropped() = runTest {
        val episodeId = mediaId(SearchMediaType.Episode, 5)
        val cache = FakeSearchCache().apply {
            resyncSummary = CachedMediaResyncSummary(
                attemptedLibraryIds = listOf(libraryId(3), libraryId(4)),
                bounded = true,
            )
        }
        val repository = MediaSearchRepository(
            transport = FakeSearchTransport(ApiResult.Success(listOf(SearchMediaWithStatus(episodeId)))),
            cache = cache,
        )

        val outcome = repository.search(scope, "missing episode")

        assertTrue(outcome is MediaSearchOutcome.Results)
        val row = (outcome as MediaSearchOutcome.Results).rows.single()
        assertTrue(row is SearchResultRow.CacheMiss)
        val miss = row as SearchResultRow.CacheMiss
        assertEquals(listOf(libraryId(3), libraryId(4)), miss.attemptedLibraryIds)
        assertTrue(miss.message.contains("silently dropped"))
        assertEquals(listOf(episodeId), cache.resyncCalls)
    }

    @Test
    fun emptyServerResultsBecomeNoResultsState() = runTest {
        val repository = MediaSearchRepository(
            transport = FakeSearchTransport(ApiResult.Success(emptyList())),
            cache = FakeSearchCache(),
        )

        val outcome = repository.search(scope, "zzzz")

        assertEquals(MediaSearchOutcome.NoResults("zzzz"), outcome)
    }

    @Test
    fun retryAfterHttpAndNetworkFailureCanReturnResults() = runTest {
        val movieId = mediaId(SearchMediaType.Movie, 6)
        val cache = FakeSearchCache().apply {
            references[movieId] = CachedMediaReference.Movie(
                id = movieId.id,
                libraryId = libraryId(6),
                title = "Retry Movie",
                imageKey = null,
                publicFallbackPath = null,
            )
        }
        val transport = FakeSearchTransport(
            ApiResult.HttpError(503, "unavailable"),
            ApiResult.NetworkError("offline"),
            ApiResult.Success(listOf(SearchMediaWithStatus(movieId))),
        )
        val repository = MediaSearchRepository(transport, cache)

        val httpFailure = repository.search(scope, "retry")
        val networkFailure = repository.search(scope, "retry")
        val success = repository.search(scope, "retry")

        assertTrue(httpFailure is MediaSearchOutcome.Failure)
        assertEquals(SearchFailureKind.Http, (httpFailure as MediaSearchOutcome.Failure).kind)
        assertTrue(networkFailure is MediaSearchOutcome.Failure)
        assertEquals(SearchFailureKind.NetworkOffline, (networkFailure as MediaSearchOutcome.Failure).kind)
        assertTrue(success is MediaSearchOutcome.Results)
        assertEquals("Retry Movie", ((success as MediaSearchOutcome.Results).rows.single() as SearchResultRow.Resolved).title)
    }

    @Test
    fun staleCacheFreshnessIsReportedWithResolvedRows() = runTest {
        val movieId = mediaId(SearchMediaType.Movie, 7)
        val cache = FakeSearchCache().apply {
            freshness = LibraryFreshness.StaleOffline("offline", itemCount = 1, lastSyncedAtMillis = null)
            references[movieId] = CachedMediaReference.Movie(
                id = movieId.id,
                libraryId = libraryId(7),
                title = "Cached Offline",
                imageKey = null,
                publicFallbackPath = null,
            )
        }
        val repository = MediaSearchRepository(
            transport = FakeSearchTransport(ApiResult.Success(listOf(SearchMediaWithStatus(movieId)))),
            cache = cache,
        )

        val outcome = repository.search(scope, "offline")

        assertTrue(outcome is MediaSearchOutcome.Results)
        assertTrue((outcome as MediaSearchOutcome.Results).staleCache)
    }

    private class FakeSearchTransport(vararg results: ApiResult<List<SearchMediaWithStatus>>) : MediaSearchTransport {
        private val queue = ArrayDeque(results.toList())

        override suspend fun queryMedia(searchText: String, limit: Int): ApiResult<List<SearchMediaWithStatus>> = queue.removeFirst()
    }

    private class FakeSearchCache : MediaSearchCache {
        val references = mutableMapOf<SearchMediaId, CachedMediaReference>()
        val resyncCalls = mutableListOf<SearchMediaId>()
        var resyncSummary = CachedMediaResyncSummary(emptyList(), bounded = false)
        var freshness: LibraryFreshness = LibraryFreshness.Fresh(itemCount = 1, syncedAtMillis = 1L)

        override fun resolve(scope: ServerCacheScope, id: SearchMediaId): CachedMediaReference? = references[id]

        override fun freshness(scope: ServerCacheScope): LibraryFreshness = freshness

        override suspend fun resync(scope: ServerCacheScope, id: SearchMediaId): CachedMediaResyncSummary {
            resyncCalls += id
            return resyncSummary
        }
    }

    private companion object {
        val scope: ServerCacheScope = ServerCacheScope.from("http://ferrex.local", "user-1")

        fun mediaId(type: SearchMediaType, seed: Int): SearchMediaId = SearchMediaId(type, uuid(seed))

        fun libraryId(seed: Int): String = uuid(100 + seed)

        fun uuid(seed: Int): String = "018f5f8d-0000-7000-8000-%012d".format(seed)
    }
}
