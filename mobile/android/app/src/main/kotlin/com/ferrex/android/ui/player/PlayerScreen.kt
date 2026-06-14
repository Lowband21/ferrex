@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package com.ferrex.android.ui.player

import android.net.Uri
import android.view.View
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
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
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
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
import com.ferrex.android.core.playback.PlaybackFailureKind
import com.ferrex.android.core.playback.PlaybackFailureMapper
import com.ferrex.android.core.playback.PlaybackPlayerState
import com.ferrex.android.core.playback.PlaybackProgressReporter
import com.ferrex.android.core.playback.PlaybackRecoveryActions
import com.ferrex.android.core.playback.PlaybackResumeProgressProvider
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.PlaybackTicketTransport
import com.ferrex.android.core.playback.PlaybackTrackOption
import com.ferrex.android.core.playback.PlaybackTrackOptions
import com.ferrex.android.core.playback.TvPlaybackOverlayEffect
import com.ferrex.android.core.playback.TvPlaybackOverlayEvent
import com.ferrex.android.core.playback.TvPlaybackOverlayReducer
import com.ferrex.android.core.playback.TvPlaybackOverlayUiState
import com.ferrex.android.core.playback.TvTrackPickerKind
import com.ferrex.android.core.playback.toPlaybackTrackGroupSnapshots
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.colors
import com.ferrex.android.ui.components.statusTone
import com.ferrex.android.ui.theme.FerrexDesignTokens
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient

private const val PLAYER_TAG = "PlayerScreen"
private const val TV_PLAYER_TAG = "TvPlayerOverlay"
private const val SEEK_BACK_MS = 10_000L
private const val SEEK_FORWARD_MS = 30_000L
private const val TV_AUTO_HIDE_MS = 5_000L

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

enum class PlayerChrome {
    Phone,
    Tv,
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
    chrome: PlayerChrome = PlayerChrome.Phone,
    onOpenDiagnostics: (() -> Unit)? = null,
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

    BackHandler(enabled = chrome == PlayerChrome.Phone || playerState !is PlaybackPlayerState.Ready) {
        onBack()
    }

    LaunchedEffect(controller) {
        controller.prepare()
    }
    DisposableEffect(controller) {
        onDispose { controller.close() }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(FerrexDesignTokens.Palette.SlateBlack),
        contentAlignment = Alignment.Center,
    ) {
        when (val state = playerState) {
            PlaybackPlayerState.Idle -> PlaybackLoading(
                message = "Preparing playback…",
                chrome = chrome,
                onBack = onBack,
            )
            is PlaybackPlayerState.Loading -> PlaybackLoading(
                message = if (state.retryAttempt > 0) {
                    "${state.message} (${state.retryAttempt}/${state.maxRetryAttempts})"
                } else {
                    state.message
                },
                chrome = chrome,
                onBack = onBack,
            )
            is PlaybackPlayerState.Ready -> PlayerContent(
                streamUrl = state.prepared.streamUrl,
                startPositionMs = state.prepared.startPositionMs,
                streamingHttpClient = streamingHttpClient,
                controller = controller,
                chrome = chrome,
                onBack = onBack,
            )
            is PlaybackPlayerState.Error -> PlaybackErrorPanel(
                failure = state.failure,
                actions = state.actions,
                chrome = chrome,
                onRetry = controller::retry,
                onBack = onBack,
                onChangeServer = onChangeServer,
                onSignOut = onSignOut,
                onOpenDiagnostics = onOpenDiagnostics,
            )
            is PlaybackPlayerState.SessionInvalidated -> PlaybackErrorPanel(
                failure = state.failure.copy(message = "Playback authorization could not be recovered. Sign in again to continue."),
                actions = PlaybackRecoveryActions(retry = false, changeServer = true, signOut = true),
                chrome = chrome,
                onRetry = controller::retry,
                onBack = onBack,
                onChangeServer = onChangeServer,
                onSignOut = onSignOut,
                onOpenDiagnostics = onOpenDiagnostics,
            )
        }
    }
}

