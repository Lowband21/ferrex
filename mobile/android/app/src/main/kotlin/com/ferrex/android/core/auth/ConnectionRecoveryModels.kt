package com.ferrex.android.core.auth

/** Surfaces that keep authenticated cached UI mounted while the transport recovers. */
enum class AuthenticatedConnectionSurface {
    Home,
    Detail,
}

enum class NoWipeRecoveryActionKind(
    val key: String,
    val label: String,
    val subtitle: String,
) {
    Retry(
        key = "retry",
        label = "Retry",
        subtitle = "Try the failed request again without changing saved Ferrex data.",
    ),
    SignOut(
        key = "sign-out",
        label = "Sign out",
        subtitle = "Clear only the local Ferrex session and return to sign in.",
    ),
    ChangeServer(
        key = "change-server",
        label = "Change server",
        subtitle = "Open the Ferrex server picker while keeping local recovery controls available.",
    ),
    ResetConnection(
        key = "reset-connection",
        label = "Reset connection",
        subtitle = "Reset saved Ferrex connection metadata and scoped caches while preserving Android app data.",
    ),
    Diagnostics(
        key = "diagnostics",
        label = "Diagnostics / Export diagnostics",
        subtitle = "Export redacted diagnostics for support without exposing tokens.",
    ),
    ClearCache(
        key = "clear-cache",
        label = "Clear cache",
        subtitle = "Clear scoped Ferrex media or image cache entries while preserving Android app data.",
    ),
}

data class NoWipeRecoveryActionDescriptor(
    val kind: NoWipeRecoveryActionKind,
    val key: String = kind.key,
    val label: String = kind.label,
    val subtitle: String = kind.subtitle,
) {
    init {
        require(key.isNotBlank()) { "recovery action key must not be blank" }
        require(label.isNotBlank()) { "recovery action label must not be blank" }
        require(subtitle.isNotBlank()) { "recovery action subtitle must not be blank" }
    }

    val requiresOsAppDataClear: Boolean = false
}

fun noWipeRecoveryActions(includeCacheClear: Boolean): List<NoWipeRecoveryActionDescriptor> = buildList {
    add(NoWipeRecoveryActionDescriptor(NoWipeRecoveryActionKind.Retry))
    add(NoWipeRecoveryActionDescriptor(NoWipeRecoveryActionKind.SignOut))
    add(NoWipeRecoveryActionDescriptor(NoWipeRecoveryActionKind.ChangeServer))
    add(NoWipeRecoveryActionDescriptor(NoWipeRecoveryActionKind.ResetConnection))
    add(NoWipeRecoveryActionDescriptor(NoWipeRecoveryActionKind.Diagnostics))
    if (includeCacheClear) add(NoWipeRecoveryActionDescriptor(NoWipeRecoveryActionKind.ClearCache))
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
