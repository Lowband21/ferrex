package com.ferrex.android.tv.ui

import com.ferrex.android.core.auth.AuthConnectionHealth
import com.ferrex.android.core.browse.AuthenticatedDetailBackDestination
import com.ferrex.android.core.browse.AuthenticatedHomeBackPolicy
import com.ferrex.android.core.browse.HomeLibraryTab
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.browse.MovieFilterMode
import com.ferrex.android.core.browse.MovieSortMode

internal fun TvReturnTarget.toChild(connectionHealth: AuthConnectionHealth): TvHomeChild? = when (
    AuthenticatedHomeBackPolicy.detailBackDestination(connectionHealth, toDetailBackDestination())
) {
    AuthenticatedDetailBackDestination.Home -> null
    AuthenticatedDetailBackDestination.Search -> TvHomeChild.Search
    AuthenticatedDetailBackDestination.MovieGrid -> TvHomeChild.Grid(HomeLibraryTab.Movies)
    AuthenticatedDetailBackDestination.SeriesGrid -> TvHomeChild.Grid(HomeLibraryTab.Series)
}

internal fun TvReturnTarget.toDetailBackDestination(): AuthenticatedDetailBackDestination = when (this) {
    TvReturnTarget.Home -> AuthenticatedDetailBackDestination.Home
    TvReturnTarget.Search -> AuthenticatedDetailBackDestination.Search
    is TvReturnTarget.Grid -> when (tab) {
        HomeLibraryTab.Movies -> AuthenticatedDetailBackDestination.MovieGrid
        HomeLibraryTab.Series -> AuthenticatedDetailBackDestination.SeriesGrid
    }
}

internal sealed interface TvHomeChild {
    data object Search : TvHomeChild
    data class Grid(val tab: HomeLibraryTab) : TvHomeChild
    data class Detail(val route: MediaRouteArgs, val returnTo: TvReturnTarget) : TvHomeChild
}

internal sealed interface TvReturnTarget {
    data object Home : TvReturnTarget
    data object Search : TvReturnTarget
    data class Grid(val tab: HomeLibraryTab) : TvReturnTarget
}

internal sealed interface MovieIndexUiState {
    data object Idle : MovieIndexUiState
    data object Loading : MovieIndexUiState
    data class Applied(
        val indices: List<Int>,
        val filterMode: MovieFilterMode,
        val sortMode: MovieSortMode,
    ) : MovieIndexUiState
    data class Unsupported(val message: String) : MovieIndexUiState
    data class Error(val message: String) : MovieIndexUiState
    data class Unavailable(val message: String) : MovieIndexUiState
}

internal const val GRID_IMAGE_LOOKUP_LIMIT = 96