@Composable
private fun PlaybackLoading(
    message: String,
    chrome: PlayerChrome,
    onBack: () -> Unit,
) {
    if (chrome == PlayerChrome.Tv) {
        TvPlaybackActionPanel(
            title = "Preparing playback",
            supportingText = message,
            tone = FerrexStatusTone.Secondary,
            actions = listOf(
                TvPlaybackPanelAction(
                    key = "back",
                    label = "Back to details",
                    role = FerrexActionRole.Secondary,
                    contentDescription = "Back to details while playback is loading",
                    onSelect = onBack,
                ),
            ),
            leading = {
                CircularProgressIndicator(
                    color = MaterialTheme.colorScheme.primary,
                    strokeWidth = FerrexDesignTokens.Focus.TvRestingBorder,
                )
            },
        )
        return
    }

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
    chrome: PlayerChrome,
    onRetry: () -> Unit,
    onBack: () -> Unit,
    onChangeServer: () -> Unit,
    onSignOut: () -> Unit,
    onOpenDiagnostics: (() -> Unit)?,
) {
    if (chrome == PlayerChrome.Tv) {
        TvPlaybackActionPanel(
            title = failure.tvPanelTitle(),
            supportingText = buildString {
                append(failure.message)
                failure.httpStatusCode?.let { append("\nHTTP $it") }
            },
            tone = FerrexStatusTone.Error,
            actions = buildList {
                if (actions.retry) {
                    add(
                        TvPlaybackPanelAction(
                            key = "retry",
                            label = "Retry playback",
                            role = FerrexActionRole.Retry,
                            contentDescription = "Retry playback after ${failure.kind}",
                            onSelect = onRetry,
                        ),
                    )
                }
                add(
                    TvPlaybackPanelAction(
                        key = "back",
                        label = "Back to details",
                        role = FerrexActionRole.Secondary,
                        contentDescription = "Back to the previous TV screen",
                        onSelect = onBack,
                    ),
                )
                if (actions.changeServer) {
                    add(
                        TvPlaybackPanelAction(
                            key = "change-server",
                            label = "Change server",
                            role = FerrexActionRole.Secondary,
                            contentDescription = "Change Ferrex server after playback failed",
                            onSelect = onChangeServer,
                        ),
                    )
                }
                if (actions.signOut) {
                    add(
                        TvPlaybackPanelAction(
                            key = "sign-out",
                            label = "Sign out",
                            role = FerrexActionRole.Secondary,
                            contentDescription = "Sign out after playback failed",
                            onSelect = onSignOut,
                        ),
                    )
                }
                onOpenDiagnostics?.let {
                    add(
                        TvPlaybackPanelAction(
                            key = "diagnostics",
                            label = "Diagnostics / Export diagnostics",
                            role = FerrexActionRole.Cache,
                            contentDescription = "Open diagnostics after playback failed",
                            onSelect = it,
                        ),
                    )
                }
            },
        )
        return
    }

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
            onOpenDiagnostics?.let {
                OutlinedButton(onClick = it, modifier = Modifier.fillMaxWidth()) { Text("Diagnostics / Export diagnostics") }
            }
        }
    }
}

private data class TvPlaybackPanelAction(
    val key: String,
    val label: String,
    val role: FerrexActionRole = FerrexActionRole.Secondary,
    val contentDescription: String = label,
    val enabled: Boolean = true,
    val onSelect: () -> Unit,
)

