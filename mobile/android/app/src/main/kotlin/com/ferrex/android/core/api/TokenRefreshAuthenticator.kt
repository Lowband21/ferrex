package com.ferrex.android.core.api

import com.google.flatbuffers.FlatBufferBuilder
import ferrex.auth.AuthToken
import ferrex.auth.RefreshRequest
import okhttp3.Authenticator
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.Route
import java.nio.ByteBuffer
import java.nio.ByteOrder
import javax.inject.Inject
import javax.inject.Singleton

/**
 * OkHttp Authenticator for automatic 401 → refresh → retry.
 *
 * When any request gets a 401, this authenticator attempts a token refresh
 * using the stored refresh token. If successful, it updates the stored
 * tokens and retries the original request with the new access token.
 *
 * Guard: only attempts one refresh per response chain to avoid infinite loops.
 */
@Singleton
class TokenRefreshAuthenticator @Inject constructor(
    private val serverConfig: ServerConfig,
    private val authInterceptor: AuthInterceptor,
) : Authenticator {

    /**
     * Callback set by AuthManager to persist refreshed tokens.
     * Using a callback avoids a circular dependency (AuthManager → OkHttpClient → Authenticator → AuthManager).
     */
    @Volatile
    var onTokenRefreshed: ((accessToken: String, refreshToken: String) -> Unit)? = null

    @Volatile
    var refreshTokenProvider: (() -> String?)? = null

    override fun authenticate(route: Route?, response: Response): Request? {
        // Don't retry if we've already tried refreshing in this chain
        if (response.request.header("X-Retry-With-Refresh") != null) {
            return null
        }

        // Don't try to refresh if it was the refresh endpoint itself that failed
        if (response.request.url.encodedPath.endsWith("/auth/refresh")) {
            return null
        }

        val currentRefreshToken = refreshTokenProvider?.invoke() ?: return null

        // Synchronize to prevent concurrent refresh attempts
        synchronized(this) {
            // Build FlatBuffer refresh request
            val builder = FlatBufferBuilder(256)
            val tokenOff = builder.createString(currentRefreshToken)
            RefreshRequest.startRefreshRequest(builder)
            RefreshRequest.addRefreshToken(builder, tokenOff)
            val root = RefreshRequest.endRefreshRequest(builder)
            builder.finish(root)

            // Use a bare OkHttpClient (no interceptors) to avoid recursion
            val refreshClient = OkHttpClient.Builder()
                .build()

            val refreshRequest = Request.Builder()
                .url("${serverConfig.serverUrl}/api/v1/auth/refresh")
                .addHeader("Accept", "application/x-flatbuffers")
                .post(
                    builder.sizedByteArray()
                        .toRequestBody("application/x-flatbuffers".toMediaType())
                )
                .build()

            return try {
                val refreshResponse = refreshClient.newCall(refreshRequest).execute()
                if (!refreshResponse.isSuccessful) return null

                val bytes = refreshResponse.body?.bytes() ?: return null
                val authToken = AuthToken.getRootAsAuthToken(
                    ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
                )

                val newAccessToken = authToken.accessToken
                val newRefreshToken = authToken.refreshToken

                // Update the interceptor and persist
                authInterceptor.accessToken = newAccessToken
                onTokenRefreshed?.invoke(newAccessToken, newRefreshToken)

                // Retry the original request with the new token
                response.request.newBuilder()
                    .header("Authorization", "Bearer $newAccessToken")
                    .header("X-Retry-With-Refresh", "true")
                    .build()
            } catch (_: Exception) {
                null
            }
        }
    }
}
