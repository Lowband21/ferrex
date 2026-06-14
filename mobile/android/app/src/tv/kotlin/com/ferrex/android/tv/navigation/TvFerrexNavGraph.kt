package com.ferrex.android.tv.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.ferrex.android.core.auth.AuthManager
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.navigation.FerrexRoutes
import com.ferrex.android.tv.ui.TvHomeScreen
import com.ferrex.android.tv.ui.TvLoadingScreen
import com.ferrex.android.tv.ui.TvLoginScreen
import com.ferrex.android.tv.ui.TvRecoverableScreen
import com.ferrex.android.tv.ui.TvServerConnectScreen
import kotlinx.coroutines.launch

@Composable
fun TvFerrexNavGraph(
    authManager: AuthManager,
    libraryRepository: LibraryRepository? = null,
    imageRepository: ImageRepository? = null,
    imagePipeline: FerrexImagePipeline? = null,
    navController: NavHostController = rememberNavController(),
) {
    val sessionState by authManager.sessionState.collectAsState()
    val retryScope = rememberCoroutineScope()
    LaunchedEffect(Unit) {
        authManager.initialize()
    }

    NavHost(navController = navController, startDestination = FerrexRoutes.LOADING) {
        composable(FerrexRoutes.LOADING) { TvLoadingScreen() }
        composable(FerrexRoutes.SERVER) {
            TvServerConnectScreen(
                state = sessionState as? SessionState.NoServer ?: SessionState.NoServer(),
                onConnect = authManager::connectToServer,
                onResetConnection = authManager::resetConnection,
            )
        }
        composable(FerrexRoutes.LOGIN) {
            val state = sessionState as? SessionState.NeedsLogin
            if (state == null) {
                TvLoadingScreen()
            } else {
                TvLoginScreen(
                    state = state,
                    onLogin = authManager::loginWithPassword,
                    onRetry = { retryScope.launch { authManager.retryRestoredSession() } },
                    onSignOut = authManager::signOut,
                    onChangeServer = authManager::changeServer,
                    onResetConnection = authManager::resetConnection,
                )
            }
        }
        composable(FerrexRoutes.RECOVERY) {
            val state = sessionState as? SessionState.RecoverableFailure
            if (state == null) {
                TvLoadingScreen()
            } else {
                TvRecoverableScreen(
                    state = state,
                    onRetry = { retryScope.launch { authManager.retryRestoredSession() } },
                    onSignOut = authManager::signOut,
                    onChangeServer = authManager::changeServer,
                    onResetConnection = authManager::resetConnection,
                )
            }
        }
        composable(FerrexRoutes.HOME) {
            val state = sessionState as? SessionState.Authenticated
            if (state == null) {
                TvLoadingScreen()
            } else {
                TvHomeScreen(
                    state = state,
                    libraryRepository = libraryRepository,
                    imageRepository = imageRepository,
                    imagePipeline = imagePipeline,
                    onSignOut = authManager::signOut,
                    onChangeServer = authManager::changeServer,
                    onResetConnection = authManager::resetConnection,
                )
            }
        }
    }

    LaunchedEffect(sessionState) {
        navController.navigate(sessionState.routeName()) {
            popUpTo(FerrexRoutes.LOADING) { inclusive = false }
            launchSingleTop = true
        }
    }
}

private fun SessionState.routeName(): String = when (this) {
    SessionState.Loading -> FerrexRoutes.LOADING
    is SessionState.NoServer -> FerrexRoutes.SERVER
    is SessionState.NeedsLogin -> FerrexRoutes.LOGIN
    is SessionState.RecoverableFailure -> FerrexRoutes.RECOVERY
    is SessionState.Authenticated -> FerrexRoutes.HOME
}