@Composable
private fun TvPlaybackActionPanel(
    title: String,
    supportingText: String,
    actions: List<TvPlaybackPanelAction>,
    tone: FerrexStatusTone = FerrexStatusTone.Secondary,
    leading: (@Composable () -> Unit)? = null,
) {
    val keys = actions.map { it.key }
    val requesters = remember(keys) { actions.associate { it.key to FocusRequester() } }
    val firstEnabledKey = actions.firstOrNull { it.enabled }?.key
    val panelColors = tone.colors()

    LaunchedEffect(keys, firstEnabledKey) {
        firstEnabledKey?.let { key -> runCatching { requesters[key]?.requestFocus() } }
    }

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .padding(FerrexDesignTokens.Space.ScreenTvVertical)
            .widthIn(max = FerrexDesignTokens.Tv.PlayerPanelMaxWidth),
        shape = FerrexDesignTokens.Shapes.RecoveryCard,
        color = panelColors.container,
        contentColor = panelColors.content,
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, panelColors.border.copy(alpha = 0.72f)),
    ) {
        Column(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Xxxl),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            leading?.invoke()
            Text(
                text = title,
                style = MaterialTheme.typography.headlineSmall,
                color = panelColors.accent,
                fontWeight = FontWeight.SemiBold,
                textAlign = TextAlign.Center,
            )
            Text(
                text = supportingText,
                style = MaterialTheme.typography.titleMedium,
                color = panelColors.content,
                textAlign = TextAlign.Center,
            )
            actions.forEach { action ->
                TvControlButton(
                    onClick = action.onSelect,
                    enabled = action.enabled,
                    role = action.role,
                    modifier = Modifier
                        .widthIn(max = FerrexDesignTokens.Tv.PlayerActionMaxWidth)
                        .fillMaxWidth()
                        .focusRequester(requesters.getValue(action.key))
                        .semantics { contentDescription = action.contentDescription },
                ) {
                    Text(action.label, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

private fun PlaybackFailure.tvPanelTitle(): String = when (kind) {
    PlaybackFailureKind.UnsupportedFormat,
    PlaybackFailureKind.Decoder -> "Unsupported media"
    PlaybackFailureKind.Network,
    PlaybackFailureKind.Timeout,
    PlaybackFailureKind.Server,
    PlaybackFailureKind.LibraryOffline -> "Playback interrupted"
    PlaybackFailureKind.Unauthorized,
    PlaybackFailureKind.Forbidden -> "Playback authorization required"
    PlaybackFailureKind.MissingFile,
    PlaybackFailureKind.Unavailable -> "Media unavailable"
    PlaybackFailureKind.Unknown -> "Playback unavailable"
}

@Composable
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
private fun PlayerContent(
    streamUrl: String,
    startPositionMs: Long,
    streamingHttpClient: OkHttpClient,
    controller: PlaybackController,
    chrome: PlayerChrome,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    var aspectRatioMode by remember { mutableStateOf(AspectRatioMode.Fit) }
    var builtInControlsVisible by remember { mutableStateOf(true) }
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
                    useController = chrome == PlayerChrome.Phone
                    controllerAutoShow = true
                    controllerShowTimeoutMs = 3_000
                    setShowSubtitleButton(true)
                    setControllerVisibilityListener(
                        PlayerView.ControllerVisibilityListener { visibility ->
                            builtInControlsVisible = visibility == View.VISIBLE
                        },
                    )
                }
            },
            update = { view ->
                view.player = exoPlayer
                view.useController = chrome == PlayerChrome.Phone
                view.resizeMode = aspectRatioMode.resizeMode
            },
            modifier = Modifier.fillMaxSize(),
        )

        if (chrome == PlayerChrome.Tv) {
            TvPlayerOverlay(
                player = exoPlayer,
                onBack = onBack,
                modifier = Modifier.fillMaxSize(),
            )
        }

        audioTrackWarning?.let { warning ->
            Surface(
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .padding(
                        top = FerrexDesignTokens.Space.Lg,
                        start = FerrexDesignTokens.Space.Xxl,
                        end = FerrexDesignTokens.Space.Xxl,
                    ),
                shape = FerrexDesignTokens.Shapes.Button,
                color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.9f),
                contentColor = MaterialTheme.colorScheme.onSurface,
                border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, MaterialTheme.colorScheme.outline.copy(alpha = 0.58f)),
            ) {
                Text(
                    warning,
                    modifier = Modifier.padding(
                        horizontal = FerrexDesignTokens.Space.Md,
                        vertical = FerrexDesignTokens.Space.Sm,
                    ),
                )
            }
        }

        if (chrome == PlayerChrome.Phone && builtInControlsVisible) {
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
                    text = "Display: ${aspectRatioMode.label}",
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
    }
}

