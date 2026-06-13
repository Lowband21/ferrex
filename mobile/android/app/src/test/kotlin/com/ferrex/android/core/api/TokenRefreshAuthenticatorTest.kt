package com.ferrex.android.core.api

import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TokenRefreshAuthenticatorTest {
    @Test
    fun retryGuardAvoidsInfiniteRefreshLoopAndInvalidatesSession() {
        val interceptor = AuthInterceptor()
        interceptor.setAccessToken("old-access")
        val authenticator = TokenRefreshAuthenticator(ServerConfig(), interceptor)
        val invalidations = mutableListOf<RefreshInvalidationReason>()
        authenticator.refreshTokenProvider = { error("provider should not be called") }
        authenticator.onSessionInvalidated = { invalidations += it }

        val request = Request.Builder()
            .url("http://ferrex.local/api/v1/users/me")
            .header(TokenRefreshAuthenticator.RETRY_HEADER, "true")
            .build()

        assertNull(authenticator.authenticate(null, unauthorizedResponse(request)))
        assertEquals(listOf(RefreshInvalidationReason.RetriedRequestRejected), invalidations)
        assertNull(interceptor.accessToken)
    }

    @Test
    fun missingRefreshTokenInvalidatesWithoutRetry() {
        val interceptor = AuthInterceptor()
        interceptor.setAccessToken("old-access")
        val authenticator = TokenRefreshAuthenticator(ServerConfig(), interceptor)
        val invalidations = mutableListOf<RefreshInvalidationReason>()
        authenticator.refreshTokenProvider = { null }
        authenticator.onSessionInvalidated = { invalidations += it }

        val request = authorizedUserRequest("old-access")

        assertNull(authenticator.authenticate(null, unauthorizedResponse(request)))
        assertEquals(listOf(RefreshInvalidationReason.MissingRefreshToken), invalidations)
        assertNull(interceptor.accessToken)
    }

    @Test
    fun refreshHttpFailureInvalidatesWithoutRetry() {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(401).setBody("revoked"))
            server.start()

            val interceptor = AuthInterceptor()
            interceptor.setAccessToken("old-access")
            val config = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val authenticator = TokenRefreshAuthenticator(config, interceptor)
            val invalidations = mutableListOf<RefreshInvalidationReason>()
            authenticator.refreshTokenProvider = { "refresh" }
            authenticator.onSessionInvalidated = { invalidations += it }

            assertNull(authenticator.authenticate(null, unauthorizedResponse(authorizedUserRequest("old-access"))))

            assertEquals("/api/v1/auth/refresh", server.takeRequest().path)
            assertEquals(listOf(RefreshInvalidationReason.RefreshRejected), invalidations)
            assertNull(interceptor.accessToken)
        }
    }

    @Test
    fun emptyRefreshResponseInvalidatesWithoutRetry() {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(200).setBody(""))
            server.start()

            val interceptor = AuthInterceptor()
            interceptor.setAccessToken("old-access")
            val config = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val authenticator = TokenRefreshAuthenticator(config, interceptor)
            val invalidations = mutableListOf<RefreshInvalidationReason>()
            authenticator.refreshTokenProvider = { "refresh" }
            authenticator.onSessionInvalidated = { invalidations += it }

            assertNull(authenticator.authenticate(null, unauthorizedResponse(authorizedUserRequest("old-access"))))

            assertEquals(listOf(RefreshInvalidationReason.InvalidRefreshResponse), invalidations)
            assertNull(interceptor.accessToken)
        }
    }

    @Test
    fun invalidRefreshResponseInvalidatesWithoutRetry() {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setResponseCode(200).setBody("not-json"))
            server.start()

            val interceptor = AuthInterceptor()
            interceptor.setAccessToken("old-access")
            val config = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val authenticator = TokenRefreshAuthenticator(config, interceptor)
            val invalidations = mutableListOf<RefreshInvalidationReason>()
            authenticator.refreshTokenProvider = { "refresh" }
            authenticator.onSessionInvalidated = { invalidations += it }

            assertNull(authenticator.authenticate(null, unauthorizedResponse(authorizedUserRequest("old-access"))))

            assertEquals(listOf(RefreshInvalidationReason.InvalidRefreshResponse), invalidations)
            assertNull(interceptor.accessToken)
        }
    }

    @Test
    fun successfulRefreshReturnsRetriedRequestWithNewBearerAndPersistsTokens() {
        MockWebServer().use { server ->
            server.enqueue(
                MockResponse()
                    .setResponseCode(200)
                    .setBody(
                        """
                        {"status":"success","data":{"access_token":"new-access","refresh_token":"rotated-refresh","expires_in":3600}}
                        """.trimIndent(),
                    ),
            )
            server.start()

            val interceptor = AuthInterceptor()
            interceptor.setAccessToken("old-access")
            val config = ServerConfig().apply { setUrl(server.url("/").toString()) }
            val authenticator = TokenRefreshAuthenticator(config, interceptor)
            var persisted: AuthTokens? = null
            val invalidations = mutableListOf<RefreshInvalidationReason>()
            authenticator.refreshTokenProvider = { "refresh" }
            authenticator.onTokenRefreshed = { persisted = it }
            authenticator.onSessionInvalidated = { invalidations += it }

            val retry = authenticator.authenticate(null, unauthorizedResponse(authorizedUserRequest("old-access")))

            assertEquals("Bearer new-access", retry?.header("Authorization"))
            assertEquals("true", retry?.header(TokenRefreshAuthenticator.RETRY_HEADER))
            assertEquals("new-access", interceptor.accessToken)
            assertEquals("rotated-refresh", persisted?.refreshToken)
            assertTrue(invalidations.isEmpty())
            assertEquals("/api/v1/auth/refresh", server.takeRequest().path)
        }
    }

    private fun authorizedUserRequest(token: String): Request = Request.Builder()
        .url("http://ferrex.local/api/v1/users/me")
        .header("Authorization", "Bearer $token")
        .build()

    private fun unauthorizedResponse(request: Request): Response = Response.Builder()
        .request(request)
        .protocol(Protocol.HTTP_1_1)
        .code(401)
        .message("Unauthorized")
        .build()
}
