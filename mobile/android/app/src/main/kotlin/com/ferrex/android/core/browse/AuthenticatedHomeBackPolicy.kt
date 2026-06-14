package com.ferrex.android.core.browse

import com.ferrex.android.core.auth.AuthConnectionHealth

enum class PhoneShellDestination(val label: String) {
    Home("Home"),
    Libraries("Libraries"),
    Search("Search"),
    AccountServer("Account"),
}

enum class PhoneSystemBackAction {
    ClosePlayback,
    CloseDetail,
    ReturnHome,
    ExitApp,
}

enum class PhoneExplicitBackAction {
    ClosePlayback,
    CloseDetail,
    StayOnSurface,
}

enum class AuthenticatedDetailBackDestination {
    Home,
    Search,
    MovieGrid,
    SeriesGrid,
}

object AuthenticatedHomeBackPolicy {
    const val PHONE_BACK_BEHAVIOR_DOCUMENTATION: String =
        "Home system Back exits the app; Libraries, Search, and Account/Server system Back return to Home; " +
            "Detail explicit or system Back closes Detail and reveals its source surface; Playback explicit or system Back closes Playback before Detail."

    fun phoneSystemBackAction(
        hasActivePlayback: Boolean,
        hasSelectedDetail: Boolean,
        currentDestination: PhoneShellDestination = PhoneShellDestination.Home,
    ): PhoneSystemBackAction = when {
        hasActivePlayback -> PhoneSystemBackAction.ClosePlayback
        hasSelectedDetail -> PhoneSystemBackAction.CloseDetail
        currentDestination != PhoneShellDestination.Home -> PhoneSystemBackAction.ReturnHome
        else -> PhoneSystemBackAction.ExitApp
    }

    fun phoneExplicitBackAction(
        hasActivePlayback: Boolean,
        hasSelectedDetail: Boolean,
    ): PhoneExplicitBackAction = when {
        hasActivePlayback -> PhoneExplicitBackAction.ClosePlayback
        hasSelectedDetail -> PhoneExplicitBackAction.CloseDetail
        else -> PhoneExplicitBackAction.StayOnSurface
    }

    fun detailBackDestination(
        connectionHealth: AuthConnectionHealth,
        requestedDestination: AuthenticatedDetailBackDestination,
    ): AuthenticatedDetailBackDestination = when (connectionHealth) {
        AuthConnectionHealth.Offline,
        AuthConnectionHealth.Probing,
        AuthConnectionHealth.Online -> requestedDestination
    }
}
