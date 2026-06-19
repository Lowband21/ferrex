package com.ferrex.android.core.tvfocus

/** Deterministic focus surfaces for the Android TV full-library grid. */
object TvGridFocusPolicy {
    const val SCREEN_GRID = "library-grid"

    const val SURFACE_TOP_CONTROLS = "grid-top-controls"
    const val SURFACE_MEDIA_TYPE_PANEL = "grid-media-type-panel"
    const val SURFACE_LIBRARY_PANEL = "grid-library-panel"
    const val SURFACE_MOVIE_CONTROLS_PANEL = "grid-movie-controls-panel"
    const val SURFACE_STATUS_PANEL = "grid-status-panel"
    const val SURFACE_CARDS = "grid-cards"
    const val SURFACE_EMPTY_ACTIONS = "grid-empty-actions"

    /** Legacy side-rail surfaces kept so in-memory focus from older compositions falls back safely. */
    const val SURFACE_HEADER = "grid-header"
    const val SURFACE_TABS = "grid-tabs"
    const val SURFACE_LIBRARY_CHOOSER = "grid-library-chooser"
    const val SURFACE_MOVIE_SORT = "movie-sort-controls"
    const val SURFACE_MOVIE_FILTER = "movie-filter-controls"
    const val SURFACE_RECOVERY_ACTIONS = "grid-recovery-actions"

    private val panelSurfaces = setOf(
        SURFACE_MEDIA_TYPE_PANEL,
        SURFACE_LIBRARY_PANEL,
        SURFACE_MOVIE_CONTROLS_PANEL,
        SURFACE_STATUS_PANEL,
    )

    private val legacyControlSurfaces = setOf(
        SURFACE_HEADER,
        SURFACE_TABS,
        SURFACE_LIBRARY_CHOOSER,
        SURFACE_MOVIE_SORT,
        SURFACE_MOVIE_FILTER,
        SURFACE_RECOVERY_ACTIONS,
    )

    /**
     * Keep focus on the user's last visible grid surface when possible. Redirect surfaces that can
     * disappear (poster cards, empty state, modal panels, and legacy side-rail controls) to a live
     * target so filter changes, cache clears, and panel dismissals do not strand D-pad focus.
     */
    fun preferredSurface(
        lastTarget: TvFocusKey?,
        hasCards: Boolean,
        openPanelSurface: String? = null,
    ): String = when {
        openPanelSurface != null -> openPanelSurface
        lastTarget == null && !hasCards -> SURFACE_EMPTY_ACTIONS
        lastTarget == null -> SURFACE_CARDS
        !hasCards && lastTarget.surface == SURFACE_CARDS -> SURFACE_EMPTY_ACTIONS
        hasCards && lastTarget.surface == SURFACE_EMPTY_ACTIONS -> SURFACE_CARDS
        lastTarget.surface == SURFACE_TOP_CONTROLS -> SURFACE_TOP_CONTROLS
        lastTarget.surface == SURFACE_CARDS -> SURFACE_CARDS
        lastTarget.surface == SURFACE_EMPTY_ACTIONS -> SURFACE_EMPTY_ACTIONS
        lastTarget.surface in panelSurfaces -> SURFACE_TOP_CONTROLS
        lastTarget.surface in legacyControlSurfaces -> SURFACE_TOP_CONTROLS
        else -> SURFACE_TOP_CONTROLS
    }
}