@Composable
private fun TvPlayerOverlay(
    player: Player,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var overlayState by remember(player) {
        mutableStateOf(
            TvPlaybackOverlayUiState(
                controlsVisible = true,
                picker = null,
                isPlaying = player.isPlaying,
            ),
        )
    }
    var position by remember(player) { mutableLongStateOf(player.currentPosition.coerceAtLeast(0L)) }
    var duration by remember(player) { mutableLongStateOf(player.duration.safeDurationMs()) }
    var currentTracks by remember(player) { mutableStateOf(player.currentTracks) }
    var trackSelectionParameters by remember(player) { mutableStateOf(player.trackSelectionParameters) }
    var interactionTick by remember(player) { mutableIntStateOf(0) }

    val rootFocusRequester = remember(player) { FocusRequester() }
    val safeControlFocusRequester = remember(player) { FocusRequester() }

    val audioOptions = remember(currentTracks, trackSelectionParameters) {
        buildMedia3TrackOptions(currentTracks, C.TRACK_TYPE_AUDIO, trackSelectionParameters)
    }
    val subtitleOptions = remember(currentTracks, trackSelectionParameters) {
        buildMedia3TrackOptions(currentTracks, C.TRACK_TYPE_TEXT, trackSelectionParameters)
    }
    val selectedAudioSummary = audioOptions.firstOrNull { it.option.selected }?.option?.title ?: "Default"
    val selectedSubtitleSummary = subtitleOptions.firstOrNull { it.option.selected }?.option?.title ?: "Off"

    fun applyEffect(effect: TvPlaybackOverlayEffect) {
        when (effect) {
            TvPlaybackOverlayEffect.None -> Unit
            TvPlaybackOverlayEffect.ExitPlayback -> onBack()
            TvPlaybackOverlayEffect.TogglePlayPause -> {
                if (player.isPlaying) player.pause() else player.play()
            }
            TvPlaybackOverlayEffect.SeekBackward -> player.seekTo((player.currentPosition - SEEK_BACK_MS).coerceAtLeast(0L))
            TvPlaybackOverlayEffect.SeekForward -> player.seekTo((player.currentPosition + SEEK_FORWARD_MS).coerceAtMost(player.duration.safeDurationMs()))
            TvPlaybackOverlayEffect.RestoreSafeFocus -> Unit
        }
    }

    fun dispatch(event: TvPlaybackOverlayEvent) {
        val (nextState, effect) = TvPlaybackOverlayReducer.reduce(overlayState, event)
        overlayState = nextState
        if (event != TvPlaybackOverlayEvent.AutoHideTimeout) interactionTick += 1
        applyEffect(effect)
    }
    val currentDispatch by rememberUpdatedState(newValue = { event: TvPlaybackOverlayEvent -> dispatch(event) })

    BackHandler {
        dispatch(TvPlaybackOverlayEvent.Back)
    }

    LaunchedEffect(player) {
        runCatching { rootFocusRequester.requestFocus() }
    }

    LaunchedEffect(overlayState.controlsVisible, overlayState.picker) {
        if (overlayState.picker == null) {
            if (overlayState.controlsVisible) {
                runCatching { safeControlFocusRequester.requestFocus() }
            } else {
                runCatching { rootFocusRequester.requestFocus() }
            }
        }
    }

    LaunchedEffect(overlayState.controlsVisible, overlayState.isPlaying, overlayState.picker, interactionTick) {
        if (overlayState.controlsVisible && overlayState.isPlaying && overlayState.picker == null) {
            delay(TV_AUTO_HIDE_MS)
            dispatch(TvPlaybackOverlayEvent.AutoHideTimeout)
        }
    }

    DisposableEffect(player) {
        currentTracks = player.currentTracks
        trackSelectionParameters = player.trackSelectionParameters
        duration = player.duration.safeDurationMs()
        logTrackAvailability(player.currentTracks)

        val listener = object : Player.Listener {
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                currentDispatch(if (isPlaying) TvPlaybackOverlayEvent.PlaybackStarted else TvPlaybackOverlayEvent.PlaybackStopped)
            }

            override fun onPlaybackStateChanged(playbackState: Int) {
                if (playbackState == Player.STATE_READY) {
                    duration = player.duration.safeDurationMs()
                }
            }

            override fun onPositionDiscontinuity(
                oldPosition: Player.PositionInfo,
                newPosition: Player.PositionInfo,
                reason: Int,
            ) {
                position = player.currentPosition.coerceAtLeast(0L)
            }

            override fun onTracksChanged(tracks: Tracks) {
                currentTracks = tracks
                logTrackAvailability(tracks)
            }

            override fun onTrackSelectionParametersChanged(parameters: TrackSelectionParameters) {
                trackSelectionParameters = parameters
            }
        }
        player.addListener(listener)
        onDispose {
            runCatching { player.removeListener(listener) }
        }
    }

    LaunchedEffect(player) {
        while (true) {
            delay(500)
            try {
                position = player.currentPosition.coerceAtLeast(0L)
                duration = player.duration.safeDurationMs()
            } catch (_: IllegalStateException) {
                break
            }
        }
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .focusRequester(rootFocusRequester)
            .focusable()
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                when (event.key) {
                    Key.DirectionCenter, Key.Enter, Key.NumPadEnter -> {
                        if (!overlayState.controlsVisible && overlayState.picker == null) {
                            dispatch(TvPlaybackOverlayEvent.DpadCenter)
                            true
                        } else {
                            false
                        }
                    }
                    Key.DirectionLeft -> {
                        if (!overlayState.controlsVisible && overlayState.picker == null) {
                            dispatch(TvPlaybackOverlayEvent.DpadLeft)
                            true
                        } else {
                            false
                        }
                    }
                    Key.DirectionRight -> {
                        if (!overlayState.controlsVisible && overlayState.picker == null) {
                            dispatch(TvPlaybackOverlayEvent.DpadRight)
                            true
                        } else {
                            false
                        }
                    }
                    Key.DirectionUp, Key.DirectionDown -> {
                        if (!overlayState.controlsVisible && overlayState.picker == null) {
                            dispatch(TvPlaybackOverlayEvent.DpadVertical)
                            true
                        } else {
                            false
                        }
                    }
                    else -> false
                }
            },
    ) {
        val controlsVisible = overlayState.controlsVisible && overlayState.picker == null
        AnimatedVisibility(
            visible = overlayState.controlsVisible,
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(
                        Brush.verticalGradient(
                            colors = listOf(
                                FerrexDesignTokens.Palette.SlateBlack.copy(alpha = 0.62f),
                                Color.Transparent,
                                FerrexDesignTokens.Palette.SlateBlack.copy(alpha = 0.78f),
                            ),
                        ),
                    ),
            )
        }

        AnimatedVisibility(
            visible = controlsVisible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.align(Alignment.TopStart),
        ) {
            TvControlButton(
                onClick = { dispatch(TvPlaybackOverlayEvent.Back) },
                modifier = Modifier
                    .padding(
                        start = FerrexDesignTokens.Tv.PlayerChromeTopPadding,
                        top = FerrexDesignTokens.Tv.PlayerChromeTopPadding,
                    )
                    .semantics { contentDescription = "Back" },
            ) {
                Text("Back")
            }
        }

        AnimatedVisibility(
            visible = controlsVisible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.align(Alignment.BottomCenter),
        ) {
            Column(
                modifier = Modifier.padding(
                    bottom = FerrexDesignTokens.Tv.PlayerChromeBottomPadding,
                    start = FerrexDesignTokens.Tv.PlayerChromeHorizontalPadding,
                    end = FerrexDesignTokens.Tv.PlayerChromeHorizontalPadding,
                ),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                PlaybackProgressStrip(position = position, duration = duration)
                Spacer(Modifier.height(FerrexDesignTokens.Space.Xl))
                Row(
                    horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TvControlButton(
                        onClick = {
                            player.seekTo((player.currentPosition - SEEK_BACK_MS).coerceAtLeast(0L))
                            interactionTick += 1
                        },
                        modifier = Modifier.semantics { contentDescription = "Seek back 10 seconds" },
                    ) {
                        Text("−10s")
                    }
                    TvControlButton(
                        onClick = {
                            if (player.isPlaying) player.pause() else player.play()
                            interactionTick += 1
                        },
                        role = FerrexActionRole.Primary,
                        modifier = Modifier
                            .size(
                                width = FerrexDesignTokens.Tv.PlayerSafeButtonWidth,
                                height = FerrexDesignTokens.Focus.TvButtonMinHeight,
                            )
                            .focusRequester(safeControlFocusRequester)
                            .semantics { contentDescription = if (player.isPlaying) "Pause playback" else "Play playback" },
                    ) {
                        Text(if (player.isPlaying) "Pause" else "Play")
                    }
                    TvControlButton(
                        onClick = {
                            player.seekTo((player.currentPosition + SEEK_FORWARD_MS).coerceAtMost(player.duration.safeDurationMs()))
                            interactionTick += 1
                        },
                        modifier = Modifier.semantics { contentDescription = "Seek forward 30 seconds" },
                    ) {
                        Text("+30s")
                    }
                }

                Spacer(Modifier.height(FerrexDesignTokens.Space.Lg))

                Row(
                    horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TvControlButton(
                        onClick = { dispatch(TvPlaybackOverlayEvent.OpenAudioPicker) },
                        role = FerrexActionRole.Cache,
                        modifier = Modifier
                            .widthIn(max = FerrexDesignTokens.Tv.ActionMaxWidth)
                            .semantics { contentDescription = "Audio track picker. Current audio: $selectedAudioSummary" },
                    ) {
                        Text(
                            text = "Audio: $selectedAudioSummary",
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    TvControlButton(
                        onClick = { dispatch(TvPlaybackOverlayEvent.OpenSubtitlePicker) },
                        role = FerrexActionRole.Cache,
                        modifier = Modifier
                            .widthIn(max = FerrexDesignTokens.Tv.ActionMaxWidth)
                            .semantics { contentDescription = "Subtitle track picker. Current subtitles: $selectedSubtitleSummary" },
                    ) {
                        Text(
                            text = "Subtitles: $selectedSubtitleSummary",
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }

        overlayState.picker?.let { picker ->
            TrackSelectionPanel(
                picker = picker,
                options = when (picker) {
                    TvTrackPickerKind.Audio -> audioOptions
                    TvTrackPickerKind.Subtitles -> subtitleOptions
                },
                onDismiss = { dispatch(TvPlaybackOverlayEvent.PickerDismissed) },
                onSelect = { option ->
                    applyTrackSelection(player, option)
                    dispatch(TvPlaybackOverlayEvent.PickerSelected)
                },
            )
        }
    }
}

@Composable
private fun PlaybackProgressStrip(position: Long, duration: Long) {
    val progress = if (duration > 0L) (position.toFloat() / duration).coerceIn(0f, 1f) else 0f
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            modifier = Modifier
                .width(FerrexDesignTokens.Tv.PlayerProgressWidth)
                .height(FerrexDesignTokens.Space.Sm)
                .background(MaterialTheme.colorScheme.onSurface.copy(alpha = 0.28f), FerrexDesignTokens.Shapes.Pill),
        ) {
            Box(
                modifier = Modifier
                    .fillMaxHeight()
                    .fillMaxWidth(progress)
                    .background(MaterialTheme.colorScheme.primary, FerrexDesignTokens.Shapes.Pill),
            )
        }
        Spacer(Modifier.height(FerrexDesignTokens.Space.Sm))
        Row(
            modifier = Modifier.width(FerrexDesignTokens.Tv.PlayerProgressWidth),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(formatTime(position), color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.86f), style = MaterialTheme.typography.bodyMedium)
            Text(formatTime(duration), color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.86f), style = MaterialTheme.typography.bodyMedium)
        }
    }
}

