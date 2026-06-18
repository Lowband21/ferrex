package com.ferrex.android.core.tvfocus

/** Deterministic focus surfaces for the Android TV full-library grid. */
object TvGridFocusPolicy {
    const val SCREEN_GRID = "library-grid"

    const val SURFACE_HEADER = "grid-header"
    const val SURFACE_TABS = "grid-tabs"
    const val SURFACE_LIBRARY_CHOOSER = "grid-library-chooser"
    const val SURFACE_MOVIE_SORT = "movie-sort-controls"
    const val SURFACE_MOVIE_FILTER = "movie-filter-controls"
    const val SURFACE_RECOVERY_ACTIONS = "grid-recovery-actions"
    const val SURFACE_CARDS = "grid-cards"
    const val SURFACE_EMPTY_ACTIONS = "grid-empty-actions"

    /**
     * Keep focus on the user's last grid surface when possible, but redirect a disappearing poster
     * grid to the empty-state recovery row instead of leaving D-pad focus on a removed card.
     */
    fun preferredSurface(lastTarget: TvFocusKey?, hasCards: Boolean): String = when {
        lastTarget == null && !hasCards -> SURFACE_EMPTY_ACTIONS
        lastTarget == null -> SURFACE_CARDS
        !hasCards && lastTarget.surface == SURFACE_CARDS -> SURFACE_EMPTY_ACTIONS
        else -> lastTarget.surface
    }
}
