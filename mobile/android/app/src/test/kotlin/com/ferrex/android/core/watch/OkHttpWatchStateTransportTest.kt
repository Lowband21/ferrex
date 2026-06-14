package com.ferrex.android.core.watch

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import kotlinx.coroutines.test.runTest
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OkHttpWatchStateTransportTest {
    @Test
    fun parsesCurrentJsonWatchEndpointsAndUsesRoutes() = runTest {
        MockWebServer().use { server ->
            server.enqueue(
                MockResponse().setBody(
                    """
                    {"status":"success","data":{"media_id":"movie-id","position":50.0,"duration":100.0,"percentage":50.0,"is_completed":false}}
                    """.trimIndent(),
                ),
            )
            server.enqueue(
                MockResponse().setBody(
                    """
                    {"status":"success","data":{"in_progress":[{"media_id":"episode-id","position":25.0,"duration":100.0,"last_watched":10}],"completed":["movie-id"]}}
                    """.trimIndent(),
                ),
            )
            server.enqueue(MockResponse().setBody(seriesStatusJson()))
            server.enqueue(
                MockResponse().setBody(
                    """
                    {"status":"success","data":{"key":{"tmdb_series_id":1234,"season_number":1,"episode_number":2},"playable_media_id":"episode-2","reason":"resume_in_progress"}}
                    """.trimIndent(),
                ),
            )
            val transport = transport(server)

            val progress = transport.fetchMediaProgress("movie-id") as ApiResult.Success<WatchMediaProgress?>
            val state = transport.fetchWatchState() as ApiResult.Success<WatchStateSnapshot>
            val series = transport.fetchSeriesWatchStatus(1234) as ApiResult.Success<WatchSeriesStatus>
            val next = transport.fetchSeriesNextEpisode(1234) as ApiResult.Success<WatchNextEpisode?>

            assertEquals(0.5f, progress.data!!.progressRatio, 0.0f)
            assertTrue(state.data.completed.contains("movie-id"))
            assertEquals(0.25f, state.data.inProgress["episode-id"]!!.progressRatio, 0.0f)
            assertEquals(WatchEpisodeState.Completed, series.data.episodeStatus(1, 1).state)
            assertEquals(0.5f, series.data.episodeStatus(1, 2).progress, 0.0f)
            assertEquals("episode-2", next.data!!.playableMediaId)

            assertEquals("/api/v1/media/movie-id/progress", server.takeRequest().path)
            assertEquals("/api/v1/watch/state", server.takeRequest().path)
            assertEquals("/api/v1/watch/series/1234", server.takeRequest().path)
            assertEquals("/api/v1/watch/series/1234/next", server.takeRequest().path)
        }
    }

    @Test
    fun mutationsUseCurrentPostDeleteRoutesWithoutWatchFlatBuffers() = runTest {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(204))
            server.enqueue(MockResponse().setResponseCode(204))
            server.enqueue(MockResponse().setResponseCode(204))
            val transport = transport(server)

            assertTrue(transport.markMovieWatched("movie-id", watched = true) is ApiResult.Success)
            assertTrue(transport.markEpisodeWatched("episode-id", watched = false) is ApiResult.Success)
            assertTrue(transport.markSeriesWatched(1234, watched = true) is ApiResult.Success)

            val movie = server.takeRequest()
            assertEquals("POST", movie.method)
            assertEquals("/api/v1/watch/movies/movie-id/watched", movie.path)
            val episode = server.takeRequest()
            assertEquals("DELETE", episode.method)
            assertEquals("/api/v1/watch/episodes/episode-id/watched", episode.path)
            val series = server.takeRequest()
            assertEquals("POST", series.method)
            assertEquals("/api/v1/watch/series/1234/watched", series.path)
            assertFalse(movie.getHeader("Accept").orEmpty().contains("flatbuffers", ignoreCase = true))
        }
    }

    private fun transport(server: MockWebServer): OkHttpWatchStateTransport {
        val config = ServerConfig()
        config.setUrl(server.url("/").toString())
        return OkHttpWatchStateTransport(OkHttpClient(), config)
    }

    private fun seriesStatusJson(): String =
        """
        {
          "status":"success",
          "data":{
            "tmdb_series_id":1234,
            "total_episodes":2,
            "watched":1,
            "in_progress":1,
            "seasons":{
              "1":{
                "key":{"tmdb_series_id":1234,"season_number":1},
                "total":2,
                "watched":1,
                "in_progress":1,
                "is_completed":false,
                "episodes":{
                  "1":{"state":"completed"},
                  "2":{"state":"in_progress","progress":0.5}
                }
              }
            },
            "next_episode":{"key":{"tmdb_series_id":1234,"season_number":1,"episode_number":2},"playable_media_id":"episode-2","reason":"resume_in_progress"}
          }
        }
        """.trimIndent()
}
