package com.ferrex.android.core.auth

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.AuthInterceptor
import com.ferrex.android.core.api.FerrexApiClient
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.api.TokenRefreshAuthenticator
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Manages authentication lifecycle: login, token refresh, session persistence.
 *
 * Exposes [sessionState] as a StateFlow for reactive UI observation.
 * The OkHttp [AuthInterceptor] is kept in sync — when tokens change,
 * the interceptor's reference is updated so all subsequent requests
 * carry the new token.
 */
@Singleton
class AuthManager @Inject constructor(
    private val apiClient: FerrexApiClient,
    private val storage: EncryptedStorage,
    private val serverConfig: ServerConfig,
    private val authInterceptor: AuthInterceptor,
    private val tokenRefreshAuthenticator: TokenRefreshAuthenticator,
) {
    private val _sessionState = MutableStateFlow<SessionState>(SessionState.Loading)
    val sessionState: StateFlow<SessionState> = _sessionState.asStateFlow()

    /**
     * Called once on app start. Restores server URL and attempts session
     * recovery from stored tokens.
     */
    suspend fun initialize() {
        // Wire up the authenticator callbacks
        tokenRefreshAuthenticator.refreshTokenProvider = { storage.refreshToken }
        tokenRefreshAuthenticator.onTokenRefreshed = { accessToken, refreshToken ->
            storage.storeTokens(accessToken, refreshToken, storage.username)
        }

        val savedUrl = storage.serverUrl
        if (savedUrl.isNullOrBlank()) {
            _sessionState.value = SessionState.NoServer
            return
        }

        serverConfig.setUrl(savedUrl)

        val savedAccessToken = storage.accessToken
        val savedRefreshToken = storage.refreshToken

        if (savedAccessToken == null || savedRefreshToken == null) {
            _sessionState.value = SessionState.NeedsLogin
            return
        }

        // Try using the stored access token
        authInterceptor.accessToken = savedAccessToken
        _sessionState.value = SessionState.Authenticated(
            accessToken = savedAccessToken,
            username = storage.username,
        )

        // TODO: Validate token freshness by calling /users/me in background.
        // If 401 → attempt refresh. If refresh fails → NeedsLogin.
    }

    /**
     * Connect to a new server URL. Validates by calling GET /setup/status.
     */
    suspend fun connectToServer(url: String): ConnectResult {
        serverConfig.setUrl(url)

        return when (val result = apiClient.getSetupStatus()) {
            is ApiResult.Success -> {
                storage.serverUrl = url
                _sessionState.value = SessionState.NeedsLogin
                ConnectResult.Success(
                    needsSetup = result.data.needsSetup,
                    registrationOpen = result.data.registrationOpen,
                )
            }
            is ApiResult.HttpError -> {
                serverConfig.setUrl("")
                ConnectResult.Error("Server returned ${result.code}: ${result.message}")
            }
            is ApiResult.NetworkError -> {
                serverConfig.setUrl("")
                ConnectResult.Error(
                    result.exception.localizedMessage ?: "Connection failed"
                )
            }
        }
    }

    /**
     * Log in with username and password.
     */
    suspend fun login(username: String, password: String): LoginResult {
        return when (val result = apiClient.login(username, password)) {
            is ApiResult.Success -> {
                val token = result.data
                storage.storeTokens(
                    accessToken = token.accessToken,
                    refreshToken = token.refreshToken,
                    username = username,
                )
                authInterceptor.accessToken = token.accessToken
                _sessionState.value = SessionState.Authenticated(
                    accessToken = token.accessToken,
                    username = username,
                )
                LoginResult.Success
            }
            is ApiResult.HttpError -> {
                if (result.code == 401) LoginResult.InvalidCredentials
                else LoginResult.Error("Server error: ${result.code}")
            }
            is ApiResult.NetworkError -> {
                LoginResult.Error(
                    result.exception.localizedMessage ?: "Network error"
                )
            }
        }
    }

    /**
     * Attempt to refresh the access token using the stored refresh token.
     */
    suspend fun refreshSession(): Boolean {
        val refreshToken = storage.refreshToken ?: return false

        return when (val result = apiClient.refreshToken(refreshToken)) {
            is ApiResult.Success -> {
                val token = result.data
                storage.storeTokens(
                    accessToken = token.accessToken,
                    refreshToken = token.refreshToken,
                    username = storage.username,
                )
                authInterceptor.accessToken = token.accessToken
                _sessionState.value = SessionState.Authenticated(
                    accessToken = token.accessToken,
                    username = storage.username,
                )
                true
            }
            else -> {
                logout()
                false
            }
        }
    }

    /**
     * Log out: clear stored tokens, reset state.
     */
    fun logout() {
        storage.clearSession()
        authInterceptor.accessToken = null
        _sessionState.value = SessionState.NeedsLogin
    }
}

sealed interface ConnectResult {
    data class Success(val needsSetup: Boolean, val registrationOpen: Boolean) : ConnectResult
    data class Error(val message: String) : ConnectResult
}

sealed interface LoginResult {
    data object Success : LoginResult
    data object InvalidCredentials : LoginResult
    data class Error(val message: String) : LoginResult
}
