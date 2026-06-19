package com.ferrex.android.core.browse

import com.ferrex.android.core.auth.AuthConnectionHealth

enum class PhoneShellDestination(val label: String) {
    Home("Home"),
    Libraries("Libraries"),
    Search("Search"),
    AccountServer("Account"),
}

enum class PhoneSystemBackAction {
    CloseDiagnostics,
    ClosePlayback,
    CloseDetail,
    ReturnHome,
    ExitApp,
}

enum class PhoneExplicitBackAction {
    CloseDiagnostics,
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
        "Diagnostics explicit or system Back closes Diagnostics first; Home system Back exits the app; " +
            "Libraries, Search, and Account/Server system Back return to Home; " +
            "Detail explicit or system Back closes Detail and reveals its source surface; Playback explicit or system Back closes Playback before Detail; " +
            "Recovery surfaces keep retry, sign out, change server, reset connection, diagnostics, and cache recovery exits visible without Android app-data wipes."

    fun phoneSystemBackAction(
        hasActivePlayback: Boolean,
        hasSelectedDetail: Boolean,
        currentDestination: PhoneShellDestination = PhoneShellDestination.Home,
        diagnosticsOpen: Boolean = false,
        recoverySurfaceActive: Boolean = false,
    ): PhoneSystemBackAction = when {
        diagnosticsOpen -> PhoneSystemBackAction.CloseDiagnostics
        recoverySurfaceActive -> PhoneSystemBackAction.ExitApp
        hasActivePlayback -> PhoneSystemBackAction.ClosePlayback
        hasSelectedDetail -> PhoneSystemBackAction.CloseDetail
        currentDestination != PhoneShellDestination.Home -> PhoneSystemBackAction.ReturnHome
        else -> PhoneSystemBackAction.ExitApp
    }

    fun phoneExplicitBackAction(
        hasActivePlayback: Boolean,
        hasSelectedDetail: Boolean,
        diagnosticsOpen: Boolean = false,
    ): PhoneExplicitBackAction = when {
        diagnosticsOpen -> PhoneExplicitBackAction.CloseDiagnostics
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
