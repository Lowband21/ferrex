package com.ferrex.android.core.tvfocus

import com.ferrex.android.core.auth.SessionState

/** Focus destinations used by the Android TV auth and recovery screens. */
enum class TvAuthFocusTarget {
    ServerUrl,
    Username,
    Password,
    RecoveryActions,
}

/**
 * Pure policy for Android TV auth/recovery focus and back handling.
 *
 * Keeping the decisions outside composables makes failure recovery deterministic:
 * failed network/auth attempts always put focus back on an existing field, while
 * fatal/recoverable auth states hand focus to the shared recovery action panel.
 */
object TvAuthRecoveryPolicy {
    fun initialServerFocusTarget(): TvAuthFocusTarget = TvAuthFocusTarget.ServerUrl

    fun initialLoginFocusTarget(isFatal: Boolean): TvAuthFocusTarget = if (isFatal) {
        TvAuthFocusTarget.RecoveryActions
    } else {
        TvAuthFocusTarget.Username
    }

    fun afterServerConnectResult(succeeded: Boolean): TvAuthFocusTarget? = if (succeeded) {
        null
    } else {
        TvAuthFocusTarget.ServerUrl
    }

    fun afterLoginResult(
        succeeded: Boolean,
        username: String,
        password: String,
    ): TvAuthFocusTarget? = when {
        succeeded -> null
        username.isBlank() -> TvAuthFocusTarget.Username
        password.isBlank() -> TvAuthFocusTarget.Password
        else -> TvAuthFocusTarget.Password
    }

    fun consumesBack(state: SessionState): Boolean = when (state) {
        SessionState.Loading,
        is SessionState.Authenticated -> false
        is SessionState.NoServer,
        is SessionState.NeedsLogin,
        is SessionState.RecoverableFailure -> true
    }
}
