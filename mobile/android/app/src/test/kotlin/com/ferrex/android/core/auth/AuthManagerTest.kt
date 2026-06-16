package com.ferrex.android.core.auth

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.AuthInterceptor
import com.ferrex.android.core.api.AuthTokens
import com.ferrex.android.core.api.CurrentUser
import com.ferrex.android.core.api.DeviceInfo
import com.ferrex.android.core.api.FerrexApi
import com.ferrex.android.core.api.KnownDeviceProfilesResponse
import com.ferrex.android.core.api.PinChallengeResponse
import com.ferrex.android.core.api.PinLoginRequest
import com.ferrex.android.core.api.RefreshInvalidationReason
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.api.SetupStatus
import com.ferrex.android.core.api.TokenRefreshAuthenticator
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.UUID

@OptIn(ExperimentalCoroutinesApi::class)
class AuthManagerTest {
    @Test
    fun restoredSessionValidatesCurrentUserBeforeAuthenticated() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "saved-access"
        fixture.storage.refreshToken = "saved-refresh"
        fixture.api.currentUserResults += ApiResult.Success(testUser)

        fixture.manager.initialize()

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.Authenticated)
        assertEquals("saved-access", fixture.interceptor.accessToken)
        assertEquals("http://ferrex.local", (state as SessionState.Authenticated).serverUrl)
        assertEquals(AuthConnectionHealth.Online, state.connectionHealth)
    }

    @Test
    fun restoredSessionLaunchesAuthenticatedOfflineWhenCurrentUserUnreachableWithCachedIdentity() = runTest {
        val fixture = Fixture(
            reconnectScope = backgroundScope,
            reconnectBackoffDelaysMillis = listOf(1_000L),
        )
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "saved-access"
        fixture.storage.refreshToken = "saved-refresh"
        fixture.storage.username = testUser.username
        fixture.storage.userId = testUser.id
        fixture.storage.userDisplayName = testUser.displayName
        fixture.api.currentUserResults += ApiResult.NetworkError("offline")

        fixture.manager.initialize()

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.Authenticated)
        state as SessionState.Authenticated
        assertEquals(AuthConnectionHealth.Offline, state.connectionHealth)
        assertEquals(RecoverableFailureReason.ServerUnreachable, state.offlineReason)
        assertEquals(testUser.id, state.user.id)
        assertEquals(testUser.displayName, state.user.displayName)
        assertEquals("saved-access", fixture.storage.accessToken)
        assertEquals("saved-refresh", fixture.storage.refreshToken)
        assertEquals("http://ferrex.local", fixture.storage.serverUrl)
        assertEquals("saved-access", fixture.interceptor.accessToken)
        assertTrue(fixture.resetClears.isEmpty())
    }

    @Test
    fun accessToken401RefreshesOnceAndValidatesRetriedUser() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "stale-access"
        fixture.storage.refreshToken = "saved-refresh"
        fixture.api.currentUserResults += ApiResult.HttpError(401, "Unauthorized")
        fixture.api.refreshResults += ApiResult.Success(
            AuthTokens(accessToken = "new-access", refreshToken = "new-refresh", userId = testUser.id),
        )
        fixture.api.currentUserResults += ApiResult.Success(testUser)

        fixture.manager.initialize()

        assertTrue(fixture.manager.sessionState.value is SessionState.Authenticated)
        assertEquals("new-access", fixture.storage.accessToken)
        assertEquals("new-refresh", fixture.storage.refreshToken)
        assertEquals("new-access", fixture.interceptor.accessToken)
        assertEquals(1, fixture.api.refreshCalls)
    }

    @Test
    fun refreshRejectionClearsTokensAndRequiresLogin() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "stale-access"
        fixture.storage.refreshToken = "revoked-refresh"
        fixture.api.currentUserResults += ApiResult.HttpError(401, "Unauthorized")
        fixture.api.refreshResults += ApiResult.HttpError(401, "Unauthorized")

        fixture.manager.initialize()

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.NeedsLogin)
        assertEquals(LoginRequiredReason.SessionRevoked, (state as SessionState.NeedsLogin).reason)
        assertNull(fixture.storage.accessToken)
        assertNull(fixture.storage.refreshToken)
        assertNull(fixture.interceptor.accessToken)
    }

    @Test
    fun invalidRefreshResponseFromAuthenticatorClearsTokensAndRequiresLogin() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "saved-access"
        fixture.storage.refreshToken = "saved-refresh"
        fixture.storage.username = testUser.username
        fixture.storage.userId = testUser.id
        fixture.api.currentUserResults += ApiResult.Success(testUser)
        fixture.manager.initialize()

        fixture.authenticator.onSessionInvalidated?.invoke(RefreshInvalidationReason.InvalidRefreshResponse)

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.NeedsLogin)
        assertEquals(LoginRequiredReason.SessionRevoked, (state as SessionState.NeedsLogin).reason)
        assertNull(fixture.storage.accessToken)
        assertNull(fixture.storage.refreshToken)
        assertNull(fixture.interceptor.accessToken)
    }

    @Test
    fun temporaryRefreshFailureKeepsHomeAuthenticatedOfflineAndPreservesTokens() = runTest {
        val fixture = Fixture(
            reconnectScope = backgroundScope,
            reconnectBackoffDelaysMillis = listOf(1_000L),
        )
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "saved-access"
        fixture.storage.refreshToken = "saved-refresh"
        fixture.storage.username = testUser.username
        fixture.storage.userId = testUser.id
        fixture.api.currentUserResults += ApiResult.Success(testUser)
        fixture.manager.initialize()

        fixture.authenticator.onRefreshTemporarilyUnavailable?.invoke()

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.Authenticated)
        state as SessionState.Authenticated
        assertEquals(AuthConnectionHealth.Offline, state.connectionHealth)
        assertEquals(RecoverableFailureReason.RefreshUnavailable, state.offlineReason)
        assertEquals("saved-access", fixture.storage.accessToken)
        assertEquals("saved-refresh", fixture.storage.refreshToken)
        assertEquals("saved-access", fixture.interceptor.accessToken)
        assertTrue(fixture.resetClears.isEmpty())
    }

    @Test
    fun backoffReconnectReturnsAuthenticatedSessionOnlineAfterCurrentUserSuccess() = runTest {
        val fixture = Fixture(
            reconnectScope = backgroundScope,
            reconnectBackoffDelaysMillis = listOf(100L),
        )
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "saved-access"
        fixture.storage.refreshToken = "saved-refresh"
        fixture.storage.username = testUser.username
        fixture.storage.userId = testUser.id
        fixture.api.currentUserResults += ApiResult.NetworkError("offline")

        fixture.manager.initialize()
        fixture.api.currentUserResults += ApiResult.Success(testUser.copy(displayName = "Fresh User"))
        advanceTimeBy(100L)
        runCurrent()

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.Authenticated)
        state as SessionState.Authenticated
        assertEquals(AuthConnectionHealth.Online, state.connectionHealth)
        assertEquals("Fresh User", state.user.displayName)
        assertEquals("saved-access", fixture.storage.accessToken)
        assertEquals("saved-refresh", fixture.storage.refreshToken)
        assertEquals("saved-access", fixture.interceptor.accessToken)
        assertEquals(2, fixture.api.currentUserCalls)
    }

    @Test
    fun networkUnavailablePreservesSavedTokensAndShowsRecoverableState() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "saved-access"
        fixture.storage.refreshToken = "saved-refresh"
        fixture.api.currentUserResults += ApiResult.NetworkError("offline")

        fixture.manager.initialize()

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.RecoverableFailure)
        assertEquals("saved-access", fixture.storage.accessToken)
        assertEquals("saved-refresh", fixture.storage.refreshToken)
        assertEquals("http://ferrex.local", fixture.storage.serverUrl)
        assertNull(fixture.interceptor.accessToken)
    }

    @Test
    fun playbackSessionInvalidationRequiresLoginWithoutClearingServerOrDevice() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "expired-access"
        fixture.storage.refreshToken = "expired-refresh"
        fixture.storage.username = testUser.username
        fixture.storage.userId = testUser.id
        fixture.storage.userDisplayName = testUser.displayName
        fixture.storage.localDeviceId = "018f5f8d-0000-7000-8000-000000000001"
        fixture.interceptor.setAccessToken("expired-access")

        fixture.manager.invalidateSessionFromPlayback()

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.NeedsLogin)
        assertEquals(LoginRequiredReason.SessionRevoked, (state as SessionState.NeedsLogin).reason)
        assertEquals("http://ferrex.local", state.serverUrl)
        assertEquals("http://ferrex.local", fixture.storage.serverUrl)
        assertEquals("018f5f8d-0000-7000-8000-000000000001", fixture.storage.localDeviceId)
        assertNull(fixture.storage.accessToken)
        assertNull(fixture.storage.refreshToken)
        assertNull(fixture.interceptor.accessToken)
        assertTrue(fixture.resetClears.isEmpty())
    }

    @Test
    fun signOutClearsTokensAndPreservesServerUrl() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.accessToken = "access"
        fixture.storage.refreshToken = "refresh"
        fixture.interceptor.setAccessToken("access")

        fixture.manager.signOut()

        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.NeedsLogin)
        assertEquals(LoginRequiredReason.SignedOut, (state as SessionState.NeedsLogin).reason)
        assertEquals("http://ferrex.local", fixture.storage.serverUrl)
        assertNull(fixture.storage.accessToken)
        assertNull(fixture.storage.refreshToken)
        assertNull(fixture.interceptor.accessToken)
    }

    @Test
    fun changeServerOnlyReplacesStoredUrlAfterSuccessfulConnect() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://old.local"
        fixture.storage.accessToken = "access"
        fixture.storage.refreshToken = "refresh"

        fixture.manager.changeServer()
        fixture.api.setupResults += ApiResult.NetworkError("offline")
        val failed = fixture.manager.connectToServer("http://bad.local")

        assertTrue(failed is ConnectResult.Error)
        assertEquals("http://old.local", fixture.storage.serverUrl)
        assertNull(fixture.storage.accessToken)
        assertNull(fixture.storage.refreshToken)

        fixture.api.setupResults += ApiResult.Success(SetupStatus(hasAdmin = true, needsSetup = false))
        val connected = fixture.manager.connectToServer("http://new.local/")

        assertTrue(connected is ConnectResult.Success)
        assertEquals("http://new.local", fixture.storage.serverUrl)
        val state = fixture.manager.sessionState.value
        assertTrue(state is SessionState.NeedsLogin)
        assertEquals(LoginRequiredReason.ChangedServer, (state as SessionState.NeedsLogin).reason)
    }

    @Test
    fun resetConnectionClearsServerTokensInterceptorAndServerScopedCache() = runTest {
        val clearedScopes = mutableListOf<Pair<String, String?>>()
        val fixture = Fixture(resetClears = clearedScopes)
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.userId = "user-1"
        fixture.storage.accessToken = "access"
        fixture.storage.refreshToken = "refresh"
        fixture.storage.localDeviceId = "device-id"
        fixture.interceptor.setAccessToken("access")

        fixture.manager.resetConnection()

        assertTrue(fixture.manager.sessionState.value is SessionState.NoServer)
        assertNull(fixture.storage.serverUrl)
        assertNull(fixture.storage.accessToken)
        assertNull(fixture.storage.refreshToken)
        assertNull(fixture.storage.localDeviceId)
        assertNull(fixture.interceptor.accessToken)
        assertEquals(listOf("http://ferrex.local" to "user-1"), clearedScopes)
    }

    @Test
    fun passwordLoginStoresTokensAfterCurrentUserValidation() = runTest {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.localDeviceId = "018f5f8d-0000-7000-8000-000000000001"
        fixture.api.loginResults += ApiResult.Success(
            AuthTokens(
                accessToken = "login-access",
                refreshToken = "login-refresh",
                sessionId = "session-1",
                deviceSessionId = "device-session-1",
                userId = "token-user",
                requiresPinSetup = true,
            ),
        )
        fixture.api.currentUserResults += ApiResult.Success(testUser)

        val result = fixture.manager.loginWithPassword(" grayson ", "correct-password")

        assertTrue(result is LoginResult.Success)
        assertTrue((result as LoginResult.Success).requiresPinSetup)
        assertEquals("login-access", fixture.storage.accessToken)
        assertEquals("login-refresh", fixture.storage.refreshToken)
        assertEquals("session-1", fixture.storage.sessionId)
        assertEquals("device-session-1", fixture.storage.deviceSessionId)
        assertEquals(testUser.username, fixture.storage.username)
        assertEquals(testUser.id, fixture.storage.userId)
        assertEquals("login-access", fixture.interceptor.accessToken)
        assertEquals(1, fixture.api.currentUserCalls)
        val loginCall = fixture.api.loginCalls.single()
        assertEquals("grayson", loginCall.username)
        assertEquals("correct-password", loginCall.password)
        assertEquals(false, loginCall.rememberDevice)
    }

    @Test
    fun passwordLoginRegeneratesBlankLocalDeviceId() = runTest {
        assertPasswordLoginRegeneratesLocalDeviceId("   ")
    }

    @Test
    fun passwordLoginRegeneratesMalformedLocalDeviceId() = runTest {
        assertPasswordLoginRegeneratesLocalDeviceId("device-id")
    }

    private suspend fun assertPasswordLoginRegeneratesLocalDeviceId(initialDeviceId: String) {
        val fixture = Fixture()
        fixture.storage.serverUrl = "http://ferrex.local"
        fixture.storage.localDeviceId = initialDeviceId
        fixture.api.loginResults += ApiResult.Success(
            AuthTokens(
                accessToken = "login-access",
                refreshToken = "login-refresh",
                userId = testUser.id,
            ),
        )
        fixture.api.currentUserResults += ApiResult.Success(testUser)

        val result = fixture.manager.loginWithPassword("grayson", "password")

        assertTrue(result is LoginResult.Success)
        val storedDeviceId = fixture.storage.localDeviceId.orEmpty()
        assertEquals(storedDeviceId, UUID.fromString(storedDeviceId).toString())
        assertTrue("Expected a regenerated UUID", storedDeviceId != initialDeviceId.trim())
        assertEquals(storedDeviceId, fixture.api.loginCalls.single().deviceInfo.deviceId)
    }

    private class Fixture(
        val resetClears: MutableList<Pair<String, String?>> = mutableListOf(),
        reconnectScope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
        reconnectBackoffDelaysMillis: List<Long> = AuthReconnectCoordinator.DEFAULT_BACKOFF_DELAYS_MILLIS,
    ) {
        val api = FakeFerrexApi()
        val storage = InMemoryAuthStorage()
        val serverConfig = ServerConfig()
        val interceptor = AuthInterceptor()
        val authenticator = TokenRefreshAuthenticator(serverConfig, interceptor)
        val manager = AuthManager(
            api = api,
            storage = storage,
            serverConfig = serverConfig,
            authInterceptor = interceptor,
            tokenRefreshAuthenticator = authenticator,
            deviceName = "Test Android",
            appVersion = "test",
            onResetConnectionCacheClear = { serverUrl, userId -> resetClears += serverUrl to userId },
            reconnectScope = reconnectScope,
            reconnectBackoffDelaysMillis = reconnectBackoffDelaysMillis,
        )
    }

    private class FakeFerrexApi : FerrexApi {
        val setupResults = ArrayDeque<ApiResult<SetupStatus>>()
        val loginResults = ArrayDeque<ApiResult<AuthTokens>>()
        val refreshResults = ArrayDeque<ApiResult<AuthTokens>>()
        val currentUserResults = ArrayDeque<ApiResult<CurrentUser>>()
        val loginCalls = mutableListOf<PasswordLoginCall>()
        var refreshCalls = 0
        var currentUserCalls = 0

        override suspend fun getSetupStatus(serverUrl: String): ApiResult<SetupStatus> = setupResults.removeFirst()

        override suspend fun knownDeviceUsers(deviceInfo: DeviceInfo): ApiResult<KnownDeviceProfilesResponse> =
            ApiResult.Success(KnownDeviceProfilesResponse())

        override suspend fun devicePasswordLogin(
            username: String,
            password: String,
            deviceInfo: DeviceInfo,
            rememberDevice: Boolean,
        ): ApiResult<AuthTokens> {
            loginCalls += PasswordLoginCall(username, password, deviceInfo, rememberDevice)
            return if (loginResults.isEmpty()) {
                ApiResult.Success(
                    AuthTokens(accessToken = "login-access", refreshToken = "login-refresh", userId = testUser.id),
                )
            } else {
                loginResults.removeFirst()
            }
        }

        override suspend fun requestPinChallenge(deviceId: String): ApiResult<PinChallengeResponse> =
            ApiResult.NetworkError("not used")

        override suspend fun pinLogin(request: PinLoginRequest): ApiResult<AuthTokens> =
            ApiResult.NetworkError("not used")

        override suspend fun refreshToken(refreshToken: String): ApiResult<AuthTokens> {
            refreshCalls += 1
            return refreshResults.removeFirst()
        }

        override suspend fun currentUser(): ApiResult<CurrentUser> {
            currentUserCalls += 1
            return currentUserResults.removeFirst()
        }
    }

    private data class PasswordLoginCall(
        val username: String,
        val password: String,
        val deviceInfo: DeviceInfo,
        val rememberDevice: Boolean,
    )

    private class InMemoryAuthStorage : AuthStorage {
        override var serverUrl: String? = null
        override var accessToken: String? = null
        override var refreshToken: String? = null
        override var username: String? = null
        override var userId: String? = null
        override var userDisplayName: String? = null
        override var userAvatarUrl: String? = null
        override var sessionId: String? = null
        override var deviceSessionId: String? = null
        override var localDeviceId: String? = null
        override var requiresPinSetup: Boolean = false
    }

    private companion object {
        val testUser = CurrentUser(
            id = "018f5f8d-0000-7000-8000-000000000001",
            username = "grayson",
            displayName = "Grayson",
        )
    }
}
