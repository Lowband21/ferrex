package com.ferrex.android.core.playback

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.AuthInterceptor
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.watch.WatchMediaProgress
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
    fun serverResumeProgressLoadsBeforeTicketedPlaybackWhenNoExplicitStartWasChosen() = runTest {
        val transport = FakeTicketTransport { ApiResult.Success(PlaybackTicket("ticket", 60)) }
        val resumeProvider = FakeResumeProgressProvider(
            ApiResult.Success(WatchMediaProgress("logical-media-id", positionSeconds = 42.5, durationSeconds = 500.0)),
        )
        val controller = playbackController(
            scope = this,
            transport = transport,
            maxRetries = 0,
            onSessionInvalidated = {},
            route = playbackRoute(startOver = false),
            resumeProgressProvider = resumeProvider,
        )

        controller.prepare()
        advanceUntilIdle()

        val ready = controller.state.value as PlaybackPlayerState.Ready
        assertEquals(42_500L, ready.prepared.startPositionMs)
        assertEquals(listOf("logical-media-id"), resumeProvider.requests)
        assertEquals(1, transport.fetchCount)
    }

    @Test
    @OptIn(ExperimentalCoroutinesApi::class)
    fun startOverAndExplicitStartPositionSkipServerResumeLookup() = runTest {
        val startOverProvider = FakeResumeProgressProvider(
            ApiResult.Success(WatchMediaProgress("logical-media-id", positionSeconds = 90.0, durationSeconds = 500.0)),
        )
        val startOverController = playbackController(
            scope = this,
            transport = FakeTicketTransport { ApiResult.Success(PlaybackTicket("ticket", 60)) },
            maxRetries = 0,
            onSessionInvalidated = {},
            route = playbackRoute(startOver = true),
            resumeProgressProvider = startOverProvider,
        )

        startOverController.prepare()
        advanceUntilIdle()

        val startOverReady = startOverController.state.value as PlaybackPlayerState.Ready
        assertEquals(0L, startOverReady.prepared.startPositionMs)
        assertTrue(startOverProvider.requests.isEmpty())

        val explicitProvider = FakeResumeProgressProvider(
            ApiResult.Success(WatchMediaProgress("logical-media-id", positionSeconds = 90.0, durationSeconds = 500.0)),
        )
        val explicitController = playbackController(
            scope = this,
            transport = FakeTicketTransport { ApiResult.Success(PlaybackTicket("ticket", 60)) },
            maxRetries = 0,
            onSessionInvalidated = {},
            route = playbackRoute(startPositionSeconds = 12.25, startOver = false),
            resumeProgressProvider = explicitProvider,
        )

        explicitController.prepare()
        advanceUntilIdle()

        val explicitReady = explicitController.state.value as PlaybackPlayerState.Ready
        assertEquals(12_250L, explicitReady.prepared.startPositionMs)
        assertTrue(explicitProvider.requests.isEmpty())
    }

    @Test
    @OptIn(ExperimentalCoroutinesApi::class)
    fun progressAuthFailureInvalidatesPlaybackSession() = runTest {
        val reporter = RecordingProgressReporter(ApiResult.HttpError(401, "expired"))
        val invalidations = mutableListOf<PlaybackFailure>()
        val controller = playbackController(
            scope = this,
            transport = FakeTicketTransport { ApiResult.Success(PlaybackTicket("ticket", 60)) },
            maxRetries = 0,
            onSessionInvalidated = { invalidations += it },
            progressReporter = reporter,
        )

        controller.reportProgress(positionMs = 10_000L, durationMs = 100_000L)
        advanceUntilIdle()

        assertEquals(1, reporter.calls.size)
        assertEquals(PlaybackFailureKind.Unauthorized, invalidations.single().kind)
        assertTrue(controller.state.value is PlaybackPlayerState.SessionInvalidated)
    }

    @Test
    @OptIn(ExperimentalCoroutinesApi::class)
    fun pauseExitAndEndProgressWritesCommitAndUseCompletionPosition() = runTest {
        val reporter = RecordingProgressReporter(ApiResult.Success(Unit))
        var committed = 0
        val controller = playbackController(
            scope = this,
            transport = FakeTicketTransport { ApiResult.Success(PlaybackTicket("ticket", 60)) },
            maxRetries = 0,
            onSessionInvalidated = {},
            progressReporter = reporter,
            onProgressCommitted = { committed += 1 },
        )

        controller.reportProgress(positionMs = 10_000L, durationMs = 100_000L)
        controller.onPlaybackExit(positionMs = 15_000L, durationMs = 100_000L)
        controller.onPlaybackEnded(durationMs = 100_000L)
        advanceUntilIdle()

        assertEquals(listOf(10.0, 15.0, 100.0), reporter.calls.map { it.positionSeconds })
        assertEquals(3, committed)
    }

    @Test
    fun progressReporterSerializesLogicalMediaProgressForWatchEndpoint() = runTest {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(204))
            server.start()
            val serverConfig = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val reporter = OkHttpPlaybackProgressReporter(OkHttpClient(), serverConfig)

            val result = reporter.reportProgress(
                route = PlaybackRouteContract(
                    targetMediaId = "target-media-id",
                    logicalMediaId = "logical-media-id",
                    mediaType = BrowseMediaType.Movie,
                    startPositionSeconds = null,
                    startOver = false,
                    sourceDetailRoute = "media/movie/logical-media-id",
                ),
                positionSeconds = 12.5,
                durationSeconds = 100.0,
            )

            assertTrue(result is ApiResult.Success)
            val request = server.takeRequest()
            assertEquals("/api/v1/watch/progress", request.path)
            assertEquals("POST", request.method)
            val body = request.body.readUtf8()
            assertTrue(body.contains("\"media_id\":\"logical-media-id\""))
            assertTrue(body.contains("\"media_type\":\"Movie\""))
            assertTrue(body.contains("\"position\":12.5"))
            assertTrue(body.contains("\"duration\":100.0"))
            assertTrue(body.contains("\"last_media_uuid\":\"target-media-id\""))
        }
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
        route: PlaybackRouteContract = playbackRoute(startOver = true),
        progressReporter: PlaybackProgressReporter? = null,
        resumeProgressProvider: PlaybackResumeProgressProvider? = null,
        onProgressCommitted: () -> Unit = {},
    ): PlaybackController {
        val serverConfig = ServerConfig().apply { setUrl("https://ferrex.example") }
        return PlaybackController(
            route = route,
            ticketTransport = transport,
            streamUrlFactory = PlaybackStreamUrlFactory(serverConfig),
            progressReporter = progressReporter,
            resumeProgressProvider = resumeProgressProvider,
            scope = scope,
            retryPolicy = PlaybackRetryPolicy(maxAutoRetries = maxRetries, backoffMillis = { 0L }),
            onSessionInvalidated = onSessionInvalidated,
            onProgressCommitted = onProgressCommitted,
        )
    }

    private fun playbackRoute(
        startPositionSeconds: Double? = null,
        startOver: Boolean,
    ): PlaybackRouteContract = PlaybackRouteContract(
        targetMediaId = "target-media-id",
        logicalMediaId = "logical-media-id",
        mediaType = BrowseMediaType.Movie,
        startPositionSeconds = startPositionSeconds,
        startOver = startOver,
        sourceDetailRoute = "media/movie/logical-media-id",
    )
}

private class FakeResumeProgressProvider(
    private val response: ApiResult<WatchMediaProgress?>,
) : PlaybackResumeProgressProvider {
    val requests = mutableListOf<String>()

    override suspend fun fetchResumeProgress(logicalMediaId: String): ApiResult<WatchMediaProgress?> {
        requests += logicalMediaId
        return response
    }
}

private data class ProgressCall(
    val route: PlaybackRouteContract,
    val positionSeconds: Double,
    val durationSeconds: Double,
)

private class RecordingProgressReporter(
    private val response: ApiResult<Unit>,
) : PlaybackProgressReporter {
    val calls = mutableListOf<ProgressCall>()

    override suspend fun reportProgress(
        route: PlaybackRouteContract,
        positionSeconds: Double,
        durationSeconds: Double,
    ): ApiResult<Unit> {
        calls += ProgressCall(route, positionSeconds, durationSeconds)
        return response
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
