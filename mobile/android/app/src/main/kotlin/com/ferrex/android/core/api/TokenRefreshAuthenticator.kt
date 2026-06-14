package com.ferrex.android.core.api

import kotlinx.serialization.SerializationException
import okhttp3.Authenticator
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.Route
import java.io.IOException

class TokenRefreshAuthenticator(
    private val serverConfig: ServerConfig,
    private val authInterceptor: AuthInterceptor,
    private val refreshClient: OkHttpClient = OkHttpClient.Builder().build(),
) : Authenticator {
    @Volatile
    var refreshTokenProvider: (() -> String?)? = null

    @Volatile
    var onTokenRefreshed: ((AuthTokens) -> Unit)? = null

    @Volatile
    var onSessionInvalidated: ((RefreshInvalidationReason) -> Unit)? = null

    @Volatile
    var onRefreshTemporarilyUnavailable: (() -> Unit)? = null

    private val lock = Any()
    private val json = FerrexApiClient.DefaultJson

    override fun authenticate(route: Route?, response: Response): Request? {
        val path = response.request.url.encodedPath
        if (path.endsWith(FerrexApiClient.Routes.REFRESH)) {
            return null
        }
        if (path.endsWith(FerrexApiClient.Routes.DEVICE_LOGIN) || path.endsWith(FerrexApiClient.Routes.PIN_LOGIN)) {
            return null
        }
        if (response.request.header(RETRY_HEADER) != null) {
            invalidate(RefreshInvalidationReason.RetriedRequestRejected)
            return null
        }

        retryWithCurrentTokenIfRotated(response)?.let { return it }

        val refreshToken = refreshTokenProvider?.invoke()
        if (refreshToken.isNullOrBlank()) {
            invalidate(RefreshInvalidationReason.MissingRefreshToken)
            return null
        }

        synchronized(lock) {
            retryWithCurrentTokenIfRotated(response)?.let { return it }

            val refreshedTokens = requestRefresh(refreshToken) ?: return null
            authInterceptor.setAccessToken(refreshedTokens.accessToken)
            onTokenRefreshed?.invoke(refreshedTokens)
            return response.request.newBuilder()
                .header("Authorization", "Bearer ${refreshedTokens.accessToken}")
                .header(RETRY_HEADER, "true")
                .build()
        }
    }

    private fun retryWithCurrentTokenIfRotated(response: Response): Request? {
        val requestToken = response.request.header("Authorization")?.removePrefix("Bearer ")
        val currentToken = authInterceptor.accessToken
        if (!currentToken.isNullOrBlank() && requestToken != null && requestToken != currentToken) {
            return response.request.newBuilder()
                .header("Authorization", "Bearer $currentToken")
                .header(RETRY_HEADER, "true")
                .build()
        }
        return null
    }

    private fun requestRefresh(refreshToken: String): AuthTokens? {
        val baseUrl = serverConfig.serverUrl
        if (baseUrl.isBlank()) {
            invalidate(RefreshInvalidationReason.MissingServerUrl)
            return null
        }

        val requestBody = json.encodeToString(RefreshRequest.serializer(), RefreshRequest(refreshToken))
            .toRequestBody(JSON_MEDIA_TYPE)
        val refreshRequest = Request.Builder()
            .url("${ServerConfig.normalize(baseUrl)}${FerrexApiClient.Routes.REFRESH}")
            .header("Accept", JSON_MEDIA_TYPE.toString())
            .post(requestBody)
            .build()

        return try {
            refreshClient.newCall(refreshRequest).execute().use { refreshResponse ->
                if (!refreshResponse.isSuccessful) {
                    when {
                        refreshResponse.code == 401 || refreshResponse.code == 403 -> {
                            invalidate(RefreshInvalidationReason.RefreshRejected)
                        }
                        refreshResponse.code.isTemporaryRefreshFailure() -> markTemporaryRefreshUnavailable()
                        else -> invalidate(RefreshInvalidationReason.RefreshRejected)
                    }
                    return null
                }

                val body = refreshResponse.body?.string()
                if (body.isNullOrBlank()) {
                    invalidate(RefreshInvalidationReason.InvalidRefreshResponse)
                    return null
                }

                val envelope = try {
                    json.decodeFromString(ApiEnvelope.serializer(AuthTokens.serializer()), body)
                } catch (e: SerializationException) {
                    invalidate(RefreshInvalidationReason.InvalidRefreshResponse)
                    return null
                } catch (e: IllegalArgumentException) {
                    invalidate(RefreshInvalidationReason.InvalidRefreshResponse)
                    return null
                }

                val tokens = envelope.data
                if (envelope.status != null && envelope.status != "success") {
                    invalidate(RefreshInvalidationReason.RefreshRejected)
                    return null
                }
                if (tokens == null || tokens.accessToken.isBlank() || tokens.refreshToken.isBlank()) {
                    invalidate(RefreshInvalidationReason.InvalidRefreshResponse)
                    return null
                }
                tokens
            }
        } catch (e: IOException) {
            markTemporaryRefreshUnavailable()
            null
        } catch (e: IllegalArgumentException) {
            markTemporaryRefreshUnavailable()
            null
        }
    }

    private fun markTemporaryRefreshUnavailable() {
        onRefreshTemporarilyUnavailable?.invoke()
    }

    private fun invalidate(reason: RefreshInvalidationReason) {
        authInterceptor.clearAccessToken()
        onSessionInvalidated?.invoke(reason)
    }

    private fun Int.isTemporaryRefreshFailure(): Boolean = this == 408 || this == 429 || this >= 500

    companion object {
        const val RETRY_HEADER = "X-Ferrex-Retry-With-Refresh"
        private val JSON_MEDIA_TYPE = "application/json; charset=utf-8".toMediaType()
    }
}

enum class RefreshInvalidationReason {
    MissingRefreshToken,
    MissingServerUrl,
    RefreshRejected,
    RefreshFailed,
    InvalidRefreshResponse,
    RetriedRequestRejected,
}
