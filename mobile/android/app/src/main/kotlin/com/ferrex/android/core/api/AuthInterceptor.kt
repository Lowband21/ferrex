package com.ferrex.android.core.api

import okhttp3.Interceptor
import okhttp3.Response
import javax.inject.Inject
import javax.inject.Singleton

/**
 * OkHttp interceptor that injects the current access token into every request.
 *
 * Skips injection for auth endpoints (login, register, refresh) since those
 * don't require / have tokens yet.
 */
@Singleton
class AuthInterceptor @Inject constructor() : Interceptor {

    @Volatile
    var accessToken: String? = null

    private val skipPaths = setOf(
        "/api/v1/auth/login",
        "/api/v1/auth/register",
        "/api/v1/auth/refresh",
        "/api/v1/setup/status",
    )

    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        val path = request.url.encodedPath

        // Don't add auth header to public endpoints
        if (skipPaths.any { path.endsWith(it) }) {
            return chain.proceed(request)
        }

        val token = accessToken
        if (token != null) {
            val authenticatedRequest = request.newBuilder()
                .addHeader("Authorization", "Bearer $token")
                .build()
            return chain.proceed(authenticatedRequest)
        }

        return chain.proceed(request)
    }
}
