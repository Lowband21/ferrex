package com.ferrex.android.core.playback

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.AuthInterceptor
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.browse.BrowseMediaType
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackFoundationTest {
    @Test
    fun fetchTicketUsesAuthenticatedTicketRouteAndBuildsTicketedStreamUrl() = runTest {
        MockWebServer().use { server ->
            server.enqueue(
                MockResponse()
                    .setResponseCode(200)
                    .setBody("""{"status":"success","data":{"access_token":"playback-ticket-secret","expires_in":21600}}"""),
            )
            server.start()

            val serverConfig = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val authInterceptor = AuthInterceptor().apply { setAccessToken("full-session-secret") }
            val client = OkHttpClient.Builder().addInterceptor(authInterceptor).build()
            val transport = OkHttpPlaybackTicketTransport(client, serverConfig)

            val result = transport.fetchTicket("media-file-id")

            assertTrue(result is ApiResult.Success)
            val ticket = (result as ApiResult.Success).data
            assertEquals("playback-ticket-secret", ticket.token)
            assertEquals(21600L, ticket.expiresInSeconds)

            val request = server.takeRequest()
            assertEquals("/api/v1/stream/media-file-id/ticket", request.path)
            assertEquals("Bearer full-session-secret", request.getHeader("Authorization"))

            val streamUrl = PlaybackStreamUrlFactory(serverConfig).streamUrl("media-file-id", ticket)
            assertTrue(streamUrl.contains("access_token=playback-ticket-secret"))
            assertFalse(streamUrl.contains("full-session-secret"))
        }
    }

    @Test
    @OptIn(ExperimentalCoroutinesApi::class)
    fun ticketAuthFailuresRetryThenInvalidateSession() = runTest {
        listOf(
            401 to PlaybackFailureKind.Unauthorized,
            403 to PlaybackFailureKind.Forbidden,
        ).forEach { (statusCode, expectedKind) ->
            val transport = FakeTicketTransport { ApiResult.HttpError(statusCode, "HTTP $statusCode") }
            val invalidations = mutableListOf<PlaybackFailure>()
            val controller = playbackController(
                scope = this,
                transport = transport,
                maxRetries = 1,
                onSessionInvalidated = { invalidations += it },
            )

            controller.prepare()
            advanceUntilIdle()

            assertEquals(2, transport.fetchCount)
            assertEquals(1, invalidations.size)
            assertEquals(expectedKind, invalidations.single().kind)
            assertTrue(controller.state.value is PlaybackPlayerState.SessionInvalidated)
        }
    }

    @Test
    @OptIn(ExperimentalCoroutinesApi::class)
    fun streamAuthFailureRetriesWithFreshTicketsThenInvalidatesAtLimit() = runTest {
        val transport = FakeTicketTransport { ApiResult.Success(PlaybackTicket("ticket-${it}", 60)) }
        val invalidations = mutableListOf<PlaybackFailure>()
        val controller = playbackController(
            scope = this,
            transport = transport,
            maxRetries = 2,
            onSessionInvalidated = { invalidations += it },
        )

        controller.prepare()
        advanceUntilIdle()
        assertTrue(controller.state.value is PlaybackPlayerState.Ready)
        assertEquals(1, transport.fetchCount)

        val forbidden = PlaybackFailureMapper.fromHttpStatus(403)
        controller.onPlayerError(forbidden, positionMs = 12_000L)
        advanceUntilIdle()
        assertTrue(controller.state.value is PlaybackPlayerState.Ready)
        assertEquals(2, transport.fetchCount)

        controller.onPlayerError(forbidden, positionMs = 13_000L)
        advanceUntilIdle()
        assertTrue(controller.state.value is PlaybackPlayerState.Ready)
        assertEquals(3, transport.fetchCount)

        controller.onPlayerError(forbidden, positionMs = 14_000L)
        advanceUntilIdle()
        assertEquals(1, invalidations.size)
        assertEquals(PlaybackFailureKind.Forbidden, invalidations.single().kind)
        assertTrue(controller.state.value is PlaybackPlayerState.SessionInvalidated)
    }

    @Test
    fun streamAuthStatusMappingMarks401And403AsSessionFailures() {
        val unauthorized = PlaybackFailureMapper.fromHttpStatus(401)
        val forbidden = PlaybackFailureMapper.fromHttpStatus(403)

        assertTrue(unauthorized.isAuthFailure)
        assertTrue(forbidden.isAuthFailure)
        assertTrue(unauthorized.autoRetryable)
        assertTrue(forbidden.autoRetryable)
    }

    @Test
    @OptIn(ExperimentalCoroutinesApi::class)
    fun transientNetworkFailureStopsAtRetryLimitWithoutInvalidatingSession() = runTest {
        val transport = FakeTicketTransport { ApiResult.Success(PlaybackTicket("ticket", 60)) }
        val invalidations = mutableListOf<PlaybackFailure>()
        val controller = playbackController(
            scope = this,
            transport = transport,
            maxRetries = 1,
            onSessionInvalidated = { invalidations += it },
        )

        controller.prepare()
        advanceUntilIdle()

        val network = PlaybackFailureMapper.network()
        controller.onPlayerError(network, positionMs = 5_000L)
        advanceUntilIdle()
        assertTrue(controller.state.value is PlaybackPlayerState.Ready)

        controller.onPlayerError(network, positionMs = 6_000L)
        advanceUntilIdle()

        assertTrue(controller.state.value is PlaybackPlayerState.Error)
        assertEquals(0, invalidations.size)
        assertEquals(2, transport.fetchCount)
    }

    @Test
    fun diagnosticsRedactPlaybackTicketsAndSessionTokens() {
        val raw = "GET http://ferrex.local/api/v1/stream/media?access_token=playback-ticket-secret&range=1 Authorization: Bearer full-session-secret {\"refresh_token\":\"refresh-secret\"}"

        val redacted = PlaybackDiagnosticLog.redact(raw)

        assertFalse(redacted.contains("playback-ticket-secret"))
        assertFalse(redacted.contains("full-session-secret"))
        assertFalse(redacted.contains("refresh-secret"))
        assertTrue(redacted.contains("access_token=<redacted>"))
        assertTrue(redacted.contains("Bearer <redacted>"))
    }

    private fun playbackController(
        scope: CoroutineScope,
        transport: PlaybackTicketTransport,
        maxRetries: Int,
        onSessionInvalidated: (PlaybackFailure) -> Unit,
    ): PlaybackController {
        val serverConfig = ServerConfig().apply { setUrl("https://ferrex.example") }
        return PlaybackController(
            route = PlaybackRouteContract(
                targetMediaId = "target-media-id",
                logicalMediaId = "logical-media-id",
                mediaType = BrowseMediaType.Movie,
                startPositionSeconds = null,
                startOver = true,
                sourceDetailRoute = "media/movie/logical-media-id",
            ),
            ticketTransport = transport,
            streamUrlFactory = PlaybackStreamUrlFactory(serverConfig),
            progressReporter = null,
            scope = scope,
            retryPolicy = PlaybackRetryPolicy(maxAutoRetries = maxRetries, backoffMillis = { 0L }),
            onSessionInvalidated = onSessionInvalidated,
        )
    }
}

private class FakeTicketTransport(
    private val response: (Int) -> ApiResult<PlaybackTicket>,
) : PlaybackTicketTransport {
    var fetchCount: Int = 0
        private set

    override suspend fun fetchTicket(mediaId: String): ApiResult<PlaybackTicket> {
        fetchCount += 1
        return response(fetchCount)
    }
}