@Composable
private fun TrackSelectionPanel(
    picker: TvTrackPickerKind,
    options: List<Media3TrackOption>,
    onSelect: (Media3TrackOption) -> Unit,
    onDismiss: () -> Unit,
) {
    val initialFocusRequester = remember(picker) { FocusRequester() }
    val initialFocusKey = options.firstOrNull { it.option.selectable }?.option?.key
    val title = when (picker) {
        TvTrackPickerKind.Audio -> "Audio tracks"
        TvTrackPickerKind.Subtitles -> "Subtitle tracks"
    }
    val helperText = when (picker) {
        TvTrackPickerKind.Audio -> "Choose the audio stream reported by ExoPlayer. Capability warnings stay visible instead of hiding playable audio."
        TvTrackPickerKind.Subtitles -> "Choose a subtitle track or turn subtitles off."
    }
    val emptyMessage = when (picker) {
        TvTrackPickerKind.Audio -> "No audio tracks have been reported yet. Playback can continue while ExoPlayer discovers tracks."
        TvTrackPickerKind.Subtitles -> "No subtitle tracks have been reported yet. The Off option remains available."
    }
    val hasReportedTracks = options.any { !it.option.isOff }
    val panelColors = FerrexStatusTone.Secondary.colors()

    LaunchedEffect(picker, initialFocusKey) {
        runCatching { initialFocusRequester.requestFocus() }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(FerrexDesignTokens.Palette.SlateBlack.copy(alpha = 0.58f)),
        contentAlignment = Alignment.Center,
    ) {
        Surface(
            modifier = Modifier
                .padding(
                    horizontal = FerrexDesignTokens.Space.ScreenTvHorizontal,
                    vertical = FerrexDesignTokens.Space.ScreenTvVertical,
                )
                .widthIn(max = FerrexDesignTokens.Tv.PlayerPickerMaxWidth)
                .focusGroup(),
            shape = FerrexDesignTokens.Shapes.RecoveryCard,
            color = panelColors.container,
            contentColor = panelColors.content,
            border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, panelColors.border.copy(alpha = 0.72f)),
        ) {
            Column(
                modifier = Modifier.padding(FerrexDesignTokens.Space.Xxxl),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            title,
                            style = MaterialTheme.typography.headlineSmall,
                            color = panelColors.accent,
                            fontWeight = FontWeight.SemiBold,
                        )
                        Text(helperText, color = panelColors.content, style = MaterialTheme.typography.bodyMedium)
                    }
                    Spacer(Modifier.width(FerrexDesignTokens.Space.Xxl))
                    TvControlButton(
                        onClick = onDismiss,
                        modifier = (if (initialFocusKey == null) Modifier.focusRequester(initialFocusRequester) else Modifier)
                            .semantics { contentDescription = "Close track picker" },
                    ) {
                        Text("Close")
                    }
                }

                if (!hasReportedTracks) {
                    Text(emptyMessage, color = panelColors.content, style = MaterialTheme.typography.bodyMedium)
                }

                LazyColumn(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(max = FerrexDesignTokens.Tv.TrackListMaxHeight),
                    verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
                ) {
                    items(options, key = { it.option.key }) { option ->
                        TrackOptionButton(
                            option = option,
                            onClick = { onSelect(option) },
                            modifier = if (option.option.key == initialFocusKey) {
                                Modifier.focusRequester(initialFocusRequester)
                            } else {
                                Modifier
                            },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun TrackOptionButton(
    option: Media3TrackOption,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var isFocused by remember { mutableStateOf(false) }
    val enabled = option.option.selectable
    val scheme = MaterialTheme.colorScheme
    val shape = FerrexDesignTokens.Shapes.FocusSurface
    val containerColor = when {
        option.option.selected -> scheme.primary
        isFocused -> FerrexDesignTokens.Palette.FocusWash
        else -> scheme.surfaceVariant.copy(alpha = 0.74f)
    }
    val contentColor = if (option.option.selected) scheme.onPrimary else scheme.onSurface
    val detailColor = when {
        !enabled -> scheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContent)
        option.option.selected -> scheme.onPrimary.copy(alpha = 0.68f)
        else -> scheme.onSurfaceVariant
    }

    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier
            .fillMaxWidth()
            .tvRemoteActivation(enabled = enabled, onActivate = onClick)
            .onFocusChanged { isFocused = it.isFocused }
            .semantics {
                contentDescription = buildString {
                    append(option.option.title)
                    option.option.details?.let { append(", ").append(it) }
                    if (option.option.selected) append(", selected")
                    if (!enabled) append(", unavailable")
                }
            }
            .border(
                width = when {
                    isFocused -> FerrexDesignTokens.Focus.TvFocusedBorder
                    option.option.selected -> FerrexDesignTokens.Focus.TvRestingBorder
                    else -> FerrexDesignTokens.Space.None
                },
                color = if (isFocused) scheme.primary else scheme.outline.copy(alpha = 0.52f),
                shape = shape,
            ),
        colors = ButtonDefaults.buttonColors(
            containerColor = containerColor,
            contentColor = contentColor,
            disabledContainerColor = scheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContainer),
            disabledContentColor = scheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContent),
        ),
        shape = shape,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = if (option.option.selected) "✓" else "",
                modifier = Modifier.width(FerrexDesignTokens.Space.Xxxl),
                fontWeight = FontWeight.Bold,
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = option.option.title,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    fontWeight = FontWeight.SemiBold,
                )
                option.option.details?.let { details ->
                    Text(
                        text = details,
                        color = detailColor,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }
}

