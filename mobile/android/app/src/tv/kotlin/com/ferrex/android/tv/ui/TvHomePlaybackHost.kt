package com.ferrex.android.tv.ui

import androidx.compose.runtime.Composable
import com.ferrex.android.core.playback.PlaybackProgressReporter
import com.ferrex.android.core.playback.PlaybackResumeProgressProvider
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.PlaybackTicketTransport
import com.ferrex.android.ui.player.PlayerChrome
import com.ferrex.android.ui.player.PlayerScreen
import okhttp3.OkHttpClient

@Composable
internal fun TvHomePlaybackHost(
    playbackContract: PlaybackRouteContract?,
    playbackTicketTransport: PlaybackTicketTransport?,
    playbackStreamUrlFactory: PlaybackStreamUrlFactory?,
    playbackProgressReporter: PlaybackProgressReporter?,
    playbackResumeProgressProvider: PlaybackResumeProgressProvider?,
    streamingHttpClient: OkHttpClient?,
    onBack: () -> Unit,
    onSessionInvalidated: () -> Unit,
    onProgressCommitted: (PlaybackRouteContract) -> Unit,
    onChangeServer: () -> Unit,
    onSignOut: () -> Unit,
    onOpenDiagnostics: () -> Unit,
): Boolean {
    if (
        playbackContract == null ||
        playbackTicketTransport == null ||
        playbackStreamUrlFactory == null ||
        streamingHttpClient == null
    ) {
        return false
    }

    PlayerScreen(
        route = playbackContract,
        ticketTransport = playbackTicketTransport,
        streamUrlFactory = playbackStreamUrlFactory,
        progressReporter = playbackProgressReporter,
        resumeProgressProvider = playbackResumeProgressProvider,
        streamingHttpClient = streamingHttpClient,
        chrome = PlayerChrome.Tv,
        onBack = onBack,
        onSessionInvalidated = { onSessionInvalidated() },
        onProgressCommitted = { onProgressCommitted(playbackContract) },
        onChangeServer = onChangeServer,
        onSignOut = onSignOut,
        onOpenDiagnostics = onOpenDiagnostics,
    )
    return true
}
