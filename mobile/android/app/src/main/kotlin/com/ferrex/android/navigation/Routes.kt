package com.ferrex.android.navigation

import kotlinx.serialization.Serializable

/**
 * Type-safe route definitions for Navigation Compose.
 * Sealed interface hierarchy enables exhaustive when-matching.
 */
sealed interface Route {

    /** Server URL entry screen — first launch. */
    @Serializable
    data object ServerConnect : Route

    /** Username/password login. */
    @Serializable
    data object Login : Route

    /** Account registration (when server allows it). */
    @Serializable
    data object Register : Route

    /** Authenticated mobile landing route, presented as Resume / Continue Watching. */
    @Serializable
    data object Home : Route

    /** Library poster grid. */
    @Serializable
    data class Library(val libraryId: String) : Route

    /** Movie detail view. */
    @Serializable
    data class MovieDetail(val movieId: String) : Route

    /** Series detail view. */
    @Serializable
    data class SeriesDetail(val seriesId: String) : Route

    /** Season episode list. */
    @Serializable
    data class Season(val seriesId: String, val seasonNumber: Int) : Route

    /** Video player. */
    @Serializable
    data class Player(
        val mediaId: String,
        /**
         * Optional explicit start offset in milliseconds.
         * - null: let player resolve resume offset from watch progress.
         * - 0 or more: force playback from that exact position.
         */
        val startPositionMs: Long? = null,
    ) : Route

    /** Search screen. */
    @Serializable
    data object Search : Route
}
