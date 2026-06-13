package com.ferrex.android.navigation

import android.net.Uri
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
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.SearchDetailTarget
import com.ferrex.android.core.search.SearchMediaType
import com.ferrex.android.ui.recovery.PhoneHomeScreen
import com.ferrex.android.ui.recovery.PhoneLoadingScreen
import com.ferrex.android.ui.recovery.PhoneLoginScreen
import com.ferrex.android.ui.recovery.PhoneRecoverableScreen
import com.ferrex.android.ui.recovery.PhoneServerConnectScreen
import com.ferrex.android.ui.search.PhoneSearchDetailScreen
import kotlinx.coroutines.launch

object FerrexRoutes {
    const val LOADING = "loading"
    const val SERVER = "server"
    const val LOGIN = "login"
    const val RECOVERY = "recovery"
    const val HOME = "home"
    const val DETAIL = "detail/{mediaType}/{mediaId}?libraryId={libraryId}"

    fun detail(target: SearchDetailTarget): String {
        val libraryQuery = target.libraryId?.let { "?libraryId=${Uri.encode(it)}" }.orEmpty()
        return "detail/${Uri.encode(target.mediaType.routeSegment)}/${Uri.encode(target.mediaId)}$libraryQuery"
    }
}

@Composable
fun FerrexNavGraph(
    authManager: AuthManager,
    libraryRepository: LibraryRepository? = null,
    imageRepository: ImageRepository? = null,
    imagePipeline: FerrexImagePipeline? = null,
    searchRepository: MediaSearchRepository? = null,
    navController: NavHostController = rememberNavController(),
) {
    val sessionState by authManager.sessionState.collectAsState()
    val retryScope = rememberCoroutineScope()
    LaunchedEffect(Unit) {
        authManager.initialize()
    }

    NavHost(navController = navController, startDestination = FerrexRoutes.LOADING) {
        composable(FerrexRoutes.LOADING) { PhoneLoadingScreen() }
        composable(FerrexRoutes.SERVER) {
            PhoneServerConnectScreen(
                state = sessionState as? SessionState.NoServer ?: SessionState.NoServer(),
                onConnect = authManager::connectToServer,
                onResetConnection = authManager::resetConnection,
            )
        }
        composable(FerrexRoutes.LOGIN) {
            val state = sessionState as? SessionState.NeedsLogin
            if (state == null) {
                PhoneLoadingScreen()
            } else {
                PhoneLoginScreen(
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
                PhoneLoadingScreen()
            } else {
                PhoneRecoverableScreen(
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
                PhoneLoadingScreen()
            } else {
                PhoneHomeScreen(
                    state = state,
                    libraryRepository = libraryRepository,
                    imageRepository = imageRepository,
                    imagePipeline = imagePipeline,
                    searchRepository = searchRepository,
                    onOpenSearchResult = { target -> navController.navigate(FerrexRoutes.detail(target)) },
                    onSignOut = authManager::signOut,
                    onChangeServer = authManager::changeServer,
                    onResetConnection = authManager::resetConnection,
                )
            }
        }
        composable(FerrexRoutes.DETAIL) { backStackEntry ->
            val state = sessionState as? SessionState.Authenticated
            val mediaType = SearchMediaType.fromRouteSegment(backStackEntry.arguments?.getString("mediaType"))
            val mediaId = backStackEntry.arguments?.getString("mediaId")
            if (state == null || mediaType == null || mediaId.isNullOrBlank()) {
                PhoneLoadingScreen()
            } else {
                val scope = ServerCacheScope.from(state.serverUrl, state.user.id)
                PhoneSearchDetailScreen(
                    scope = scope,
                    mediaType = mediaType,
                    mediaId = mediaId,
                    libraryId = backStackEntry.arguments?.getString("libraryId")?.takeIf { it.isNotBlank() },
                    libraryRepository = libraryRepository,
                    imageRepository = imageRepository,
                    imagePipeline = imagePipeline,
                    onBack = { navController.popBackStack() },
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
