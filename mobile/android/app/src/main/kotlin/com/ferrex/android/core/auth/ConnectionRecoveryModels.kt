package com.ferrex.android.core.auth

/** Surfaces that keep authenticated cached UI mounted while the transport recovers. */
enum class AuthenticatedConnectionSurface {
    Home,
    Detail,
}

data class AuthenticatedConnectionUi(
    val health: AuthConnectionHealth,
    val visible: Boolean,
    val title: String,
    val message: String,
    val retryLabel: String,
    val retryEnabled: Boolean,
    val networkActionsEnabled: Boolean,
    val networkActionMessage: String?,
)

fun SessionState.Authenticated.connectionRecoveryUi(
    surface: AuthenticatedConnectionSurface,
): AuthenticatedConnectionUi {
    val reason = offlineReason.connectionReasonCopy()
    return when (connectionHealth) {
        AuthConnectionHealth.Online -> AuthenticatedConnectionUi(
            health = connectionHealth,
            visible = false,
            title = "Online",
            message = "Online",
            retryLabel = "Retry connection",
            retryEnabled = false,
            networkActionsEnabled = true,
            networkActionMessage = null,
        )
        AuthConnectionHealth.Offline -> AuthenticatedConnectionUi(
            health = connectionHealth,
            visible = true,
            title = "Offline",
            message = when (surface) {
                AuthenticatedConnectionSurface.Home -> "Offline — showing cached Home while $reason. Ferrex will retry automatically."
                AuthenticatedConnectionSurface.Detail -> "Offline — cached details stay available while $reason. Ferrex will retry automatically."
            },
            retryLabel = "Retry connection",
            retryEnabled = true,
            networkActionsEnabled = false,
            networkActionMessage = "Reconnect before starting playback or updating watch state.",
        )
        AuthConnectionHealth.Probing -> AuthenticatedConnectionUi(
            health = connectionHealth,
            visible = true,
            title = "Reconnecting",
            message = when (surface) {
                AuthenticatedConnectionSurface.Home -> "Checking the saved session… Home stays available while Ferrex reconnects."
                AuthenticatedConnectionSurface.Detail -> "Checking the saved session… cached details stay available while Ferrex reconnects."
            },
            retryLabel = "Checking connection…",
            retryEnabled = false,
            networkActionsEnabled = false,
            networkActionMessage = "Ferrex is checking the saved session; playback and watch updates will be available when the connection is online.",
        )
    }
}

class ConnectionRecoveryRefreshGate(initialHealth: AuthConnectionHealth) {
    private var previousHealth: AuthConnectionHealth = initialHealth

    fun consumeOnlineRecoveryRefresh(currentHealth: AuthConnectionHealth): Boolean {
        val shouldRefresh = previousHealth != currentHealth &&
            previousHealth != AuthConnectionHealth.Online &&
            currentHealth == AuthConnectionHealth.Online
        previousHealth = currentHealth
        return shouldRefresh
    }
}

private fun RecoverableFailureReason?.connectionReasonCopy(): String = when (this) {
    RecoverableFailureReason.ServerUnreachable -> "the server is unreachable"
    RecoverableFailureReason.ValidationUnavailable -> "session validation is unavailable"
    RecoverableFailureReason.RefreshUnavailable -> "token refresh is temporarily unavailable"
    RecoverableFailureReason.InvalidServerResponse -> "the server response was not understood"
    null -> "the connection is temporarily unavailable"
}
