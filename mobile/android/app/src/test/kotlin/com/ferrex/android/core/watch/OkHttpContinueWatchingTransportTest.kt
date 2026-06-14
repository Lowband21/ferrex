package com.ferrex.android.core.watch

import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.browse.BrowseMediaType
import kotlinx.coroutines.test.runTest
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class OkHttpContinueWatchingTransportTest {
    @Test
    fun parsesContinueWatchingApiResponseActionTargetsAndRoutes() = runTest {
        MockWebServer().use { server ->
            server.enqueue(
                MockResponse().setBody(
                    """
                    {"status":"success","data":[{"media_id":"series-card-playback-id","media_type":"Series","card_media_id":"series-card-id","action_target":{"media_id":"episode-id","media_type":"Episode"},"action_hint":"next_episode","position":0.0,"duration":0.0,"last_watched":1704067200,"title":"Series","subtitle":"S01E02","poster_iid":"11111111-1111-1111-1111-111111111111"}]}
                    """.trimIndent(),
                ),
            )
            val transport = transport(server)

            val result = transport.fetchContinueWatching()

            assertTrue(result is ContinueWatchingResult.Success)
            val item = (result as ContinueWatchingResult.Success).value.single()
            assertEquals("series-card-id", item.cardMediaId)
            assertEquals("episode-id", item.actionTarget.mediaId)
            assertEquals("next_episode", item.actionHint)
            val card = ContinueWatchingMapper.toCard(item)
            assertEquals(BrowseMediaType.Episode, card.route.mediaType)
            assertEquals("episode-id", card.route.mediaId)
            assertEquals("/api/v1/watch/continue", server.takeRequest().path)
        }
    }

    @Test
    fun authFailuresRemainRetryableFailuresAfterAuthenticatorRecoveryIsExhausted() = runTest {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(401).setBody("unauthorized"))
            val transport = transport(server)

            val result = transport.fetchContinueWatching()

            assertTrue(result is ContinueWatchingResult.Failure)
            assertTrue((result as ContinueWatchingResult.Failure).message.isNotBlank())
        }
    }

    private fun transport(server: MockWebServer): OkHttpContinueWatchingTransport {
        val config = ServerConfig()
        config.setUrl(server.url("/").toString())
        return OkHttpContinueWatchingTransport(OkHttpClient(), config)
    }
}
