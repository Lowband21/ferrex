package com.ferrex.android.core.auth

import com.ferrex.android.core.api.CurrentUser
import com.ferrex.android.core.api.SetupStatus

sealed interface SessionState {
    data object Loading : SessionState

    data class NoServer(
        val reason: NoServerReason = NoServerReason.FirstInstall,
        val previousServerUrl: String? = null,
    ) : SessionState

    data class NeedsLogin(
        val serverUrl: String,
        val reason: LoginRequiredReason = LoginRequiredReason.NoSavedSession,
        val setupStatus: SetupStatus? = null,
    ) : SessionState

    data class RecoverableFailure(
        val serverUrl: String,
        val reason: RecoverableFailureReason,
    ) : SessionState

    data class Authenticated(
        val serverUrl: String,
        val user: CurrentUser,
        val requiresPinSetup: Boolean,
    ) : SessionState
}

enum class NoServerReason {
    FirstInstall,
    ResetConnection,
    ChangeServer,
}

enum class LoginRequiredReason {
    NoSavedSession,
    SignedOut,
    SessionExpired,
    SessionRevoked,
    RefreshFailed,
    SetupRequired,
    RegistrationClosed,
    ChangedServer,
}

enum class RecoverableFailureReason {
    ServerUnreachable,
    ValidationUnavailable,
    RefreshUnavailable,
    InvalidServerResponse,
}

val SessionState.serverUrlOrNull: String?
    get() = when (this) {
        is SessionState.Authenticated -> serverUrl
        is SessionState.NeedsLogin -> serverUrl
        is SessionState.RecoverableFailure -> serverUrl
        is SessionState.NoServer -> previousServerUrl
        SessionState.Loading -> null
    }
