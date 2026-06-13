package com.ferrex.android.core.api

import okhttp3.Interceptor
import okhttp3.Response

class AuthInterceptor : Interceptor {
    @Volatile
    var accessToken: String? = null
        private set

    fun setAccessToken(token: String?) {
        accessToken = token?.takeIf { it.isNotBlank() }
    }

    fun clearAccessToken() {
        accessToken = null
    }

    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        if (request.header("Authorization") != null || shouldSkip(request.url.encodedPath)) {
            return chain.proceed(request)
        }

        val token = accessToken ?: return chain.proceed(request)
        val authenticatedRequest = request.newBuilder()
            .header("Authorization", "Bearer $token")
            .build()
        return chain.proceed(authenticatedRequest)
    }

    private fun shouldSkip(path: String): Boolean = PUBLIC_PATH_SUFFIXES.any { path.endsWith(it) }

    companion object {
        private val PUBLIC_PATH_SUFFIXES = setOf(
            "/api/v1/setup/status",
            "/api/v1/auth/device/login",
            "/api/v1/auth/device/users",
            "/api/v1/auth/device/pin/challenge",
            "/api/v1/auth/device/pin",
            "/api/v1/auth/refresh",
        )
    }
}
