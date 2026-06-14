package com.ferrex.android.ui.player

import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.Tracks
import androidx.media3.datasource.HttpDataSource
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.trackselection.DefaultTrackSelector
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.ui.AspectRatioFrameLayout
import androidx.media3.ui.PlayerView
import com.ferrex.android.core.playback.Media3PlaybackDiagnostics
import com.ferrex.android.core.playback.PlaybackController
import com.ferrex.android.core.playback.PlaybackDiagnosticLog
import com.ferrex.android.core.playback.PlaybackFailure
import com.ferrex.android.core.playback.PlaybackFailureMapper
import com.ferrex.android.core.playback.PlaybackFailureKind
import com.ferrex.android.core.playback.PlaybackPlayerState
import com.ferrex.android.core.playback.PlaybackProgressReporter
import com.ferrex.android.core.playback.PlaybackRecoveryActions
import com.ferrex.android.core.playback.PlaybackResumeProgressProvider
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.PlaybackTicketTransport
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient

private const val PLAYER_TAG = "PlayerScreen"

private enum class AspectRatioMode(
    val label: String,
    @androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
    val resizeMode: Int,
) {
    @androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
    Fit("Fit", AspectRatioFrameLayout.RESIZE_MODE_FIT),

    @androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
    Fill("Fill", AspectRatioFrameLayout.RESIZE_MODE_FILL),

    @androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
    Zoom("Zoom", AspectRatioFrameLayout.RESIZE_MODE_ZOOM),
}

@Composable
fun PlayerScreen(
    route: PlaybackRouteContract,
    ticketTransport: PlaybackTicketTransport,
    streamUrlFactory: PlaybackStreamUrlFactory,
    progressReporter: PlaybackProgressReporter?,
    resumeProgressProvider: PlaybackResumeProgressProvider?,
    streamingHttpClient: OkHttpClient,
    onBack: () -> Unit,
    onSessionInvalidated: (PlaybackFailure) -> Unit,
    onProgressCommitted: () -> Unit,
    onChangeServer: () -> Unit,
    onSignOut: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val currentSessionInvalidated by rememberUpdatedState(onSessionInvalidated)
    val currentProgressCommitted by rememberUpdatedState(onProgressCommitted)
    val controller = remember(route, ticketTransport, streamUrlFactory, progressReporter, resumeProgressProvider, scope) {
        PlaybackController(
            route = route,
            ticketTransport = ticketTransport,
            streamUrlFactory = streamUrlFactory,
            progressReporter = progressReporter,
            resumeProgressProvider = resumeProgressProvider,
            scope = scope,
            onSessionInvalidated = { currentSessionInvalidated(it) },
            onProgressCommitted = { currentProgressCommitted() },
        )
    }
    val playerState by controller.state.collectAsState()

    LaunchedEffect(controller) {
        controller.prepare()
    }
    DisposableEffect(controller) {
        onDispose { controller.close() }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black),
        contentAlignment = Alignment.Center,
    ) {
        when (val state = playerState) {
            PlaybackPlayerState.Idle -> PlaybackLoading(message = "Preparing playback…")
            is PlaybackPlayerState.Loading -> PlaybackLoading(
                message = if (state.retryAttempt > 0) {
                    "${state.message} (${state.retryAttempt}/${state.maxRetryAttempts})"
                } else {
                    state.message
                },
            )
            is PlaybackPlayerState.Ready -> PlayerContent(
                streamUrl = state.prepared.streamUrl,
                startPositionMs = state.prepared.startPositionMs,
                streamingHttpClient = streamingHttpClient,
                controller = controller,
            )
            is PlaybackPlayerState.Error -> PlaybackErrorPanel(
                failure = state.failure,
                actions = state.actions,
                onRetry = controller::retry,
                onBack = onBack,
                onChangeServer = onChangeServer,
                onSignOut = onSignOut,
            )
            is PlaybackPlayerState.SessionInvalidated -> PlaybackErrorPanel(
                failure = state.failure.copy(message = "Playback authorization could not be recovered. Sign in again to continue."),
                actions = PlaybackRecoveryActions(retry = false, changeServer = true, signOut = true),
                onRetry = controller::retry,
                onBack = onBack,
                onChangeServer = onChangeServer,
                onSignOut = onSignOut,
            )
        }
    }
}

