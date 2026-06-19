package com.ferrex.android.core.auth

import com.ferrex.android.core.api.CurrentUser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionRecoveryModelsTest {
    @Test
    fun offlineHomeStatusKeepsCachedUiVisibleAndDisablesNetworkActions() {
        val status = authenticated(
            health = AuthConnectionHealth.Offline,
            reason = RecoverableFailureReason.ServerUnreachable,
        ).connectionRecoveryUi(AuthenticatedConnectionSurface.Home)

        assertTrue(status.visible)
        assertEquals("Offline", status.title)
        assertTrue(status.message.contains("cached Home"))
        assertTrue(status.message.contains("server is unreachable"))
        assertTrue(status.retryEnabled)
        assertFalse(status.networkActionsEnabled)
        assertTrue(status.networkActionMessage!!.contains("Reconnect"))
    }

    @Test
    fun probingDetailStatusKeepsDetailsVisibleWithoutRetryStormActions() {
        val status = authenticated(
            health = AuthConnectionHealth.Probing,
            reason = RecoverableFailureReason.RefreshUnavailable,
        ).connectionRecoveryUi(AuthenticatedConnectionSurface.Detail)

        assertTrue(status.visible)
        assertEquals("Reconnecting", status.title)
        assertTrue(status.message.contains("cached details"))
        assertEquals("Checking connection…", status.retryLabel)
        assertFalse(status.retryEnabled)
        assertFalse(status.networkActionsEnabled)
    }

    @Test
    fun onlineStatusHidesBannerAndEnablesPlaybackAndWatchActions() {
        val status = authenticated(AuthConnectionHealth.Online).connectionRecoveryUi(AuthenticatedConnectionSurface.Detail)

        assertFalse(status.visible)
        assertTrue(status.networkActionsEnabled)
        assertFalse(status.retryEnabled)
        assertNull(status.networkActionMessage)
    }

    @Test
    fun onlineRecoveryRefreshGateTriggersOncePerOfflineToOnlineTransition() {
        val gate = ConnectionRecoveryRefreshGate(AuthConnectionHealth.Offline)

        assertFalse(gate.consumeOnlineRecoveryRefresh(AuthConnectionHealth.Offline))
        assertFalse(gate.consumeOnlineRecoveryRefresh(AuthConnectionHealth.Probing))
        assertTrue(gate.consumeOnlineRecoveryRefresh(AuthConnectionHealth.Online))
        assertFalse(gate.consumeOnlineRecoveryRefresh(AuthConnectionHealth.Online))
        assertFalse(gate.consumeOnlineRecoveryRefresh(AuthConnectionHealth.Offline))
        assertTrue(gate.consumeOnlineRecoveryRefresh(AuthConnectionHealth.Online))
    }

    @Test
    fun noWipeRecoveryActionsCoverAuthAndCacheRecoveryWithoutOsAppDataClear() {
        val actions = noWipeRecoveryActions(includeCacheClear = true)

        assertEquals(
            listOf(
                NoWipeRecoveryActionKind.Retry,
                NoWipeRecoveryActionKind.SignOut,
                NoWipeRecoveryActionKind.ChangeServer,
                NoWipeRecoveryActionKind.ResetConnection,
                NoWipeRecoveryActionKind.Diagnostics,
                NoWipeRecoveryActionKind.ClearCache,
            ),
            actions.map { it.kind },
        )
        actions.forEach { action ->
            assertFalse("${action.key} must not clear OS app data", action.requiresOsAppDataClear)
            assertFalse(action.label.contains("wipe", ignoreCase = true))
            assertFalse(action.subtitle.contains("wipe", ignoreCase = true))
            assertTrue(action.subtitle.contains("Ferrex", ignoreCase = true) || action.kind == NoWipeRecoveryActionKind.Diagnostics)
        }
        assertFalse(noWipeRecoveryActions(includeCacheClear = false).any { it.kind == NoWipeRecoveryActionKind.ClearCache })
    }

    private fun authenticated(
        health: AuthConnectionHealth,
        reason: RecoverableFailureReason? = null,
    ): SessionState.Authenticated = SessionState.Authenticated(
        serverUrl = "http://ferrex.local",
        user = CurrentUser(id = "user-1", username = "grayson", displayName = "Grayson"),
        requiresPinSetup = false,
        connectionHealth = health,
        offlineReason = reason,
    )
}
