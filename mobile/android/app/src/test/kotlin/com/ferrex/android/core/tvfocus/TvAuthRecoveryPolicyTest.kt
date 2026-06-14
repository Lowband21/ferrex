package com.ferrex.android.core.tvfocus

import com.ferrex.android.core.api.CurrentUser
import com.ferrex.android.core.auth.LoginRequiredReason
import com.ferrex.android.core.auth.NoServerReason
import com.ferrex.android.core.auth.RecoverableFailureReason
import com.ferrex.android.core.auth.SessionState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TvAuthRecoveryPolicyTest {
    @Test
    fun initialAuthFocusTargetsReachableFieldsOrRecoveryActions() {
        assertEquals(TvAuthFocusTarget.ServerUrl, TvAuthRecoveryPolicy.initialServerFocusTarget())
        assertEquals(
            TvAuthFocusTarget.Username,
            TvAuthRecoveryPolicy.initialLoginFocusTarget(isFatal = false),
        )
        assertEquals(
            TvAuthFocusTarget.RecoveryActions,
            TvAuthRecoveryPolicy.initialLoginFocusTarget(isFatal = true),
        )
    }

    @Test
    fun failedConnectAndLoginRestoreSafeFocusableFields() {
        assertEquals(
            TvAuthFocusTarget.ServerUrl,
            TvAuthRecoveryPolicy.afterServerConnectResult(succeeded = false),
        )
        assertNull(TvAuthRecoveryPolicy.afterServerConnectResult(succeeded = true))
        assertEquals(
            TvAuthFocusTarget.Username,
            TvAuthRecoveryPolicy.afterLoginResult(succeeded = false, username = "", password = "secret"),
        )
        assertEquals(
            TvAuthFocusTarget.Password,
            TvAuthRecoveryPolicy.afterLoginResult(succeeded = false, username = "grayson", password = ""),
        )
        assertEquals(
            TvAuthFocusTarget.Password,
            TvAuthRecoveryPolicy.afterLoginResult(succeeded = false, username = "grayson", password = "bad"),
        )
        assertNull(
            TvAuthRecoveryPolicy.afterLoginResult(succeeded = true, username = "grayson", password = "secret"),
        )
    }

    @Test
    fun backIsConsumedOnlyForTvAuthAndRecoveryStates() {
        assertFalse(TvAuthRecoveryPolicy.consumesBack(SessionState.Loading))
        assertTrue(
            TvAuthRecoveryPolicy.consumesBack(SessionState.NoServer(NoServerReason.FirstInstall)),
        )
        assertTrue(
            TvAuthRecoveryPolicy.consumesBack(
                SessionState.NeedsLogin("http://ferrex.local", LoginRequiredReason.NoSavedSession),
            ),
        )
        assertTrue(
            TvAuthRecoveryPolicy.consumesBack(
                SessionState.RecoverableFailure(
                    "http://ferrex.local",
                    RecoverableFailureReason.ServerUnreachable,
                ),
            ),
        )
        assertFalse(
            TvAuthRecoveryPolicy.consumesBack(
                SessionState.Authenticated(
                    serverUrl = "http://ferrex.local",
                    user = CurrentUser(id = "user-1", username = "grayson"),
                    requiresPinSetup = false,
                ),
            ),
        )
    }
}