@Composable
private fun TvControlButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    role: FerrexActionRole = FerrexActionRole.Secondary,
    content: @Composable RowScope.() -> Unit,
) {
    var isFocused by remember { mutableStateOf(false) }
    val scheme = MaterialTheme.colorScheme
    val statusColors = role.statusTone().colors()
    val buttonShape = FerrexDesignTokens.Shapes.FocusSurface
    val containerColor = when (role) {
        FerrexActionRole.Primary,
        FerrexActionRole.Retry -> if (isFocused && enabled) scheme.primary else statusColors.container.copy(alpha = 0.82f)
        FerrexActionRole.DestructiveReset,
        FerrexActionRole.Error -> if (isFocused && enabled) scheme.error else statusColors.container
        FerrexActionRole.Secondary,
        FerrexActionRole.Cache,
        FerrexActionRole.StaleOffline -> if (isFocused && enabled) statusColors.container.copy(alpha = 0.92f) else statusColors.container
    }
    val contentColor = when {
        !enabled -> scheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContent)
        isFocused && (role == FerrexActionRole.Primary || role == FerrexActionRole.Retry) -> scheme.onPrimary
        isFocused && (role == FerrexActionRole.DestructiveReset || role == FerrexActionRole.Error) -> scheme.onError
        else -> statusColors.content
    }
    val border = BorderStroke(
        width = if (isFocused && enabled) FerrexDesignTokens.Focus.TvFocusedBorder else FerrexDesignTokens.Focus.TvRestingBorder,
        color = if (isFocused && enabled) statusColors.accent else statusColors.border.copy(alpha = 0.58f),
    )

    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier
            .tvRemoteActivation(enabled = enabled, onActivate = onClick)
            .onFocusChanged { isFocused = it.isFocused }
            .scale(if (isFocused && enabled) FerrexDesignTokens.Focus.TvFocusedScale else FerrexDesignTokens.Focus.TvRestingScale),
        colors = ButtonDefaults.buttonColors(
            containerColor = containerColor,
            contentColor = contentColor,
            disabledContainerColor = scheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContainer),
            disabledContentColor = scheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContent),
        ),
        border = border,
        shape = buttonShape,
        content = content,
    )
}

