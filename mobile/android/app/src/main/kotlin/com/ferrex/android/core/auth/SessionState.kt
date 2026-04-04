package com.ferrex.android.core.auth

/**
 * App-level session state, exposed as StateFlow from AuthManager.
 * The navigation graph observes this to determine the start destination.
 */
sealed interface SessionState {
    /** Checking stored credentials on launch. */
    data object Loading : SessionState

    /** No server URL configured — show server connect screen. */
    data object NoServer : SessionState

    /** Server configured but no valid session — show login. */
    data object NeedsLogin : SessionState

    /** Authenticated with valid tokens. */
    data class Authenticated(
        val accessToken: String,
        val username: String?,
    ) : SessionState
}
