package com.ferrex.android.tv

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import com.ferrex.android.core.di.AndroidAuthDependencies
import com.ferrex.android.core.diagnostics.AndroidDisplayDiagnostics
import com.ferrex.android.tv.navigation.TvFerrexNavGraph
import com.ferrex.android.ui.theme.FerrexTheme

class TvMainActivity : ComponentActivity() {
    private val dependencies by lazy {
        AndroidAuthDependencies(
            context = applicationContext,
            deviceName = "Ferrex Android TV",
        )
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        AndroidDisplayDiagnostics.logCurrentDisplay(this, window)
        setContent {
            FerrexTheme(tv = true) {
                TvFerrexNavGraph(
                    authManager = dependencies.authManager,
                    libraryRepository = dependencies.libraryRepository,
                    libraryIndexTransport = dependencies.libraryIndexTransport,
                    imageRepository = dependencies.imageRepository,
                    imagePipeline = dependencies.imagePipeline,
                    searchRepository = dependencies.searchRepository,
                    continueWatchingRepository = dependencies.continueWatchingRepository,
                    watchRepository = dependencies.watchRepository,
                    watchStateInvalidationBus = dependencies.watchStateInvalidationBus,
                    playbackTicketTransport = dependencies.playbackTicketTransport,
                    playbackStreamUrlFactory = dependencies.playbackStreamUrlFactory,
                    playbackProgressReporter = dependencies.playbackProgressReporter,
                    playbackResumeProgressProvider = dependencies.playbackResumeProgressProvider,
                    streamingHttpClient = dependencies.streamingHttpClient,
                    diagnostics = dependencies.diagnostics,
                )
            }
        }
    }
}