private data class Media3TrackOption(
    val option: PlaybackTrackOption,
    val mediaTrackGroup: TrackGroup?,
)

private fun buildMedia3TrackOptions(
    tracks: Tracks,
    trackType: Int,
    parameters: TrackSelectionParameters,
): List<Media3TrackOption> {
    val snapshots = tracks.toPlaybackTrackGroupSnapshots()
    val groupsByIndex = tracks.groups.mapIndexed { index, group -> index to group.mediaTrackGroup }.toMap()
    return PlaybackTrackOptions.buildOptions(
        groups = snapshots,
        trackType = trackType,
        disabledTrackTypes = parameters.disabledTrackTypes,
    ).map { option ->
        Media3TrackOption(
            option = option,
            mediaTrackGroup = option.groupIndex?.let(groupsByIndex::get),
        )
    }
}

private fun applyTrackSelection(player: Player, trackOption: Media3TrackOption) {
    val option = trackOption.option
    if (!option.selectable) return
    if (!player.isCommandAvailable(Player.COMMAND_SET_TRACK_SELECTION_PARAMETERS)) {
        PlaybackDiagnosticLog.warn(
            TV_PLAYER_TAG,
            "Cannot select ${trackTypeLabel(option.type)} track; player command is unavailable",
        )
        return
    }

    try {
        val builder = player.trackSelectionParameters
            .buildUpon()
            .clearOverridesOfType(option.type)

        if (option.isOff && option.type == C.TRACK_TYPE_TEXT) {
            player.setTrackSelectionParameters(
                builder
                    .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
                    .build(),
            )
            PlaybackDiagnosticLog.info(TV_PLAYER_TAG, "Subtitles turned off")
            return
        }

        val mediaTrackGroup = trackOption.mediaTrackGroup
        val trackIndex = option.trackIndex
        if (mediaTrackGroup == null || trackIndex == null) {
            PlaybackDiagnosticLog.warn(
                TV_PLAYER_TAG,
                "Ignoring invalid ${trackTypeLabel(option.type)} track option: ${option.title}",
            )
            return
        }

        player.setTrackSelectionParameters(
            builder
                .setTrackTypeDisabled(option.type, false)
                .setOverrideForType(TrackSelectionOverride(mediaTrackGroup, trackIndex))
                .build(),
        )
        PlaybackDiagnosticLog.info(
            TV_PLAYER_TAG,
            "Selected ${trackTypeLabel(option.type)} track: ${option.title}${option.details?.let { " ($it)" }.orEmpty()}",
        )
    } catch (error: RuntimeException) {
        PlaybackDiagnosticLog.error(
            TV_PLAYER_TAG,
            "Failed selecting ${trackTypeLabel(option.type)} track: ${option.title}",
            error,
        )
    }
}