@Composable
private fun PlaybackLoading(message: String) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        CircularProgressIndicator(color = Color.White)
        Text(
            text = message,
            color = Color.White,
            style = MaterialTheme.typography.titleMedium,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun PlaybackErrorPanel(
    failure: PlaybackFailure,
    actions: PlaybackRecoveryActions,
    onRetry: () -> Unit,
    onBack: () -> Unit,
    onChangeServer: () -> Unit,
    onSignOut: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(32.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("Playback unavailable", style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.primary)
            Text(failure.message, textAlign = TextAlign.Center)
            failure.httpStatusCode?.let { Text("HTTP $it", style = MaterialTheme.typography.bodySmall) }
            if (actions.retry) {
                Button(onClick = onRetry, modifier = Modifier.fillMaxWidth()) { Text("Retry playback") }
            }
            OutlinedButton(onClick = onBack, modifier = Modifier.fillMaxWidth()) { Text("Back to details") }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (actions.changeServer) {
                    TextButton(onClick = onChangeServer, modifier = Modifier.weight(1f)) { Text("Change server") }
                }
                if (actions.signOut) {
                    TextButton(onClick = onSignOut, modifier = Modifier.weight(1f)) { Text("Sign out") }
                }
            }
        }
    }
}

@Composable
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
private fun PlayerContent(
    streamUrl: String,
    startPositionMs: Long,
    streamingHttpClient: OkHttpClient,
    controller: PlaybackController,
) {
    val context = LocalContext.current
    var aspectRatioMode by remember { mutableStateOf(AspectRatioMode.Fit) }
    var finalizedPlayback by remember(streamUrl) { mutableStateOf(false) }
    var audioTrackWarning by remember(streamUrl) { mutableStateOf<String?>(null) }
    val currentController by rememberUpdatedState(controller)

    val trackSelector = remember(context) {
        DefaultTrackSelector(context).apply {
            setParameters(
                buildUponParameters()
                    .setConstrainAudioChannelCountToDeviceCapabilities(false)
                    .setAllowInvalidateSelectionsOnRendererCapabilitiesChange(true)
                    .build(),
            )
        }
    }

    val exoPlayer = remember(context, streamUrl, streamingHttpClient, trackSelector) {
        val dataSourceFactory = OkHttpDataSource.Factory(streamingHttpClient)
            .setUserAgent("Ferrex-Android/0.1")
        val mediaSourceFactory = DefaultMediaSourceFactory(dataSourceFactory)
            .setLoadErrorHandlingPolicy(DefaultLoadErrorHandlingPolicy(6))
        val loadControl = DefaultLoadControl.Builder()
            .setBufferDurationsMs(
                15_000,
                45_000,
                2_500,
                5_000,
            )
            .setTargetBufferBytes(48 * 1024 * 1024)
            .build()

        PlaybackDiagnosticLog.info(PLAYER_TAG, "Creating ExoPlayer for ${PlaybackDiagnosticLog.redact(streamUrl)}")
        ExoPlayer.Builder(context)
            .setTrackSelector(trackSelector)
            .setMediaSourceFactory(mediaSourceFactory)
            .setLoadControl(loadControl)
            .build()
            .also { player -> player.addAnalyticsListener(Media3PlaybackDiagnostics()) }
    }

    DisposableEffect(exoPlayer, streamUrl, startPositionMs) {
        val listener = object : Player.Listener {
            override fun onPlaybackStateChanged(playbackState: Int) {
                when (playbackState) {
                    Player.STATE_READY -> currentController.onPlaybackReady()
                    Player.STATE_ENDED -> {
                        val duration = exoPlayer.duration
                        if (duration > 0 && duration != C.TIME_UNSET) {
                            finalizedPlayback = true
                            currentController.onPlaybackEnded(duration)
                        }
                    }
                    else -> Unit
                }
            }

            override fun onIsPlayingChanged(isPlaying: Boolean) {
                if (!isPlaying && exoPlayer.playbackState != Player.STATE_ENDED) {
                    val duration = exoPlayer.duration
                    if (duration > 0 && duration != C.TIME_UNSET) {
                        currentController.reportProgress(exoPlayer.currentPosition, duration)
                    }
                }
            }

            override fun onTracksChanged(tracks: Tracks) {
                audioTrackWarning = tracks.unsupportedAudioWarning()
            }

            override fun onPlayerError(error: PlaybackException) {
                val lastPosition = exoPlayer.currentPosition.coerceAtLeast(0L)
                currentController.onPlayerError(error.toPlaybackFailure(), lastPosition)
            }
        }

        exoPlayer.addListener(listener)
        exoPlayer.setMediaItem(MediaItem.fromUri(Uri.parse(streamUrl)))
        exoPlayer.prepare()
        if (startPositionMs > 0L) {
            exoPlayer.seekTo(startPositionMs)
        }
        exoPlayer.playWhenReady = true

        onDispose {
            val duration = runCatching { exoPlayer.duration }.getOrDefault(C.TIME_UNSET)
            if (!finalizedPlayback && duration > 0 && duration != C.TIME_UNSET) {
                val position = runCatching { exoPlayer.currentPosition }.getOrDefault(0L)
                currentController.onPlaybackExit(position, duration)
            }
            exoPlayer.removeListener(listener)
            exoPlayer.release()
            PlaybackDiagnosticLog.info(PLAYER_TAG, "Released ExoPlayer for ${PlaybackDiagnosticLog.redact(streamUrl)}")
        }
    }

    LaunchedEffect(exoPlayer) {
        while (true) {
            delay(10_000)
            try {
                if (exoPlayer.isPlaying) {
                    val duration = exoPlayer.duration
                    if (duration > 0 && duration != C.TIME_UNSET) {
                        currentController.reportProgress(exoPlayer.currentPosition, duration)
                    }
                }
            } catch (_: IllegalStateException) {
                break
            }
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        AndroidView(
            factory = { ctx ->
                PlayerView(ctx).apply {
                    player = exoPlayer
                    useController = true
                    setShowSubtitleButton(true)
                }
            },
            update = { view ->
                view.player = exoPlayer
                view.resizeMode = aspectRatioMode.resizeMode
            },
            modifier = Modifier.fillMaxSize(),
        )

        audioTrackWarning?.let { warning ->
            Surface(
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .padding(top = 16.dp, start = 24.dp, end = 24.dp),
                shape = RoundedCornerShape(4.dp),
                color = Color.Black.copy(alpha = 0.72f),
                contentColor = Color.White,
            ) {
                Text(warning, modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
            }
        }

        Surface(
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(12.dp),
            onClick = {
                val modes = AspectRatioMode.entries
                aspectRatioMode = modes[(aspectRatioMode.ordinal + 1) % modes.size]
            },
            shape = RoundedCornerShape(4.dp),
            color = Color.Black.copy(alpha = 0.6f),
            contentColor = Color.White,
        ) {
            Text(
                text = aspectRatioMode.label.uppercase(),
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                style = MaterialTheme.typography.labelMedium,
            )
        }
    }
}

private fun Tracks.unsupportedAudioWarning(): String? {
    val audioGroups = groups.filter { it.type == C.TRACK_TYPE_AUDIO }
    val audioTrackCount = audioGroups.sumOf { it.length }
    if (audioTrackCount == 0) return null

    val supportedAudioTrackCount = audioGroups.sumOf { group ->
        (0 until group.length).count { group.isTrackSupported(it) }
    }
    if (supportedAudioTrackCount > 0) return null

    return "Audio tracks were found, but this device cannot fully support them."
}

private fun PlaybackException.toPlaybackFailure(): PlaybackFailure {
    val httpError = findCause<HttpDataSource.InvalidResponseCodeException>()
    if (httpError != null) {
        return PlaybackFailureMapper.fromHttpStatus(httpError.responseCode, httpError.responseMessage)
    }

    return when (errorCode) {
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED -> PlaybackFailureMapper.network("Network connection failed while streaming.")
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT -> PlaybackFailureMapper.timeout("The stream connection timed out.")
        PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS -> PlaybackFailureMapper.unknown("The server rejected the stream request.")
        PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND -> PlaybackFailureMapper.fromHttpStatus(404)
        PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED -> PlaybackFailureMapper.unsupported()
        PlaybackException.ERROR_CODE_DECODER_INIT_FAILED -> PlaybackFailureMapper.decoder()
        else -> PlaybackFailure(
            kind = PlaybackFailureKind.Unknown,
            message = PlaybackDiagnosticLog.redact(message ?: "Playback failed unexpectedly."),
            autoRetryable = false,
        )
    }
}

private inline fun <reified T : Throwable> Throwable.findCause(): T? {
    var current: Throwable? = this
    while (current != null) {
        if (current is T) return current
        current = current.cause
    }
    return null
}
