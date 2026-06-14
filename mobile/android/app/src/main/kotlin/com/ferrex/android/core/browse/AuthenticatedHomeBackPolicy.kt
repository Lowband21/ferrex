package com.ferrex.android.core.browse

import com.ferrex.android.core.auth.AuthConnectionHealth

enum class PhoneSystemBackAction {
    ClosePlayback,
    CloseDetail,
    ExitApp,
}

enum class AuthenticatedDetailBackDestination {
    Home,
    Search,
    MovieGrid,
    SeriesGrid,
}

object AuthenticatedHomeBackPolicy {
    fun phoneSystemBackAction(
        hasActivePlayback: Boolean,
        hasSelectedDetail: Boolean,
    ): PhoneSystemBackAction = when {
        hasActivePlayback -> PhoneSystemBackAction.ClosePlayback
        hasSelectedDetail -> PhoneSystemBackAction.CloseDetail
        else -> PhoneSystemBackAction.ExitApp
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
