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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.UUID

private val UUID_REGEX = Regex(
    "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
)

private enum class AuthValidationOutcome {
    Online,
    TemporaryOffline,
    NeedsLogin,
    RecoverableFailure,
}

class AuthManager(
    private val api: FerrexApi,
    private val storage: AuthStorage,
    private val serverConfig: ServerConfig,
    private val authInterceptor: AuthInterceptor,
    private val tokenRefreshAuthenticator: TokenRefreshAuthenticator,
    private val deviceName: String,
    private val appVersion: String,
    private val onResetConnectionCacheClear: (serverUrl: String, userId: String?) -> Unit = { _, _ -> },
    reconnectScope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
    reconnectBackoffDelaysMillis: List<Long> = AuthReconnectCoordinator.DEFAULT_BACKOFF_DELAYS_MILLIS,
) {
    private val _sessionState = MutableStateFlow<SessionState>(SessionState.Loading)
    val sessionState: StateFlow<SessionState> = _sessionState.asStateFlow()

    private val reconnectCoordinator = AuthReconnectCoordinator(
        scope = reconnectScope,
        backoffDelaysMillis = reconnectBackoffDelaysMillis,
        attemptReconnect = ::attemptReconnect,
    )

    init {
        configureTokenRefreshCallbacks()
    }

    suspend fun initialize() {
        configureTokenRefreshCallbacks()
        reconnectCoordinator.cancel()
        _sessionState.value = SessionState.Loading

        val savedServerUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (savedServerUrl.isBlank()) {
            serverConfig.clear()
            authInterceptor.clearAccessToken()
            _sessionState.value = SessionState.NoServer()
            return
        }

        serverConfig.setUrl(savedServerUrl)
        validateSavedSession(savedServerUrl, scheduleReconnect = true)
    }

    suspend fun retryRestoredSession() {
        val state = _sessionState.value
        if (state is SessionState.Authenticated && state.connectionHealth != AuthConnectionHealth.Online) {
            retryAuthenticatedConnection()
        } else {
            initialize()
        }
    }

    fun retryAuthenticatedConnection() {
        val state = _sessionState.value
        if (state is SessionState.Authenticated && state.connectionHealth != AuthConnectionHealth.Online) {
            reconnectCoordinator.retryNow()
        }
    }

    fun notifyConnectivityAvailable() {
        val state = _sessionState.value
        if (state is SessionState.Authenticated && state.connectionHealth != AuthConnectionHealth.Online) {
            reconnectCoordinator.notifyConnectivityAvailable()
        }
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
                reconnectCoordinator.cancel()
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
        reconnectCoordinator.cancel()
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
        reconnectCoordinator.cancel()
        storage.clearTokens()
        authInterceptor.clearAccessToken()
        val previousServer = storage.serverUrl?.let(ServerConfig::normalize)
        _sessionState.value = SessionState.NoServer(
            reason = NoServerReason.ChangeServer,
            previousServerUrl = previousServer,
        )
    }

    fun resetConnection() {
        reconnectCoordinator.cancel()
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

    private suspend fun validateSavedSession(
        serverUrl: String,
        scheduleReconnect: Boolean,
    ): AuthValidationOutcome {
        val savedAccessToken = storage.accessToken
        val savedRefreshToken = storage.refreshToken
        if (savedAccessToken.isNullOrBlank()) {
            authInterceptor.clearAccessToken()
            _sessionState.value = SessionState.NeedsLogin(serverUrl, LoginRequiredReason.NoSavedSession)
            return AuthValidationOutcome.NeedsLogin
        }

        authInterceptor.setAccessToken(savedAccessToken)
        return when (val currentUser = api.currentUser()) {
            is ApiResult.Success -> {
                val tokens = AuthTokens(
                    accessToken = savedAccessToken,
                    refreshToken = savedRefreshToken.orEmpty(),
                    userId = currentUser.data.id,
                    requiresPinSetup = storage.requiresPinSetup,
                )
                persistValidatedUser(currentUser.data, tokens)
                AuthValidationOutcome.Online
            }
            is ApiResult.HttpError -> when {
                currentUser.code == 401 || currentUser.code == 403 -> refreshAndValidate(
                    serverUrl,
                    savedRefreshToken,
                    scheduleReconnect,
                )
                currentUser.code.isTemporaryHttpError() -> authenticatedOfflineOrRecoverable(
                    serverUrl,
                    RecoverableFailureReason.ValidationUnavailable,
                    scheduleReconnect,
                )
                else -> {
                    recoverable(serverUrl, RecoverableFailureReason.ValidationUnavailable)
                    AuthValidationOutcome.RecoverableFailure
                }
            }
            ApiResult.EmptyBody,
            is ApiResult.ParseError,
            is ApiResult.ServerError -> {
                recoverable(serverUrl, RecoverableFailureReason.InvalidServerResponse)
                AuthValidationOutcome.RecoverableFailure
            }
            is ApiResult.NetworkError -> authenticatedOfflineOrRecoverable(
                serverUrl,
                RecoverableFailureReason.ServerUnreachable,
                scheduleReconnect,
            )
        }
    }

    private suspend fun refreshAndValidate(
        serverUrl: String,
        refreshToken: String?,
        scheduleReconnect: Boolean,
    ): AuthValidationOutcome {
        if (refreshToken.isNullOrBlank()) {
            invalidateLocalSession(LoginRequiredReason.SessionExpired)
            return AuthValidationOutcome.NeedsLogin
        }

        return when (val refresh = api.refreshToken(refreshToken)) {
            is ApiResult.Success -> {
                val tokens = refresh.data.copy(
                    requiresPinSetup = refresh.data.requiresPinSetup || storage.requiresPinSetup,
                )
                storage.storeTokens(tokens, storage.username, tokens.userId ?: storage.userId)
                authInterceptor.setAccessToken(tokens.accessToken)
                when (val currentUser = api.currentUser()) {
                    is ApiResult.Success -> {
                        persistValidatedUser(currentUser.data, tokens)
                        AuthValidationOutcome.Online
                    }
                    is ApiResult.HttpError -> when {
                        currentUser.code == 401 || currentUser.code == 403 -> {
                            invalidateLocalSession(LoginRequiredReason.SessionRevoked)
                            AuthValidationOutcome.NeedsLogin
                        }
                        currentUser.code.isTemporaryHttpError() -> authenticatedOfflineOrRecoverable(
                            serverUrl,
                            RecoverableFailureReason.ValidationUnavailable,
                            scheduleReconnect,
                        )
                        else -> {
                            recoverable(serverUrl, RecoverableFailureReason.ValidationUnavailable)
                            AuthValidationOutcome.RecoverableFailure
                        }
                    }
                    ApiResult.EmptyBody,
                    is ApiResult.ParseError,
                    is ApiResult.ServerError -> {
                        recoverable(serverUrl, RecoverableFailureReason.InvalidServerResponse)
                        AuthValidationOutcome.RecoverableFailure
                    }
                    is ApiResult.NetworkError -> authenticatedOfflineOrRecoverable(
                        serverUrl,
                        RecoverableFailureReason.ValidationUnavailable,
                        scheduleReconnect,
                    )
                }
            }
            is ApiResult.HttpError -> when {
                refresh.code == 401 || refresh.code == 403 -> {
                    invalidateLocalSession(LoginRequiredReason.SessionRevoked)
                    AuthValidationOutcome.NeedsLogin
                }
                refresh.code.isTemporaryHttpError() -> authenticatedOfflineOrRecoverable(
                    serverUrl,
                    RecoverableFailureReason.RefreshUnavailable,
                    scheduleReconnect,
                )
                else -> {
                    recoverable(serverUrl, RecoverableFailureReason.RefreshUnavailable)
                    AuthValidationOutcome.RecoverableFailure
                }
            }
            ApiResult.EmptyBody,
            is ApiResult.ParseError,
            is ApiResult.ServerError -> {
                invalidateLocalSession(LoginRequiredReason.RefreshFailed)
                AuthValidationOutcome.NeedsLogin
            }
            is ApiResult.NetworkError -> authenticatedOfflineOrRecoverable(
                serverUrl,
                RecoverableFailureReason.RefreshUnavailable,
                scheduleReconnect,
            )
        }
    }

    private suspend fun attemptReconnect(trigger: AuthReconnectTrigger): AuthReconnectResult {
        val serverUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (serverUrl.isBlank()) return AuthReconnectResult.Terminal

        val currentState = _sessionState.value as? SessionState.Authenticated
        val reason = currentState?.offlineReason ?: RecoverableFailureReason.ServerUnreachable
        if (!markAuthenticatedProbing(serverUrl, reason)) return AuthReconnectResult.Terminal

        return when (validateSavedSession(serverUrl, scheduleReconnect = false)) {
            AuthValidationOutcome.Online -> AuthReconnectResult.Online
            AuthValidationOutcome.TemporaryOffline -> AuthReconnectResult.TemporaryOffline
            AuthValidationOutcome.NeedsLogin,
            AuthValidationOutcome.RecoverableFailure -> AuthReconnectResult.Terminal
        }
    }

    private fun authenticatedOfflineOrRecoverable(
        serverUrl: String,
        reason: RecoverableFailureReason,
        scheduleReconnect: Boolean,
    ): AuthValidationOutcome = if (markAuthenticatedOffline(serverUrl, reason, scheduleReconnect)) {
        AuthValidationOutcome.TemporaryOffline
    } else {
        recoverable(serverUrl, reason)
        AuthValidationOutcome.RecoverableFailure
    }

    private fun markAuthenticatedOffline(
        serverUrl: String,
        reason: RecoverableFailureReason,
        scheduleReconnect: Boolean,
    ): Boolean {
        val normalizedServerUrl = ServerConfig.normalize(serverUrl)
        if (normalizedServerUrl.isBlank()) return false
        val user = (_sessionState.value as? SessionState.Authenticated)?.user ?: cachedCurrentUser() ?: return false

        serverConfig.setUrl(normalizedServerUrl)
        authInterceptor.setAccessToken(storage.accessToken)
        _sessionState.value = SessionState.Authenticated(
            serverUrl = normalizedServerUrl,
            user = user,
            requiresPinSetup = storage.requiresPinSetup,
            connectionHealth = AuthConnectionHealth.Offline,
            offlineReason = reason,
        )
        if (scheduleReconnect) {
            reconnectCoordinator.scheduleBackoffRetry()
        }
        return true
    }

    private fun markAuthenticatedProbing(serverUrl: String, reason: RecoverableFailureReason): Boolean {
        val normalizedServerUrl = ServerConfig.normalize(serverUrl)
        if (normalizedServerUrl.isBlank()) return false
        val user = (_sessionState.value as? SessionState.Authenticated)?.user ?: cachedCurrentUser() ?: return false

        serverConfig.setUrl(normalizedServerUrl)
        authInterceptor.setAccessToken(storage.accessToken)
        _sessionState.value = SessionState.Authenticated(
            serverUrl = normalizedServerUrl,
            user = user,
            requiresPinSetup = storage.requiresPinSetup,
            connectionHealth = AuthConnectionHealth.Probing,
            offlineReason = reason,
        )
        return true
    }

    private fun markAuthenticatedOnlineAfterTokenRefresh() {
        val serverUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (serverUrl.isBlank()) return
        val user = (_sessionState.value as? SessionState.Authenticated)?.user ?: cachedCurrentUser() ?: return

        serverConfig.setUrl(serverUrl)
        _sessionState.value = SessionState.Authenticated(
            serverUrl = serverUrl,
            user = user,
            requiresPinSetup = storage.requiresPinSetup,
            connectionHealth = AuthConnectionHealth.Online,
        )
        reconnectCoordinator.markOnline()
    }

    private fun markRefreshTemporarilyUnavailable() {
        val serverUrl = storage.serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (serverUrl.isBlank()) {
            invalidateLocalSession(LoginRequiredReason.RefreshFailed)
            return
        }
        if (!markAuthenticatedOffline(
                serverUrl = serverUrl,
                reason = RecoverableFailureReason.RefreshUnavailable,
                scheduleReconnect = true,
            )
        ) {
            recoverable(serverUrl, RecoverableFailureReason.RefreshUnavailable)
        }
    }

    private fun persistValidatedUser(user: CurrentUser, tokens: AuthTokens) {
        storage.username = user.username
        storage.userId = user.id
        storage.userDisplayName = user.displayName
        storage.userAvatarUrl = user.avatarUrl
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
            connectionHealth = AuthConnectionHealth.Online,
        )
        reconnectCoordinator.markOnline()
    }

    private fun invalidateLocalSession(reason: LoginRequiredReason) {
        reconnectCoordinator.cancel()
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
        reconnectCoordinator.cancel()
        authInterceptor.clearAccessToken()
        serverConfig.setUrl(serverUrl)
        _sessionState.value = SessionState.RecoverableFailure(serverUrl, reason)
    }

    private fun cachedCurrentUser(): CurrentUser? {
        val userId = storage.userId.nonBlank() ?: return null
        val username = storage.username.nonBlank() ?: return null
        return CurrentUser(
            id = userId,
            username = username,
            displayName = storage.userDisplayName.nonBlank(),
            avatarUrl = storage.userAvatarUrl.nonBlank(),
        )
    }

    private fun currentDeviceInfo(): DeviceInfo {
        return DeviceInfo(
            deviceId = currentLocalDeviceId(),
            deviceName = deviceName,
            appVersion = appVersion,
        )
    }

    private fun currentLocalDeviceId(): String {
        val storedDeviceId = storage.localDeviceId?.trim()
        if (storedDeviceId?.isUuidShaped() == true) {
            val normalizedDeviceId = UUID.fromString(storedDeviceId).toString()
            if (storage.localDeviceId != normalizedDeviceId) {
                storage.localDeviceId = normalizedDeviceId
            }
            return normalizedDeviceId
        }

        return UUID.randomUUID().toString().also { storage.localDeviceId = it }
    }

    private fun String.isUuidShaped(): Boolean = UUID_REGEX.matches(this)

    private fun Int.isTemporaryHttpError(): Boolean = this == 408 || this == 429 || this >= 500

    private fun String?.nonBlank(): String? = this?.trim()?.takeIf { it.isNotEmpty() }

    private fun configureTokenRefreshCallbacks() {
        tokenRefreshAuthenticator.refreshTokenProvider = { storage.refreshToken }
        tokenRefreshAuthenticator.onTokenRefreshed = { tokens ->
            val mergedTokens = tokens.copy(
                requiresPinSetup = tokens.requiresPinSetup || storage.requiresPinSetup,
            )
            storage.storeTokens(mergedTokens, storage.username, mergedTokens.userId ?: storage.userId)
            authInterceptor.setAccessToken(mergedTokens.accessToken)
            markAuthenticatedOnlineAfterTokenRefresh()
        }
        tokenRefreshAuthenticator.onRefreshTemporarilyUnavailable = {
            markRefreshTemporarilyUnavailable()
        }
        tokenRefreshAuthenticator.onSessionInvalidated = { reason ->
            when (reason) {
                RefreshInvalidationReason.MissingRefreshToken,
                RefreshInvalidationReason.MissingServerUrl,
                RefreshInvalidationReason.RefreshRejected,
                RefreshInvalidationReason.InvalidRefreshResponse,
                RefreshInvalidationReason.RetriedRequestRejected -> invalidateLocalSession(LoginRequiredReason.SessionRevoked)
                RefreshInvalidationReason.RefreshFailed -> markRefreshTemporarilyUnavailable()
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
