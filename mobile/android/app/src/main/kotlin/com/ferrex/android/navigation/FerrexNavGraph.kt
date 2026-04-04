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

@Composable
fun FerrexNavGraph() {
    val navController = rememberNavController()
    val authViewModel: AuthViewModel = hiltViewModel()
    val sessionState by authViewModel.sessionState.collectAsState()

    // Determine start destination based on session state
    val startDestination: Route = when (sessionState) {
        is SessionState.NoServer -> Route.ServerConnect
        is SessionState.NeedsLogin -> Route.Login
        is SessionState.Authenticated -> Route.Home
        is SessionState.Loading -> Route.ServerConnect // Show connect while checking
    }

    NavHost(
        navController = navController,
        startDestination = startDestination,
    ) {
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

        composable<Route.Home> {
            // Placeholder — Phase 3
            androidx.compose.material3.Text("Home — coming in Phase 3")
        }
    }
}