private fun logTrackAvailability(tracks: Tracks) {
    val summary = PlaybackTrackOptions.describeTracksForDiagnostics(
        tracks.toPlaybackTrackGroupSnapshots().filter { group ->
            group.type == C.TRACK_TYPE_AUDIO || group.type == C.TRACK_TYPE_TEXT || group.type == C.TRACK_TYPE_VIDEO
        },
    )
    PlaybackDiagnosticLog.info(TV_PLAYER_TAG, summary)
    val audioTrackCount = tracks.groups.filter { it.type == C.TRACK_TYPE_AUDIO }.sumOf { it.length }
    val textTrackCount = tracks.groups.filter { it.type == C.TRACK_TYPE_TEXT }.sumOf { it.length }
    if (audioTrackCount == 0) {
        PlaybackDiagnosticLog.warn(TV_PLAYER_TAG, "Player.currentTracks has no audio tracks")
    }
    if (textTrackCount == 0) {
        PlaybackDiagnosticLog.info(TV_PLAYER_TAG, "Player.currentTracks has no subtitle/text tracks")
    }
}

private fun Tracks.unsupportedAudioWarning(): String? {
    val audioGroups = groups.filter { it.type == C.TRACK_TYPE_AUDIO }
    val audioTrackCount = audioGroups.sumOf { it.length }
    if (audioTrackCount == 0) return null

    val playableAudioTrackCount = audioGroups.sumOf { group ->
        (0 until group.length).count { trackIndex ->
            val support = group.getTrackSupport(trackIndex)
            support == C.FORMAT_HANDLED || support == C.FORMAT_EXCEEDS_CAPABILITIES
        }
    }
    if (playableAudioTrackCount > 0) return null

    return "Audio tracks were found, but this device cannot fully support them."
}

private fun Long.safeDurationMs(): Long = takeUnless { it == C.TIME_UNSET }?.coerceAtLeast(0L) ?: 0L

private fun formatTime(ms: Long): String {
    val totalSeconds = ms.coerceAtLeast(0L) / 1000
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    val seconds = totalSeconds % 60
    return if (hours > 0) {
        "%d:%02d:%02d".format(hours, minutes, seconds)
    } else {
        "%02d:%02d".format(minutes, seconds)
    }
}

private fun trackTypeLabel(trackType: Int): String = when (trackType) {
    C.TRACK_TYPE_AUDIO -> "audio"
    C.TRACK_TYPE_TEXT -> "subtitle"
    else -> "media"
}

private fun Modifier.tvRemoteActivation(
    enabled: Boolean,
    onActivate: () -> Unit,
): Modifier = if (!enabled) {
    this
} else {
    onPreviewKeyEvent { event ->
        if (!event.key.isTvActivationKey()) return@onPreviewKeyEvent false
        when (event.type) {
            KeyEventType.KeyDown -> true
            KeyEventType.KeyUp -> {
                onActivate()
                true
            }
            else -> false
        }
    }
}

private fun Key.isTvActivationKey(): Boolean = this == Key.DirectionCenter || this == Key.Enter || this == Key.NumPadEnter

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
