package com.ferrex.android.core.watch

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ContinueWatchingRepositoryTest {
    @Test
    fun mapsContinueWatchingRouteArgumentsFromActionTarget() {
        val card = ContinueWatchingMapper.toCard(
            ContinueWatchingApiItem(
                mediaId = "series-card-playback-id",
                mediaType = "Series",
                cardMediaId = "series-card-id",
                actionTarget = ContinueWatchingActionTarget(
                    mediaId = "episode-id",
                    mediaType = "Episode",
                ),
                actionHint = "next_episode",
                title = "Series",
                subtitle = "S01E02",
            ),
        )

        assertEquals(BrowseMediaType.Episode, card.route.mediaType)
        assertEquals("episode-id", card.route.mediaId)
        assertEquals(BrowseSourceSurface.HomeContinueWatching, card.route.sourceSurface)
        assertEquals("media/episode/episode-id?source=home_continue_watching", card.route.toRouteString())
    }

    @Test
    fun staleOfflineStateKeepsPreviousContinueWatchingCards() = runTest {
        val transport = FakeContinueWatchingTransport()
        val repository = ContinueWatchingRepository(transport)
        transport.next = ContinueWatchingResult.Success(
            listOf(
                ContinueWatchingApiItem(
                    mediaId = "movie-id",
                    mediaType = "Movie",
                    cardMediaId = "movie-id",
                    actionTarget = ContinueWatchingActionTarget("movie-id", "Movie"),
                    actionHint = "resume",
                    position = 60f,
                    duration = 600f,
                    title = "Movie",
                ),
            ),
        )

        val fresh = repository.refresh()
        transport.next = ContinueWatchingResult.Failure("offline")
        val stale = repository.refresh()

        assertTrue(fresh.status is ContinueWatchingStatus.Fresh)
        assertTrue(stale.status is ContinueWatchingStatus.StaleOffline)
        assertEquals(1, stale.cards.size)
        assertEquals("Movie", stale.cards.single().title)
    }

    @Test
    fun emptyFailureIsRetryableWithoutBlockingLibraryBrowse() = runTest {
        val repository = ContinueWatchingRepository(
            FakeContinueWatchingTransport().apply {
                next = ContinueWatchingResult.Failure("server unavailable")
            },
        )

        val state = repository.refresh()

        assertTrue(state.status is ContinueWatchingStatus.ErrorRetryable)
        assertEquals(emptyList<ContinueWatchingCard>(), state.cards)
    }

    private class FakeContinueWatchingTransport : ContinueWatchingTransport {
        var next: ContinueWatchingResult<List<ContinueWatchingApiItem>> = ContinueWatchingResult.Success(emptyList())

        override suspend fun fetchContinueWatching(): ContinueWatchingResult<List<ContinueWatchingApiItem>> = next
    }
}
