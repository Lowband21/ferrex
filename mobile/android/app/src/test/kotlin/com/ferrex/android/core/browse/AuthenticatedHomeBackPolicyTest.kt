package com.ferrex.android.core.browse

import com.ferrex.android.core.auth.AuthConnectionHealth
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthenticatedHomeBackPolicyTest {
    @Test
    fun phoneBackClosesPlaybackThenDetailThenReturnsTopLevelRoutesHomeBeforeExitingApp() {
        assertEquals(
            PhoneSystemBackAction.ClosePlayback,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(
                hasActivePlayback = true,
                hasSelectedDetail = true,
                currentDestination = PhoneShellDestination.Search,
            ),
        )
        assertEquals(
            PhoneSystemBackAction.CloseDetail,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(
                hasActivePlayback = false,
                hasSelectedDetail = true,
                currentDestination = PhoneShellDestination.Libraries,
            ),
        )
        assertEquals(
            PhoneSystemBackAction.ReturnHome,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(
                hasActivePlayback = false,
                hasSelectedDetail = false,
                currentDestination = PhoneShellDestination.Search,
            ),
        )
        assertEquals(
            PhoneSystemBackAction.ReturnHome,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(
                hasActivePlayback = false,
                hasSelectedDetail = false,
                currentDestination = PhoneShellDestination.AccountServer,
            ),
        )
        assertEquals(
            PhoneSystemBackAction.ExitApp,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(
                hasActivePlayback = false,
                hasSelectedDetail = false,
                currentDestination = PhoneShellDestination.Home,
            ),
        )
    }

    @Test
    fun explicitBackOnlyClosesDiagnosticsPlaybackOrDetailLayers() {
        assertEquals(
            PhoneExplicitBackAction.CloseDiagnostics,
            AuthenticatedHomeBackPolicy.phoneExplicitBackAction(
                hasActivePlayback = true,
                hasSelectedDetail = true,
                diagnosticsOpen = true,
            ),
        )
        assertEquals(
            PhoneExplicitBackAction.ClosePlayback,
            AuthenticatedHomeBackPolicy.phoneExplicitBackAction(hasActivePlayback = true, hasSelectedDetail = true),
        )
        assertEquals(
            PhoneExplicitBackAction.CloseDetail,
            AuthenticatedHomeBackPolicy.phoneExplicitBackAction(hasActivePlayback = false, hasSelectedDetail = true),
        )
        assertEquals(
            PhoneExplicitBackAction.StayOnSurface,
            AuthenticatedHomeBackPolicy.phoneExplicitBackAction(hasActivePlayback = false, hasSelectedDetail = false),
        )
    }

    @Test
    fun diagnosticsSystemBackClosesDiagnosticsBeforeAuthenticatedLayers() {
        assertEquals(
            PhoneSystemBackAction.CloseDiagnostics,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(
                hasActivePlayback = true,
                hasSelectedDetail = true,
                currentDestination = PhoneShellDestination.Search,
                diagnosticsOpen = true,
            ),
        )
    }

    @Test
    fun recoverySurfacesDoNotTrapSystemBackOrHideNoWipeRecoveryActions() {
        assertEquals(
            PhoneSystemBackAction.ExitApp,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(
                hasActivePlayback = false,
                hasSelectedDetail = false,
                recoverySurfaceActive = true,
            ),
        )

        val documentation = AuthenticatedHomeBackPolicy.PHONE_BACK_BEHAVIOR_DOCUMENTATION
        listOf("retry", "sign out", "change server", "reset connection", "diagnostics", "cache recovery").forEach { action ->
            assertTrue(documentation.contains(action, ignoreCase = true))
        }
        assertTrue(documentation.contains("without Android app-data wipes"))
    }

    @Test
    fun phoneBackDocumentationNamesEveryAuthenticatedSurface() {
        val documentation = AuthenticatedHomeBackPolicy.PHONE_BACK_BEHAVIOR_DOCUMENTATION
        listOf("Home", "Libraries", "Search", "Account/Server", "Detail", "Playback", "Diagnostics", "Recovery").forEach { surface ->
            assertTrue(documentation.contains(surface))
        }
    }

    @Test
    fun offlineDetailBackStaysInsideAuthenticatedReturnTarget() {
        assertEquals(
            AuthenticatedDetailBackDestination.Home,
            AuthenticatedHomeBackPolicy.detailBackDestination(
                AuthConnectionHealth.Offline,
                AuthenticatedDetailBackDestination.Home,
            ),
        )
        assertEquals(
            AuthenticatedDetailBackDestination.MovieGrid,
            AuthenticatedHomeBackPolicy.detailBackDestination(
                AuthConnectionHealth.Probing,
                AuthenticatedDetailBackDestination.MovieGrid,
            ),
        )
    }
}
