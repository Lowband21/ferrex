package com.ferrex.android.core.playback

/**
 * Pure launch gate shared by the phone and TV shells before handing a route to
 * [PlaybackController]. The decision carries the original [PlaybackRouteContract]
 * unchanged so the controller receives the media-file target selected by detail
 * route construction rather than a logical movie/episode id.
 */
object PlaybackLaunchPolicy {
    const val MISSING_SUBSTRATE_MESSAGE = "Playback is unavailable because the ticketed Media3 substrate is not configured."

    fun phone(
        route: PlaybackRouteContract,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
        ticketTransportReady: Boolean,
        streamUrlFactoryReady: Boolean,
        streamingHttpClientReady: Boolean,
    ): PlaybackLaunchDecision = decide(
        route = route,
        networkActionsEnabled = networkActionsEnabled,
        networkActionMessage = networkActionMessage,
        ticketTransportReady = ticketTransportReady,
        streamUrlFactoryReady = streamUrlFactoryReady,
        streamingHttpClientReady = streamingHttpClientReady,
    )

    fun tv(
        route: PlaybackRouteContract,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
        ticketTransportReady: Boolean,
        streamUrlFactoryReady: Boolean,
        streamingHttpClientReady: Boolean,
    ): PlaybackLaunchDecision = decide(
        route = route,
        networkActionsEnabled = networkActionsEnabled,
        networkActionMessage = networkActionMessage,
        ticketTransportReady = ticketTransportReady,
        streamUrlFactoryReady = streamUrlFactoryReady,
        streamingHttpClientReady = streamingHttpClientReady,
    )

    private fun decide(
        route: PlaybackRouteContract,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
        ticketTransportReady: Boolean,
        streamUrlFactoryReady: Boolean,
        streamingHttpClientReady: Boolean,
    ): PlaybackLaunchDecision {
        if (!networkActionsEnabled) {
            return PlaybackLaunchDecision.Blocked(networkActionMessage)
        }
        if (!ticketTransportReady || !streamUrlFactoryReady || !streamingHttpClientReady) {
            return PlaybackLaunchDecision.Blocked(MISSING_SUBSTRATE_MESSAGE)
        }
        return PlaybackLaunchDecision.Launch(route)
    }
}

sealed interface PlaybackLaunchDecision {
    data class Launch(val route: PlaybackRouteContract) : PlaybackLaunchDecision
    data class Blocked(val message: String?) : PlaybackLaunchDecision
}
