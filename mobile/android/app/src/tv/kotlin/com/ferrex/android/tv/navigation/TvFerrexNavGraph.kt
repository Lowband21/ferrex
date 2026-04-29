package com.ferrex.android.tv.navigation

import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.navigation.Route
import com.ferrex.android.ui.auth.AuthViewModel
import com.ferrex.android.ui.detail.DetailViewModel
import com.ferrex.android.ui.home.HomeViewModel
import com.ferrex.android.ui.library.LibraryViewModel
import com.ferrex.android.ui.player.PlayerViewModel
import com.ferrex.android.ui.search.SearchViewModel
import com.ferrex.android.ui.tv.TvHomeScreen
import com.ferrex.android.ui.tv.TvLoginScreen
import com.ferrex.android.ui.tv.TvMovieDetailScreen
import com.ferrex.android.ui.tv.TvPlayerScreen
import com.ferrex.android.ui.tv.TvSearchScreen
import com.ferrex.android.ui.tv.TvSeriesDetailScreen
import com.ferrex.android.ui.tv.TvServerConnectScreen

private const val TV_TRANSITION_MS = 180

@Composable
fun TvFerrexNavGraph() {
    val navController = rememberNavController()
    val authViewModel: AuthViewModel = hiltViewModel()
    val sessionState by authViewModel.sessionState.collectAsState()

    val startDestination: Route = when (sessionState) {
        is SessionState.NoServer -> Route.ServerConnect
        is SessionState.NeedsLogin -> Route.Login
        is SessionState.Authenticated -> Route.Home
        is SessionState.Loading -> Route.ServerConnect
    }

    NavHost(
        navController = navController,
        startDestination = startDestination,
        enterTransition = { fadeIn(tween(TV_TRANSITION_MS)) },
        exitTransition = { fadeOut(tween(TV_TRANSITION_MS)) },
        popEnterTransition = { fadeIn(tween(TV_TRANSITION_MS)) },
        popExitTransition = { fadeOut(tween(TV_TRANSITION_MS)) },
    ) {
        composable<Route.ServerConnect> {
            TvServerConnectScreen(
                viewModel = authViewModel,
                onConnected = {
                    navController.navigate(Route.Login) {
                        popUpTo(Route.ServerConnect) { inclusive = true }
                    }
                },
            )
        }

        composable<Route.Login> {
            TvLoginScreen(
                viewModel = authViewModel,
                onLoginSuccess = {
                    navController.navigate(Route.Home) {
                        popUpTo(Route.Login) { inclusive = true }
                    }
                },
                onNavigateToRegister = {
                    navController.navigate(Route.Register)
                },
            )
        }

        composable<Route.Register> {
            TvLoginScreen(
                viewModel = authViewModel,
                onLoginSuccess = {
                    navController.navigate(Route.Home) {
                        popUpTo(Route.Register) { inclusive = true }
                    }
                },
                onNavigateToRegister = {},
            )
        }

        composable<Route.Home> {
            val libraryViewModel: LibraryViewModel = hiltViewModel()
            val homeViewModel: HomeViewModel = hiltViewModel()
            TvHomeScreen(
                libraryViewModel = libraryViewModel,
                homeViewModel = homeViewModel,
                onSearchClick = {
                    navController.navigate(Route.Search)
                },
                onMovieClick = { movieId ->
                    navController.navigate(Route.MovieDetail(movieId))
                },
                onSeriesClick = { seriesId ->
                    navController.navigate(Route.SeriesDetail(seriesId))
                },
                onContinueWatchingClick = { mediaId ->
                    navController.navigate(Route.Player(mediaId = mediaId))
                },
            )
        }

        composable<Route.MovieDetail> {
            val detailViewModel: DetailViewModel = hiltViewModel()
            TvMovieDetailScreen(
                viewModel = detailViewModel,
                onBack = { navController.popBackStack() },
                onPlay = { mediaId, startPositionMs ->
                    navController.navigate(Route.Player(mediaId = mediaId, startPositionMs = startPositionMs))
                },
            )
        }

        composable<Route.SeriesDetail> {
            val detailViewModel: DetailViewModel = hiltViewModel()
            TvSeriesDetailScreen(
                viewModel = detailViewModel,
                onBack = { navController.popBackStack() },
                onEpisodeClick = { mediaId, startPositionMs ->
                    navController.navigate(Route.Player(mediaId = mediaId, startPositionMs = startPositionMs))
                },
            )
        }

        composable<Route.Search> {
            val searchViewModel: SearchViewModel = hiltViewModel()
            TvSearchScreen(
                viewModel = searchViewModel,
                onBack = { navController.popBackStack() },
                onMovieClick = { movieId -> navController.navigate(Route.MovieDetail(movieId)) },
                onSeriesClick = { seriesId -> navController.navigate(Route.SeriesDetail(seriesId)) },
            )
        }

        composable<Route.Player> {
            val playerViewModel: PlayerViewModel = hiltViewModel()
            TvPlayerScreen(
                viewModel = playerViewModel,
                okHttpClient = playerViewModel.streamingClient,
                onBack = { navController.popBackStack() },
            )
        }
    }
}
