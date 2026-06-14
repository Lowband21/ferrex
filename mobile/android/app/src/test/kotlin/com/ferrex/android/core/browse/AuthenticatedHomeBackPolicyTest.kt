package com.ferrex.android.core.browse

import com.ferrex.android.core.auth.AuthConnectionHealth
import org.junit.Assert.assertEquals
import org.junit.Test

class AuthenticatedHomeBackPolicyTest {
    @Test
    fun phoneBackClosesPlaybackThenDetailBeforeExitingApp() {
        assertEquals(
            PhoneSystemBackAction.ClosePlayback,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(hasActivePlayback = true, hasSelectedDetail = true),
        )
        assertEquals(
            PhoneSystemBackAction.CloseDetail,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(hasActivePlayback = false, hasSelectedDetail = true),
        )
        assertEquals(
            PhoneSystemBackAction.ExitApp,
            AuthenticatedHomeBackPolicy.phoneSystemBackAction(hasActivePlayback = false, hasSelectedDetail = false),
        )
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
