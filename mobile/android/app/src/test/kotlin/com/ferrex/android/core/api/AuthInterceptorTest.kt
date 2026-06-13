package com.ferrex.android.core.api

import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AuthInterceptorTest {
    @Test
    fun injectsAndClearsBearerToken() {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(200).setBody("{}"))
            server.enqueue(MockResponse().setResponseCode(200).setBody("{}"))
            server.start()

            val interceptor = AuthInterceptor()
            val client = OkHttpClient.Builder().addInterceptor(interceptor).build()
            interceptor.setAccessToken("access-token")

            client.newCall(Request.Builder().url(server.url("/api/v1/users/me")).build()).execute().close()
            assertEquals("Bearer access-token", server.takeRequest().getHeader("Authorization"))

            interceptor.clearAccessToken()
            client.newCall(Request.Builder().url(server.url("/api/v1/users/me")).build()).execute().close()
            assertNull(server.takeRequest().getHeader("Authorization"))
        }
    }

    @Test
    fun skipsPublicDeviceAuthEndpoints() {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(200).setBody("{}"))
            server.start()

            val interceptor = AuthInterceptor()
            val client = OkHttpClient.Builder().addInterceptor(interceptor).build()
            interceptor.setAccessToken("access-token")

            client.newCall(Request.Builder().url(server.url("/api/v1/auth/device/login")).build()).execute().close()

            assertNull(server.takeRequest().getHeader("Authorization"))
        }
    }
}
