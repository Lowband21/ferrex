package com.ferrex.android.core.api

import okhttp3.OkHttpClient
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class FerrexApiClientTest {

    @Test
    fun `server config trims trailing slashes`() {
        val config = ServerConfig()

        config.setUrl("https://ferrex.example.test///")

        assertTrue(config.isConfigured)
        assertEquals("https://ferrex.example.test", config.serverUrl)
    }

    @Test
    fun `server config treats empty url as unconfigured`() {
        val config = ServerConfig()

        config.setUrl("")

        assertFalse(config.isConfigured)
        assertEquals("", config.serverUrl)
    }

    @Test
    fun `image helper urls append API routes to normalized server url`() {
        val config = ServerConfig().apply { setUrl("https://ferrex.example.test/") }
        val client = FerrexApiClient(OkHttpClient(), config)

        assertEquals(
            "https://ferrex.example.test/api/v1/images/blob/sha256-deadbeef",
            client.imageBlobUrl("sha256-deadbeef"),
        )
        assertEquals(
            "https://ferrex.example.test/api/v1/images/iid/00112233-4455-6677-8899-aabbccddeeff",
            client.imageIidUrl("00112233-4455-6677-8899-aabbccddeeff"),
        )
    }
}
