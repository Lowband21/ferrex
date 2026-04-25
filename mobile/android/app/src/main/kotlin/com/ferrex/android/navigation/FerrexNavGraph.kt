package com.ferrex.android.navigation

import androidx.compose.animation.AnimatedContentTransitionScope
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
import com.ferrex.android.ui.auth.AuthViewModel
import com.ferrex.android.ui.auth.LoginScreen
import com.ferrex.android.ui.auth.ServerConnectScreen
import com.ferrex.android.ui.detail.DetailViewModel
import com.ferrex.android.ui.detail.MovieDetailScreen
import com.ferrex.android.ui.detail.SeriesDetailScreen
import com.ferrex.android.ui.home.HomeScreen
import com.ferrex.android.ui.home.HomeViewModel
import com.ferrex.android.ui.library.LibraryViewModel
import com.ferrex.android.ui.player.PlayerScreen
import com.ferrex.android.ui.player.PlayerViewModel
import com.ferrex.android.ui.search.SearchScreen
import com.ferrex.android.ui.search.SearchViewModel

private const val TRANSITION_DURATION = 300

@Composable
fun FerrexNavGraph() {
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
        enterTransition = {
            fadeIn(tween(TRANSITION_DURATION)) + slideIntoContainer(
                AnimatedContentTransitionScope.SlideDirection.Start,
                tween(TRANSITION_DURATION),
            )
        },
        exitTransition = {
            fadeOut(tween(TRANSITION_DURATION)) + slideOutOfContainer(
                AnimatedContentTransitionScope.SlideDirection.Start,
                tween(TRANSITION_DURATION),
            )
        },
        popEnterTransition = {
            fadeIn(tween(TRANSITION_DURATION)) + slideIntoContainer(
                AnimatedContentTransitionScope.SlideDirection.End,
                tween(TRANSITION_DURATION),
            )
        },
        popExitTransition = {
            fadeOut(tween(TRANSITION_DURATION)) + slideOutOfContainer(
                AnimatedContentTransitionScope.SlideDirection.End,
                tween(TRANSITION_DURATION),
            )
        },
    ) {
        // ── Auth flow ───────────────────────────────────────────

        composable<Route.ServerConnect>(
            enterTransition = { fadeIn(tween(TRANSITION_DURATION)) },
            exitTransition = { fadeOut(tween(TRANSITION_DURATION)) },
        ) {
            ServerConnectScreen(
                viewModel = authViewModel,
                onConnected = {
                    navController.navigate(Route.Login) {
                        popUpTo(Route.ServerConnect) { inclusive = true }
                    }
                },
            )
        }

        composable<Route.Login>(
            enterTransition = { fadeIn(tween(TRANSITION_DURATION)) },
            exitTransition = { fadeOut(tween(TRANSITION_DURATION)) },
        ) {
            LoginScreen(
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
            LoginScreen(
                viewModel = authViewModel,
                onLoginSuccess = {
                    navController.navigate(Route.Home) {
                        popUpTo(Route.Register) { inclusive = true }
                    }
                },
                onNavigateToRegister = {},
            )
        }

        // ── Main flow ───────────────────────────────────────────

        composable<Route.Home>(
            enterTransition = { fadeIn(tween(TRANSITION_DURATION)) },
            exitTransition = {
                fadeOut(tween(TRANSITION_DURATION)) + slideOutOfContainer(
                    AnimatedContentTransitionScope.SlideDirection.Start,
                    tween(TRANSITION_DURATION),
                )
            },
            popEnterTransition = {
                fadeIn(tween(TRANSITION_DURATION)) + slideIntoContainer(
                    AnimatedContentTransitionScope.SlideDirection.End,
                    tween(TRANSITION_DURATION),
                )
            },
        ) {
            val libraryViewModel: LibraryViewModel = hiltViewModel()
            val homeViewModel: HomeViewModel = hiltViewModel()
            HomeScreen(
                libraryViewModel = libraryViewModel,
                homeViewModel = homeViewModel,
                onMovieClick = { movieId ->
                    navController.navigate(Route.MovieDetail(movieId))
                },
                onSeriesClick = { seriesId ->
                    navController.navigate(Route.SeriesDetail(seriesId))
                },
                onSearchClick = {
                    navController.navigate(Route.Search)
                },
                onContinueWatchingClick = { mediaId ->
                    navController.navigate(Route.Player(mediaId = mediaId))
                },
            )
        }

        // ── Detail views ────────────────────────────────────────

        composable<Route.MovieDetail> {
            val detailViewModel: DetailViewModel = hiltViewModel()
            MovieDetailScreen(
                viewModel = detailViewModel,
                onBack = { navController.popBackStack() },
                onPlay = { mediaId, startPositionMs ->
                    navController.navigate(Route.Player(mediaId = mediaId, startPositionMs = startPositionMs))
                },
            )
        }

        composable<Route.SeriesDetail> {
            val detailViewModel: DetailViewModel = hiltViewModel()
            SeriesDetailScreen(
                viewModel = detailViewModel,
                onBack = { navController.popBackStack() },
                onEpisodeClick = { mediaId, startPositionMs ->
                    navController.navigate(Route.Player(mediaId = mediaId, startPositionMs = startPositionMs))
                },
            )
        }

        // ── Player ──────────────────────────────────────────────

        composable<Route.Player>(
            enterTransition = { fadeIn(tween(200)) },
            exitTransition = { fadeOut(tween(200)) },
            popEnterTransition = { fadeIn(tween(200)) },
            popExitTransition = { fadeOut(tween(200)) },
        ) {
            val playerViewModel: PlayerViewModel = hiltViewModel()
            PlayerScreen(
                viewModel = playerViewModel,
                okHttpClient = playerViewModel.streamingClient,
            )
        }

        // ── Search ──────────────────────────────────────────────

        composable<Route.Search> {
            val searchViewModel: SearchViewModel = hiltViewModel()
            SearchScreen(
                viewModel = searchViewModel,
                onBack = { navController.popBackStack() },
                onMovieClick = { movieId ->
                    navController.navigate(Route.MovieDetail(movieId))
                },
                onSeriesClick = { seriesId ->
                    navController.navigate(Route.SeriesDetail(seriesId))
                },
            )
        }
    }
}
