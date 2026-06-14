package com.ferrex.android.core.watch

import com.ferrex.android.core.api.ApiResult
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class WatchRepositoryTest {
    @Test
    fun optimisticMovieMutationRollsBackOnFailure() = runTest {
        val transport = FakeWatchStateTransport()
        val bus = WatchStateInvalidationBus()
        val repository = WatchRepository(transport, bus, UnconfinedTestDispatcher(testScheduler))
        transport.mediaProgress = ApiResult.Success(WatchMediaProgress("movie-id", positionSeconds = 30.0, durationSeconds = 300.0))
        repository.refreshMediaProgress("movie-id")
        transport.movieMutation = ApiResult.NetworkError("offline")

        val result = repository.markMovieWatched("movie-id", watched = true)

        assertTrue(result is ApiResult.NetworkError)
        val progress = repository.state.value.media["movie-id"]!!
        assertFalse(progress.isCompleted)
        assertEquals(30.0, progress.positionSeconds, 0.0)
        assertTrue(repository.state.value.lastError!!.contains("offline"))
    }

    @Test
    fun successfulEpisodeMutationCommitsOptimisticStateAndInvalidatesWatchState() = runTest {
        val transport = FakeWatchStateTransport()
        val bus = WatchStateInvalidationBus()
        val repository = WatchRepository(transport, bus, UnconfinedTestDispatcher(testScheduler))
        val events = mutableListOf<WatchStateInvalidation>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            bus.events.take(1).toList(events)
        }

        val result = repository.markEpisodeWatched("episode-id", watched = true)

        assertTrue(result is ApiResult.Success)
        val progress = repository.state.value.media["episode-id"]!!
        assertTrue(progress.isCompleted)
        assertFalse(progress.pendingMutation)
        assertEquals("episode watched:episode-id", events.single().reason)
    }

    @Test
    fun successfulSeriesMutationUpdatesAggregateState() = runTest {
        val transport = FakeWatchStateTransport()
        val bus = WatchStateInvalidationBus()
        val repository = WatchRepository(transport, bus, UnconfinedTestDispatcher(testScheduler))
        transport.seriesStatus = ApiResult.Success(
            WatchSeriesStatus(
                tmdbSeriesId = 42,
                totalEpisodes = 2,
                watched = 0,
                inProgress = 1,
                seasons = mapOf(
                    1 to WatchSeasonStatus(
                        seasonNumber = 1,
                        total = 2,
                        watched = 0,
                        inProgress = 1,
                        isCompleted = false,
                        episodes = mapOf(1 to WatchEpisodeStatus(WatchEpisodeState.InProgress, 0.5f)),
                    ),
                ),
                nextEpisode = null,
            ),
        )
        repository.refreshSeries(42)

        val result = repository.markSeriesWatched(42, watched = true)

        assertTrue(result is ApiResult.Success)
        val status = repository.state.value.series[42]!!
        assertEquals(2, status.watched)
        assertEquals(0, status.inProgress)
        assertTrue(status.isCompleted)
        assertFalse(status.pendingMutation)
    }

    private class FakeWatchStateTransport : WatchStateTransport {
        var mediaProgress: ApiResult<WatchMediaProgress?> = ApiResult.Success(null)
        var watchState: ApiResult<WatchStateSnapshot> = ApiResult.Success(WatchStateSnapshot())
        var seriesStatus: ApiResult<WatchSeriesStatus> = ApiResult.Success(
            WatchSeriesStatus(0, 0, 0, 0, emptyMap(), null),
        )
        var nextEpisode: ApiResult<WatchNextEpisode?> = ApiResult.Success(null)
        var movieMutation: ApiResult<Unit> = ApiResult.Success(Unit)
        var episodeMutation: ApiResult<Unit> = ApiResult.Success(Unit)
        var seriesMutation: ApiResult<Unit> = ApiResult.Success(Unit)

        override suspend fun fetchMediaProgress(mediaId: String): ApiResult<WatchMediaProgress?> = mediaProgress
        override suspend fun fetchWatchState(): ApiResult<WatchStateSnapshot> = watchState
        override suspend fun fetchSeriesWatchStatus(tmdbSeriesId: Long): ApiResult<WatchSeriesStatus> = seriesStatus
        override suspend fun fetchSeriesNextEpisode(tmdbSeriesId: Long): ApiResult<WatchNextEpisode?> = nextEpisode
        override suspend fun markMovieWatched(mediaId: String, watched: Boolean): ApiResult<Unit> = movieMutation
        override suspend fun markEpisodeWatched(mediaId: String, watched: Boolean): ApiResult<Unit> = episodeMutation
        override suspend fun markSeriesWatched(tmdbSeriesId: Long, watched: Boolean): ApiResult<Unit> = seriesMutation
    }
}
