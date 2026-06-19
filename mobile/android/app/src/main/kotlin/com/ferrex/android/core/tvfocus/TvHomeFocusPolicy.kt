package com.ferrex.android.core.tvfocus

/**
 * Deterministic initial focus policy for Android TV Home.
 *
 * Home prefers active media, then the search entry point, then library browsing,
 * and finally recovery actions so every state has a D-pad reachable escape.
 */
object TvHomeFocusPolicy {
    const val SCREEN_HOME = "home"
    const val SURFACE_CONTINUE_WATCHING = "continue-watching"
    const val SURFACE_HOME_ACTIONS = "home-actions"
    const val SURFACE_LIBRARY_ACTIONS = "library-actions"
    const val SURFACE_RECOVERY_ACTIONS = "recovery-actions"
    const val ITEM_SEARCH = "search"
    const val ITEM_DIAGNOSTICS = "settings-diagnostics"

    fun initialHomeTarget(
        continueWatchingKeys: List<String>,
        searchAvailable: Boolean,
        libraryActionKeys: List<String>,
        recoveryActionKeys: List<String>,
        homeActionKeys: List<String> = emptyList(),
    ): TvFocusKey {
        continueWatchingKeys.firstOrNull()?.let { key ->
            return TvFocusKey(SCREEN_HOME, SURFACE_CONTINUE_WATCHING, key)
        }
        if (searchAvailable && ITEM_SEARCH in homeActionKeys) {
            return TvFocusKey(SCREEN_HOME, SURFACE_HOME_ACTIONS, ITEM_SEARCH)
        }
        homeActionKeys.firstOrNull()?.let { key ->
            return TvFocusKey(SCREEN_HOME, SURFACE_HOME_ACTIONS, key)
        }
        if (searchAvailable) {
            return TvFocusKey(SCREEN_HOME, SURFACE_HOME_ACTIONS, ITEM_SEARCH)
        }
        libraryActionKeys.firstOrNull()?.let { key ->
            return TvFocusKey(SCREEN_HOME, SURFACE_LIBRARY_ACTIONS, key)
        }
        return TvFocusKey(
            screen = SCREEN_HOME,
            surface = SURFACE_RECOVERY_ACTIONS,
            item = recoveryActionKeys.firstOrNull() ?: "retry-cache-sync",
        )
    }
}
