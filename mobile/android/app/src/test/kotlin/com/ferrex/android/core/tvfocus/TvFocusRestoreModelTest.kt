package com.ferrex.android.core.tvfocus

import com.ferrex.android.core.browse.LibraryBrowseModels
import com.ferrex.android.core.browse.LibraryRecoveryActionKeys
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TvFocusRestoreModelTest {
    @Test
    fun restoreReturnsRememberedItemWhenItIsStillAvailable() {
        val state = TvFocusRestoreState()
            .record(TvFocusKey(screen = "home", surface = "cache-actions", item = "clear-all"))

        val request = state.restore(
            screen = "home",
            surface = "cache-actions",
            availableItems = listOf("retry", "clear-selected", "clear-all"),
            fallbackItem = "retry",
        )

        assertEquals(TvFocusKey("home", "cache-actions", "clear-all"), request.target)
        assertFalse(request.usedFallback)
    }

    @Test
    fun restoreFallsBackWhenRememberedItemDisappears() {
        val state = TvFocusRestoreState()
            .record(TvFocusKey(screen = "home", surface = "cache-actions", item = "clear-selected"))

        val request = state.restore(
            screen = "home",
            surface = "cache-actions",
            availableItems = listOf("retry", "clear-all"),
            fallbackItem = "retry",
        )

        assertEquals(TvFocusKey("home", "cache-actions", "retry"), request.target)
        assertTrue(request.usedFallback)
    }

    @Test
    fun focusMemoryIsScopedByScreenAndSurface() {
        val state = TvFocusRestoreState()
            .record(TvFocusKey(screen = "home", surface = "actions", item = "change-server"))
            .record(TvFocusKey(screen = "recovery", surface = "actions", item = "retry"))

        val homeRequest = state.restore(
            screen = "home",
            surface = "actions",
            availableItems = listOf("retry", "change-server"),
            fallbackItem = "retry",
        )
        val recoveryRequest = state.restore(
            screen = "recovery",
            surface = "actions",
            availableItems = listOf("retry", "change-server"),
            fallbackItem = "change-server",
        )

        assertEquals("change-server", homeRequest.target.item)
        assertEquals("retry", recoveryRequest.target.item)
    }

    @Test
    fun forgetScreenLeavesOtherScreensIntact() {
        val state = TvFocusRestoreState()
            .record(TvFocusKey(screen = "home", surface = "actions", item = "reset"))
            .record(TvFocusKey(screen = "login", surface = "actions", item = "retry"))
            .forgetScreen("home")

        assertNull(state.rememberedTarget(screen = "home", surface = "actions"))
        assertEquals(TvFocusKey("login", "actions", "retry"), state.rememberedTarget(screen = "login", surface = "actions"))
    }

    @Test
    fun lastTargetTracksMostRecentFocusAndForgetsWithScreen() {
        val state = TvFocusRestoreState()
            .record(TvFocusKey(screen = "home", surface = "continue", item = "movie-1"))
            .record(TvFocusKey(screen = "home", surface = "library", item = "browse"))

        assertEquals(TvFocusKey("home", "library", "browse"), state.lastTarget("home"))
        assertNull(state.forgetScreen("home").lastTarget("home"))
    }

    @Test
    fun tvDetailInitialFocusIsPageScopedBackWithStableSurfaces() {
        val movieTarget = TvDetailFocusPolicy.initialDetailTarget("movie:alpha")
        val episodeTarget = TvDetailFocusPolicy.initialDetailTarget("episode:pilot")

        assertEquals(TvDetailFocusPolicy.SURFACE_BACK, movieTarget.surface)
        assertEquals(TvDetailFocusPolicy.ITEM_BACK, movieTarget.item)
        assertEquals("detail:movie:alpha", movieTarget.screen)
        assertEquals("detail:episode:pilot", episodeTarget.screen)
        assertEquals("detail-rail:series:alpha:episodes", TvDetailFocusPolicy.railSurface("series:alpha:episodes"))
    }

    @Test
    fun tvGridFocusMovesBetweenDenseCardsAndEmptyRecovery() {
        val cardState = TvFocusRestoreState()
            .record(TvFocusKey(TvGridFocusPolicy.SCREEN_GRID, TvGridFocusPolicy.SURFACE_CARDS, "movie:library:item"))
        val emptyState = TvFocusRestoreState()
            .record(TvFocusKey(TvGridFocusPolicy.SCREEN_GRID, TvGridFocusPolicy.SURFACE_EMPTY_ACTIONS, "retry-all"))

        assertEquals(
            TvGridFocusPolicy.SURFACE_EMPTY_ACTIONS,
            TvGridFocusPolicy.preferredSurface(lastTarget = null, hasCards = false),
        )
        assertEquals(
            TvGridFocusPolicy.SURFACE_EMPTY_ACTIONS,
            TvGridFocusPolicy.preferredSurface(cardState.lastTarget(TvGridFocusPolicy.SCREEN_GRID), hasCards = false),
        )
        assertEquals(
            TvGridFocusPolicy.SURFACE_CARDS,
            TvGridFocusPolicy.preferredSurface(cardState.lastTarget(TvGridFocusPolicy.SCREEN_GRID), hasCards = true),
        )
        assertEquals(
            TvGridFocusPolicy.SURFACE_CARDS,
            TvGridFocusPolicy.preferredSurface(emptyState.lastTarget(TvGridFocusPolicy.SCREEN_GRID), hasCards = true),
        )
    }

    @Test
    fun tvGridRecoveryActionsKeepNoWipeExitsDpadReachable() {
        val actionKeys = LibraryBrowseModels.recoveryActionKeys(selectedLibraryId = "series-library", includeRetryAll = true)
        val state = TvFocusRestoreState()
            .record(TvFocusKey(TvGridFocusPolicy.SCREEN_GRID, TvGridFocusPolicy.SURFACE_STATUS_PANEL, LibraryRecoveryActionKeys.Diagnostics))

        assertEquals(
            listOf(
                LibraryRecoveryActionKeys.RetrySelected,
                LibraryRecoveryActionKeys.RetryAll,
                LibraryRecoveryActionKeys.ClearSelectedCache,
                LibraryRecoveryActionKeys.ClearAllCache,
                LibraryRecoveryActionKeys.ChangeServer,
                LibraryRecoveryActionKeys.ResetConnection,
                LibraryRecoveryActionKeys.Diagnostics,
            ),
            actionKeys,
        )
        assertEquals(
            LibraryRecoveryActionKeys.Diagnostics,
            state.restore(
                screen = TvGridFocusPolicy.SCREEN_GRID,
                surface = TvGridFocusPolicy.SURFACE_STATUS_PANEL,
                availableItems = actionKeys,
                fallbackItem = LibraryRecoveryActionKeys.RetrySelected,
            ).target.item,
        )
    }

    @Test
    fun tvGridFocusRestoresTopControlsAroundControlPanels() {
        val panelState = TvFocusRestoreState()
            .record(TvFocusKey(TvGridFocusPolicy.SCREEN_GRID, TvGridFocusPolicy.SURFACE_STATUS_PANEL, "diagnostics"))
        val legacyRailState = TvFocusRestoreState()
            .record(TvFocusKey(TvGridFocusPolicy.SCREEN_GRID, TvGridFocusPolicy.SURFACE_MOVIE_FILTER, "filter-All"))

        assertEquals(
            TvGridFocusPolicy.SURFACE_STATUS_PANEL,
            TvGridFocusPolicy.preferredSurface(
                lastTarget = panelState.lastTarget(TvGridFocusPolicy.SCREEN_GRID),
                hasCards = true,
                openPanelSurface = TvGridFocusPolicy.SURFACE_STATUS_PANEL,
            ),
        )
        assertEquals(
            TvGridFocusPolicy.SURFACE_TOP_CONTROLS,
            TvGridFocusPolicy.preferredSurface(panelState.lastTarget(TvGridFocusPolicy.SCREEN_GRID), hasCards = true),
        )
        assertEquals(
            TvGridFocusPolicy.SURFACE_TOP_CONTROLS,
            TvGridFocusPolicy.preferredSurface(legacyRailState.lastTarget(TvGridFocusPolicy.SCREEN_GRID), hasCards = true),
        )
    }

    @Test
    fun tvSearchFocusKeysKeepCacheMissRowsRecoverableAndResolvedRowsUniqueByLibrary() {
        val sameMediaLibraryA = TvSearchFocusPolicy.resolvedRowKey("movie", "same-media", "library-a")
        val sameMediaLibraryB = TvSearchFocusPolicy.resolvedRowKey("movie", "same-media", "library-b")
        val missKey = TvSearchFocusPolicy.cacheMissRowKey("episode", "missing-episode")
        val missSurface = TvSearchFocusPolicy.cacheMissSurface(missKey)
        val retryAction = TvSearchFocusPolicy.cacheMissRetryAction(missKey)
        val diagnosticsAction = TvSearchFocusPolicy.cacheMissDiagnosticsAction(missKey)
        val state = TvFocusRestoreState()
            .record(TvFocusKey(TvSearchFocusPolicy.SCREEN_SEARCH, missSurface, retryAction))

        assertNotEquals(sameMediaLibraryA, sameMediaLibraryB)
        assertEquals("miss:episode:missing-episode", missKey)
        assertTrue(TvSearchFocusPolicy.shouldAutoFocusRecovery(missSurface))
        assertEquals(
            TvFocusKey(TvSearchFocusPolicy.SCREEN_SEARCH, missSurface, retryAction),
            state.restore(
                screen = TvSearchFocusPolicy.SCREEN_SEARCH,
                surface = missSurface,
                availableItems = listOf(retryAction, diagnosticsAction),
                fallbackItem = diagnosticsAction,
            ).target,
        )
    }

    @Test
    fun tvHomeInitialFocusPrefersContinueThenSearchThenLibraryThenRecovery() {
        assertEquals(
            TvFocusKey("home", "continue-watching", "continue-1"),
            TvHomeFocusPolicy.initialHomeTarget(
                continueWatchingKeys = listOf("continue-1"),
                searchAvailable = true,
                libraryActionKeys = listOf("browse-movies"),
                recoveryActionKeys = listOf("retry-cache-sync"),
            ),
        )
        assertEquals(
            TvFocusKey("home", "home-actions", "search"),
            TvHomeFocusPolicy.initialHomeTarget(
                continueWatchingKeys = emptyList(),
                searchAvailable = true,
                libraryActionKeys = listOf("browse-movies"),
                recoveryActionKeys = listOf("retry-cache-sync"),
            ),
        )
        assertEquals(
            TvFocusKey("home", "home-actions", "search"),
            TvHomeFocusPolicy.initialHomeTarget(
                continueWatchingKeys = emptyList(),
                searchAvailable = true,
                libraryActionKeys = listOf("browse-movies"),
                recoveryActionKeys = listOf("retry-cache-sync"),
                homeActionKeys = listOf("retry-connection", TvHomeFocusPolicy.ITEM_SEARCH, TvHomeFocusPolicy.ITEM_DIAGNOSTICS),
            ),
        )
        assertEquals(
            TvFocusKey("home", "home-actions", TvHomeFocusPolicy.ITEM_DIAGNOSTICS),
            TvHomeFocusPolicy.initialHomeTarget(
                continueWatchingKeys = emptyList(),
                searchAvailable = false,
                libraryActionKeys = listOf("browse-movies"),
                recoveryActionKeys = listOf("retry-cache-sync"),
                homeActionKeys = listOf(TvHomeFocusPolicy.ITEM_DIAGNOSTICS),
            ),
        )
        assertEquals(
            TvFocusKey("home", "library-actions", "browse-series"),
            TvHomeFocusPolicy.initialHomeTarget(
                continueWatchingKeys = emptyList(),
                searchAvailable = false,
                libraryActionKeys = listOf("browse-series"),
                recoveryActionKeys = listOf("retry-cache-sync"),
            ),
        )
        assertEquals(
            TvFocusKey("home", "recovery-actions", "reset-connection"),
            TvHomeFocusPolicy.initialHomeTarget(
                continueWatchingKeys = emptyList(),
                searchAvailable = false,
                libraryActionKeys = emptyList(),
                recoveryActionKeys = listOf("reset-connection"),
            ),
        )
    }
}
