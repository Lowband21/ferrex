package com.ferrex.android.tv.navigation

import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import com.ferrex.android.core.auth.AuthManager
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.browse.LibraryIndexTransport
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.playback.PlaybackProgressReporter
import com.ferrex.android.core.playback.PlaybackResumeProgressProvider
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.PlaybackTicketTransport
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.tvfocus.TvAuthRecoveryPolicy
import com.ferrex.android.core.watch.ContinueWatchingRepository
import com.ferrex.android.core.watch.WatchRepository
import com.ferrex.android.core.watch.WatchStateInvalidationBus
import com.ferrex.android.tv.ui.TvHomeScreen
import com.ferrex.android.tv.ui.TvLoadingScreen
import com.ferrex.android.tv.ui.TvLoginScreen
import com.ferrex.android.tv.ui.TvRecoverableScreen
import com.ferrex.android.tv.ui.TvServerConnectScreen
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient

@Composable
fun TvFerrexNavGraph(
    authManager: AuthManager,
    libraryRepository: LibraryRepository? = null,
    libraryIndexTransport: LibraryIndexTransport? = null,
    imageRepository: ImageRepository? = null,
    imagePipeline: FerrexImagePipeline? = null,
    searchRepository: MediaSearchRepository? = null,
    continueWatchingRepository: ContinueWatchingRepository? = null,
    watchRepository: WatchRepository? = null,
    watchStateInvalidationBus: WatchStateInvalidationBus? = null,
    playbackTicketTransport: PlaybackTicketTransport? = null,
    playbackStreamUrlFactory: PlaybackStreamUrlFactory? = null,
    playbackProgressReporter: PlaybackProgressReporter? = null,
    playbackResumeProgressProvider: PlaybackResumeProgressProvider? = null,
    streamingHttpClient: OkHttpClient? = null,
) {
    val sessionState by authManager.sessionState.collectAsState()
    val retryScope = rememberCoroutineScope()
    LaunchedEffect(Unit) {
        authManager.initialize()
    }

    BackHandler(enabled = TvAuthRecoveryPolicy.consumesBack(sessionState)) {
        // Auth and recovery screens intentionally stay on their current state-driven route.
        // Recovery actions provide explicit Sign out, Change server, and Reset connection exits.
    }

    when (val state = sessionState) {
        SessionState.Loading -> TvLoadingScreen()
        is SessionState.NoServer -> TvServerConnectScreen(
            state = state,
            onConnect = authManager::connectToServer,
            onResetConnection = authManager::resetConnection,
        )
        is SessionState.NeedsLogin -> TvLoginScreen(
            state = state,
            onLogin = authManager::loginWithPassword,
            onRetry = { retryScope.launch { authManager.retryRestoredSession() } },
            onSignOut = authManager::signOut,
            onChangeServer = authManager::changeServer,
            onResetConnection = authManager::resetConnection,
        )
        is SessionState.RecoverableFailure -> TvRecoverableScreen(
            state = state,
            onRetry = { retryScope.launch { authManager.retryRestoredSession() } },
            onSignOut = authManager::signOut,
            onChangeServer = authManager::changeServer,
            onResetConnection = authManager::resetConnection,
        )
        is SessionState.Authenticated -> TvHomeScreen(
            state = state,
            libraryRepository = libraryRepository,
            libraryIndexTransport = libraryIndexTransport,
            imageRepository = imageRepository,
            imagePipeline = imagePipeline,
            searchRepository = searchRepository,
            continueWatchingRepository = continueWatchingRepository,
            watchRepository = watchRepository,
            watchStateInvalidationBus = watchStateInvalidationBus,
            playbackTicketTransport = playbackTicketTransport,
            playbackStreamUrlFactory = playbackStreamUrlFactory,
            playbackProgressReporter = playbackProgressReporter,
            playbackResumeProgressProvider = playbackResumeProgressProvider,
            streamingHttpClient = streamingHttpClient,
            onSignOut = authManager::signOut,
            onChangeServer = authManager::changeServer,
            onResetConnection = authManager::resetConnection,
            onPlaybackSessionInvalidated = authManager::invalidateSessionFromPlayback,
        )
    }
}
