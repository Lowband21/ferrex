package com.ferrex.android.core.auth

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.AuthInterceptor
import com.ferrex.android.core.api.AuthTokens
import com.ferrex.android.core.api.CurrentUser
import com.ferrex.android.core.api.DeviceInfo
import com.ferrex.android.core.api.FerrexApi
import com.ferrex.android.core.api.RefreshInvalidationReason
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.api.TokenRefreshAuthenticator
import com.ferrex.android.core.api.messageOrFallback
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.UUID

class AuthManager(
    private val api: FerrexApi,
    private val storage: AuthStorage,
    private val serverConfig: ServerConfig,
    private val authInterceptor: AuthInterceptor,
    private val tokenRefreshAuthenticator: TokenRefreshAuthenticator,
    private val deviceName: String,
    private val appVersion: String,
    private val onResetConnectionCacheClear: (serverUrl: String, userId: String?) -> Unit = { _, _ -> },
) {
    private val _sessionState = MutableStateFlow<SessionState>(SessionState.Loading)
    val sessionState: StateFlow<SessionState> = _sessionState.asStateFlow()

    init {
        configureTokenRefreshCallbacks()
    }

    suspend fun initialize() {
        configureTokenRefreshCallbacks()
        _sessionState.value = SessionState.Loading

        val savedServerUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (savedServerUrl.isBlank()) {
            serverConfig.clear()
            authInterceptor.clearAccessToken()
            _sessionState.value = SessionState.NoServer()
            return
        }

        serverConfig.setUrl(savedServerUrl)
        validateSavedSession(savedServerUrl)
    }

    suspend fun retryRestoredSession() {
        initialize()
    }

    suspend fun connectToServer(url: String): ConnectResult {
        val normalizedUrl = ServerConfig.normalize(url)
        if (normalizedUrl.isBlank()) {
            return ConnectResult.Error("Enter a Ferrex server URL.")
        }

        val previousUrl = storage.serverUrl
        val stateBeforeConnect = _sessionState.value
        return when (val result = api.getSetupStatus(normalizedUrl)) {
            is ApiResult.Success -> {
                storage.serverUrl = normalizedUrl
                serverConfig.setUrl(normalizedUrl)
                storage.clearTokens()
                authInterceptor.clearAccessToken()
                val status = result.data
                val reason = when {
                    status.needsSetup -> LoginRequiredReason.SetupRequired
                    !status.hasAdmin -> LoginRequiredReason.RegistrationClosed
                    previousUrl != null && ServerConfig.normalize(previousUrl) != normalizedUrl -> LoginRequiredReason.ChangedServer
                    else -> LoginRequiredReason.NoSavedSession
                }
                _sessionState.value = SessionState.NeedsLogin(
                    serverUrl = normalizedUrl,
                    reason = reason,
                    setupStatus = status,
                )
                ConnectResult.Success(status)
            }
            is ApiResult.HttpError,
            is ApiResult.ServerError,
            ApiResult.EmptyBody,
            is ApiResult.ParseError,
            is ApiResult.NetworkError -> {
                previousUrl?.let(serverConfig::setUrl) ?: serverConfig.clear()
                _sessionState.value = if (stateBeforeConnect is SessionState.NoServer) {
                    stateBeforeConnect.copy(
                        previousServerUrl = stateBeforeConnect.previousServerUrl ?: previousUrl ?: normalizedUrl,
                    )
                } else if (previousUrl.isNullOrBlank()) {
                    SessionState.NoServer(previousServerUrl = normalizedUrl)
                } else {
                    SessionState.RecoverableFailure(
                        serverUrl = previousUrl,
                        reason = RecoverableFailureReason.ServerUnreachable,
                    )
                }
                ConnectResult.Error(result.messageOrFallback("Could not reach that Ferrex server."))
            }
        }
    }

    suspend fun loginWithPassword(username: String, password: String): LoginResult {
        val serverUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (serverUrl.isBlank()) {
            _sessionState.value = SessionState.NoServer()
            return LoginResult.Error("Connect to a Ferrex server before signing in.")
        }
        if (username.isBlank() || password.isBlank()) {
            return LoginResult.Error("Enter a username and password.")
        }

        serverConfig.setUrl(serverUrl)
        return when (
            val result = api.devicePasswordLogin(
                username = username.trim(),
                password = password,
                deviceInfo = currentDeviceInfo(),
                rememberDevice = false,
            )
        ) {
            is ApiResult.Success -> {
                val tokens = result.data
                storage.storeTokens(tokens, username.trim(), tokens.userId)
                authInterceptor.setAccessToken(tokens.accessToken)
                when (val userResult = api.currentUser()) {
                    is ApiResult.Success -> {
                        persistValidatedUser(userResult.data, tokens)
                        LoginResult.Success(requiresPinSetup = tokens.requiresPinSetup)
                    }
                    is ApiResult.HttpError -> {
                        if (userResult.code == 401 || userResult.code == 403) {
                            invalidateLocalSession(LoginRequiredReason.SessionExpired)
                        } else {
                            recoverable(serverUrl, RecoverableFailureReason.ValidationUnavailable)
                        }
                        LoginResult.Error(userResult.messageOrFallback("Sign-in validation failed."))
                    }
                    ApiResult.EmptyBody,
                    is ApiResult.ParseError,
                    is ApiResult.ServerError,
                    is ApiResult.NetworkError -> {
                        recoverable(serverUrl, RecoverableFailureReason.ValidationUnavailable)
                        LoginResult.Error(userResult.messageOrFallback("Sign-in validation failed."))
                    }
                }
            }
            is ApiResult.HttpError -> {
                val message = when (result.code) {
                    401 -> "The username or password was not accepted."
                    403 -> "This device is not eligible for authentication on that server."
                    404 -> "That account is not available on this server."
                    else -> result.messageOrFallback("Sign in failed.")
                }
                LoginResult.Error(message)
            }
            is ApiResult.ServerError,
            ApiResult.EmptyBody,
            is ApiResult.ParseError,
            is ApiResult.NetworkError -> LoginResult.Error(result.messageOrFallback("Sign in failed."))
        }
    }

    fun signOut() {
        storage.clearTokens()
        authInterceptor.clearAccessToken()
        val serverUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (serverUrl.isBlank()) {
            serverConfig.clear()
            _sessionState.value = SessionState.NoServer()
        } else {
            serverConfig.setUrl(serverUrl)
            _sessionState.value = SessionState.NeedsLogin(serverUrl, LoginRequiredReason.SignedOut)
        }
    }

    fun changeServer() {
        storage.clearTokens()
        authInterceptor.clearAccessToken()
        val previousServer = storage.serverUrl?.let(ServerConfig::normalize)
        _sessionState.value = SessionState.NoServer(
            reason = NoServerReason.ChangeServer,
            previousServerUrl = previousServer,
        )
    }

    fun resetConnection() {
        val previousServerUrl = storage.serverUrl?.let(ServerConfig::normalize)
        val previousUserId = storage.userId
        storage.clearConnectionData()
        serverConfig.clear()
        authInterceptor.clearAccessToken()
        if (!previousServerUrl.isNullOrBlank()) {
            onResetConnectionCacheClear(previousServerUrl, previousUserId)
        }
        _sessionState.value = SessionState.NoServer(NoServerReason.ResetConnection)
    }

    fun invalidateSessionFromPlayback() {
        invalidateLocalSession(LoginRequiredReason.SessionRevoked)
    }

    private suspend fun validateSavedSession(serverUrl: String) {
        val savedAccessToken = storage.accessToken
        val savedRefreshToken = storage.refreshToken
        if (savedAccessToken.isNullOrBlank()) {
            authInterceptor.clearAccessToken()
            _sessionState.value = SessionState.NeedsLogin(serverUrl, LoginRequiredReason.NoSavedSession)
            return
        }

        authInterceptor.setAccessToken(savedAccessToken)
        when (val currentUser = api.currentUser()) {
            is ApiResult.Success -> {
                val tokens = AuthTokens(
                    accessToken = savedAccessToken,
                    refreshToken = savedRefreshToken.orEmpty(),
                    userId = currentUser.data.id,
                    requiresPinSetup = storage.requiresPinSetup,
                )
                persistValidatedUser(currentUser.data, tokens)
            }
            is ApiResult.HttpError -> {
                if (currentUser.code == 401 || currentUser.code == 403) {
                    refreshAndValidate(serverUrl, savedRefreshToken)
                } else {
                    recoverable(serverUrl, RecoverableFailureReason.ValidationUnavailable)
                }
            }
            ApiResult.EmptyBody,
            is ApiResult.ParseError,
            is ApiResult.ServerError -> recoverable(serverUrl, RecoverableFailureReason.InvalidServerResponse)
            is ApiResult.NetworkError -> recoverable(serverUrl, RecoverableFailureReason.ServerUnreachable)
        }
    }

    private suspend fun refreshAndValidate(serverUrl: String, refreshToken: String?) {
        if (refreshToken.isNullOrBlank()) {
            invalidateLocalSession(LoginRequiredReason.SessionExpired)
            return
        }

        when (val refresh = api.refreshToken(refreshToken)) {
            is ApiResult.Success -> {
                val tokens = refresh.data.copy(
                    requiresPinSetup = refresh.data.requiresPinSetup || storage.requiresPinSetup,
                )
                storage.storeTokens(tokens, storage.username, tokens.userId ?: storage.userId)
                authInterceptor.setAccessToken(tokens.accessToken)
                when (val currentUser = api.currentUser()) {
                    is ApiResult.Success -> persistValidatedUser(currentUser.data, tokens)
                    is ApiResult.HttpError -> {
                        if (currentUser.code == 401 || currentUser.code == 403) {
                            invalidateLocalSession(LoginRequiredReason.SessionRevoked)
                        } else {
                            recoverable(serverUrl, RecoverableFailureReason.ValidationUnavailable)
                        }
                    }
                    ApiResult.EmptyBody,
                    is ApiResult.ParseError,
                    is ApiResult.ServerError -> recoverable(serverUrl, RecoverableFailureReason.InvalidServerResponse)
                    is ApiResult.NetworkError -> recoverable(serverUrl, RecoverableFailureReason.ValidationUnavailable)
                }
            }
            is ApiResult.HttpError -> {
                if (refresh.code == 401 || refresh.code == 403) {
                    invalidateLocalSession(LoginRequiredReason.SessionRevoked)
                } else {
                    recoverable(serverUrl, RecoverableFailureReason.RefreshUnavailable)
                }
            }
            ApiResult.EmptyBody,
            is ApiResult.ParseError,
            is ApiResult.ServerError -> invalidateLocalSession(LoginRequiredReason.RefreshFailed)
            is ApiResult.NetworkError -> recoverable(serverUrl, RecoverableFailureReason.RefreshUnavailable)
        }
    }

    private fun persistValidatedUser(user: CurrentUser, tokens: AuthTokens) {
        storage.username = user.username
        storage.userId = user.id
        if (tokens.accessToken.isNotBlank()) storage.accessToken = tokens.accessToken
        if (tokens.refreshToken.isNotBlank()) storage.refreshToken = tokens.refreshToken
        storage.sessionId = tokens.sessionId ?: storage.sessionId
        storage.deviceSessionId = tokens.deviceSessionId ?: storage.deviceSessionId
        storage.requiresPinSetup = tokens.requiresPinSetup || storage.requiresPinSetup
        authInterceptor.setAccessToken(storage.accessToken)
        _sessionState.value = SessionState.Authenticated(
            serverUrl = serverConfig.requireUrl(),
            user = user,
            requiresPinSetup = storage.requiresPinSetup,
        )
    }

    private fun invalidateLocalSession(reason: LoginRequiredReason) {
        storage.clearTokens()
        authInterceptor.clearAccessToken()
        val serverUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (serverUrl.isBlank()) {
            serverConfig.clear()
            _sessionState.value = SessionState.NoServer()
        } else {
            serverConfig.setUrl(serverUrl)
            _sessionState.value = SessionState.NeedsLogin(serverUrl, reason)
        }
    }

    private fun recoverable(serverUrl: String, reason: RecoverableFailureReason) {
        authInterceptor.clearAccessToken()
        serverConfig.setUrl(serverUrl)
        _sessionState.value = SessionState.RecoverableFailure(serverUrl, reason)
    }

    private fun currentDeviceInfo(): DeviceInfo {
        val localDeviceId = storage.localDeviceId?.takeIf { it.isNotBlank() }
            ?: UUID.randomUUID().toString().also { storage.localDeviceId = it }
        return DeviceInfo(
            deviceId = localDeviceId,
            deviceName = deviceName,
            appVersion = appVersion,
        )
    }

    private fun configureTokenRefreshCallbacks() {
        tokenRefreshAuthenticator.refreshTokenProvider = { storage.refreshToken }
        tokenRefreshAuthenticator.onTokenRefreshed = { tokens ->
            val mergedTokens = tokens.copy(
                requiresPinSetup = tokens.requiresPinSetup || storage.requiresPinSetup,
            )
            storage.storeTokens(mergedTokens, storage.username, mergedTokens.userId ?: storage.userId)
            authInterceptor.setAccessToken(mergedTokens.accessToken)
        }
        tokenRefreshAuthenticator.onSessionInvalidated = { reason ->
            when (reason) {
                RefreshInvalidationReason.MissingRefreshToken,
                RefreshInvalidationReason.MissingServerUrl,
                RefreshInvalidationReason.RefreshRejected,
                RefreshInvalidationReason.InvalidRefreshResponse,
                RefreshInvalidationReason.RetriedRequestRejected -> invalidateLocalSession(LoginRequiredReason.SessionRevoked)
                RefreshInvalidationReason.RefreshFailed -> {
                    val serverUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
                    if (serverUrl.isBlank()) {
                        invalidateLocalSession(LoginRequiredReason.RefreshFailed)
                    } else {
                        recoverable(serverUrl, RecoverableFailureReason.RefreshUnavailable)
                    }
                }
            }
        }
    }
}

sealed interface ConnectResult {
    data class Success(val setupStatus: com.ferrex.android.core.api.SetupStatus) : ConnectResult
    data class Error(val message: String) : ConnectResult
}

sealed interface LoginResult {
    data class Success(val requiresPinSetup: Boolean) : LoginResult
    data class Error(val message: String) : LoginResult
}
