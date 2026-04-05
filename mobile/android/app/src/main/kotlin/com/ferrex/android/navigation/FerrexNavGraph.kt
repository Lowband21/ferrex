package com.ferrex.android.navigation

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
import com.ferrex.android.ui.library.LibraryViewModel
import com.ferrex.android.ui.player.PlayerScreen
import com.ferrex.android.ui.player.PlayerViewModel
import com.ferrex.android.ui.search.SearchScreen
import com.ferrex.android.ui.search.SearchViewModel
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
    ) {
        // ── Auth flow ───────────────────────────────────────────

        composable<Route.ServerConnect> {
            ServerConnectScreen(
                viewModel = authViewModel,
                onConnected = {
                    navController.navigate(Route.Login) {
                        popUpTo(Route.ServerConnect) { inclusive = true }
                    }
                },
            )
        }

        composable<Route.Login> {
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
            // Registration screen — deferred, use login for now
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

        composable<Route.Home> {
            val libraryViewModel: LibraryViewModel = hiltViewModel()
            HomeScreen(
                libraryViewModel = libraryViewModel,
                onMovieClick = { movieId ->
                    navController.navigate(Route.MovieDetail(movieId))
                },
                onSearchClick = {
                    navController.navigate(Route.Search)
                },
            )
        }

        // ── Detail views ────────────────────────────────────────

        composable<Route.MovieDetail> {
            val detailViewModel: DetailViewModel = hiltViewModel()
            MovieDetailScreen(
                viewModel = detailViewModel,
                onBack = { navController.popBackStack() },
                onPlay = { mediaId ->
                    navController.navigate(Route.Player(mediaId))
                },
            )
        }

        composable<Route.SeriesDetail> {
            val detailViewModel: DetailViewModel = hiltViewModel()
            SeriesDetailScreen(
                viewModel = detailViewModel,
                onBack = { navController.popBackStack() },
                onEpisodeClick = { mediaId ->
                    navController.navigate(Route.Player(mediaId))
                },
            )
        }

        // ── Player ──────────────────────────────────────────────

        composable<Route.Player> {
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
            )
        }
    }
}
