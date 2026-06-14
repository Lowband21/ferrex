package com.ferrex.android.core.tvfocus

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
