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
    fun explicitBackOnlyClosesPlaybackOrDetailLayers() {
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
    fun phoneBackDocumentationNamesEveryAuthenticatedSurface() {
        val documentation = AuthenticatedHomeBackPolicy.PHONE_BACK_BEHAVIOR_DOCUMENTATION
        listOf("Home", "Libraries", "Search", "Account/Server", "Detail", "Playback").forEach { surface ->
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
