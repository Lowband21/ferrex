package com.ferrex.android.core.search

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SearchTransportTest {
    @Test
    fun postsJsonMediaQueryBodyWithBoundedLimitAndNoFlatBuffersWatchState() = runTest {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(200).setBody("""{"status":"success","data":[]}"""))
            server.start()

            val config = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val transport = OkHttpMediaSearchTransport(OkHttpClient(), config)

            val result = transport.queryMedia("  alien  ", limit = 250)

            assertTrue(result is ApiResult.Success)
            val request = server.takeRequest()
            assertEquals("POST", request.method)
            assertEquals("/api/v1/media/query", request.path)
            assertEquals("application/json", request.getHeader("Accept"))
            assertTrue(request.getHeader("Content-Type")?.startsWith("application/json") == true)
            assertFalse(request.getHeader("Accept").orEmpty().contains("flatbuffers", ignoreCase = true))

            val bodyText = request.body.readUtf8()
            assertFalse(bodyText.contains("WatchState"))
            val body = Json.parseToJsonElement(bodyText).jsonObject
            assertEquals("alien", body.getValue("search").jsonObject.getValue("text").jsonPrimitive.content)
            assertEquals("all", body.getValue("search").jsonObject.getValue("fields").jsonArray.single().jsonPrimitive.content)
            assertEquals("true", body.getValue("search").jsonObject.getValue("fuzzy").jsonPrimitive.content)
            assertEquals("100", body.getValue("pagination").jsonObject.getValue("limit").jsonPrimitive.content)
            assertEquals("title", body.getValue("sort").jsonObject.getValue("primary").jsonPrimitive.content)
            assertTrue(body.getValue("filters").jsonObject.getValue("library_ids").jsonArray.isEmpty())
        }
    }

    @Test
    fun parsesCurrentJsonMediaIdEnumVariants() = runTest {
        val movie = "018f5f8d-0000-7000-8000-000000000001"
        val series = "018f5f8d-0000-7000-8000-000000000002"
        val season = "018f5f8d-0000-7000-8000-000000000003"
        val episode = "018f5f8d-0000-7000-8000-000000000004"
        MockWebServer().use { server ->
            server.enqueue(
                MockResponse().setResponseCode(200).setBody(
                    """
                    {
                      "status": "success",
                      "data": [
                        {"id": {"Movie": "$movie"}, "watch_status": {"Completed": {"completed_at": "now"}}},
                        {"id": {"Series": "$series"}},
                        {"id": {"Season": "$season"}},
                        {"id": {"episode": "$episode"}}
                      ]
                    }
                    """.trimIndent(),
                ),
            )
            server.start()

            val config = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val transport = OkHttpMediaSearchTransport(OkHttpClient(), config)

            val result = transport.queryMedia("space")

            assertTrue(result is ApiResult.Success)
            val hits = (result as ApiResult.Success).data
            assertEquals(SearchMediaId(SearchMediaType.Movie, movie), hits[0].id)
            assertEquals(SearchMediaId(SearchMediaType.Series, series), hits[1].id)
            assertEquals(SearchMediaId(SearchMediaType.Season, season), hits[2].id)
            assertEquals(SearchMediaId(SearchMediaType.Episode, episode), hits[3].id)
        }
    }

    @Test
    fun zeroLimitUsesDefaultSearchLimit() = runTest {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(200).setBody("""{"status":"success","data":[]}"""))
            server.start()

            val config = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val transport = OkHttpMediaSearchTransport(OkHttpClient(), config)

            transport.queryMedia("matrix", limit = 0)

            val body = Json.parseToJsonElement(server.takeRequest().body.readUtf8()).jsonObject
            assertEquals("50", body.getValue("pagination").jsonObject.getValue("limit").jsonPrimitive.content)
        }
    }
}
