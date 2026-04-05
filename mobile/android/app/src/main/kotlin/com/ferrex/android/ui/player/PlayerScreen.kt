package com.ferrex.android.ui.player

import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.ui.PlayerView
import com.ferrex.android.core.diagnostics.DiagnosticLog
import com.ferrex.android.core.diagnostics.PlaybackDiagnostics
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient

private const val TAG = "PlayerScreen"

/**
 * Video player screen using Media3 ExoPlayer.
 *
 * Uses OkHttpDataSource.Factory so that ExoPlayer inherits the app's
 * OkHttpClient with its AuthInterceptor — stream requests automatically
 * include the Bearer token. This is required because the stream endpoint
 * (`GET /api/v1/stream/{id}`) validates auth.
 *
 * Progress tracking: a LaunchedEffect coroutine loop reports position
 * every 10 seconds via the WatchProgressTracker. Immediate report on
 * pause/stop via Player.Listener.
 */
@Composable
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
fun PlayerScreen(
    viewModel: PlayerViewModel,
    okHttpClient: OkHttpClient,
) {
    // Enter immersive fullscreen: hides status bar + nav bar, locks landscape,
    // keeps screen on.  Automatically restored when this composable leaves
    // composition (i.e. user navigates away from the player).
    ImmersiveMode()

    val playerState by viewModel.playerState.collectAsState()

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black),
        contentAlignment = Alignment.Center,
    ) {
        when (val state = playerState) {
            is PlayerState.Loading -> {
                LoadingScreen(message = "Preparing playback…")
            }
            is PlayerState.Error -> {
                ErrorScreen(
                    message = state.message,
                    onRetry = if (state.canRetry) {{ viewModel.retry() }} else null,
                )
            }
            is PlayerState.Ready -> {
                PlayerContent(
                    streamUrl = state.streamUrl,
                    startPositionMs = state.startPositionMs,
                    okHttpClient = okHttpClient,
                    onProgressUpdate = { positionMs, durationMs ->
                        viewModel.reportProgress(positionMs, durationMs)
                    },
                    onPlaybackEnded = { viewModel.onPlaybackEnded() },
                    onError = { message, positionMs ->
                        viewModel.onPlayerError(message, positionMs)
                    },
                )
            }
        }
    }
}

/**
 * Inner composable that owns the ExoPlayer lifecycle.
 *
 * Lifecycle contract:
 * - Player is created in [remember] (survives recomposition).
 * - Playback setup, listener registration, and release are all in one
 *   [DisposableEffect] keyed on the player instance — guarantees paired
 *   add/remove and prevents accessing a released player from stale listeners.
 * - Progress tracking runs in a separate [LaunchedEffect] with a guard
 *   against [IllegalStateException] from a concurrently-released player.
 * - All callback lambdas use [rememberUpdatedState] so listeners always
 *   invoke the latest reference, even after recomposition.
 *
 * Buffer / retry tuning (addresses fast-LAN failures):
 * - Max buffer capped at 30s / 32 MB — prevents OOM on high-bitrate
 *   content and reduces idle-connection pauses that can trigger
 *   server-side timeouts.
 * - ExoPlayer internal retry raised to 6 (from default 3) with
 *   exponential backoff — gives transient network issues time to
 *   resolve before the error surfaces.
 * - [PlaybackDiagnostics] analytics listener logs every load, decode,
 *   and state event into [DiagnosticLog] so crash files have a
 *   complete trail of what ExoPlayer was doing.
 */
@Composable
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
private fun PlayerContent(
    streamUrl: String,
    startPositionMs: Long,
    okHttpClient: OkHttpClient,
    onProgressUpdate: (positionMs: Long, durationMs: Long) -> Unit,
    onPlaybackEnded: () -> Unit,
    onError: (message: String, lastPositionMs: Long) -> Unit,
) {
    val context = LocalContext.current

    // rememberUpdatedState: callbacks captured in DisposableEffect/LaunchedEffect
    // always point to the latest lambda instance after recomposition.
    val currentOnProgressUpdate by rememberUpdatedState(onProgressUpdate)
    val currentOnPlaybackEnded by rememberUpdatedState(onPlaybackEnded)
    val currentOnError by rememberUpdatedState(onError)

    // Use OkHttp as ExoPlayer's HTTP backend so the AuthInterceptor
    // injects the Bearer token into stream requests.
    val exoPlayer = remember {
        DiagnosticLog.i(TAG, "Creating ExoPlayer for: $streamUrl")

        val dataSourceFactory = OkHttpDataSource.Factory(okHttpClient)
            .setUserAgent("Ferrex-Android/1.0")

        // More aggressive retry: 6 attempts (default is 3) before the
        // error surfaces as fatal.  Covers transient connection resets
        // without bothering the user.
        val errorPolicy = DefaultLoadErrorHandlingPolicy(/* minimumLoadableRetryCount = */ 6)

        val mediaSourceFactory = DefaultMediaSourceFactory(dataSourceFactory)
            .setLoadErrorHandlingPolicy(errorPolicy)

        // Buffer tuning:
        //  - minBufferMs  = 15s — resume loading when buffer drops below 15s
        //  - maxBufferMs  = 30s — stop loading once we have 30s buffered
        //  - playbackMs   = 2.5s — start playback once 2.5s is available
        //  - rebufferMs   = 5s   — after a rebuffer, wait for 5s before resuming play
        //  - targetBuffer = 32 MB — hard byte cap; prevents OOM on high-bitrate content
        //                           and keeps the connection active (less idle time for
        //                           the server to consider the socket dead).
        val loadControl = DefaultLoadControl.Builder()
            .setBufferDurationsMs(
                /* minBufferMs = */                     15_000,
                /* maxBufferMs = */                     30_000,
                /* bufferForPlaybackMs = */              2_500,
                /* bufferForPlaybackAfterRebufferMs = */ 5_000,
            )
            .setTargetBufferBytes(32 * 1024 * 1024) // 32 MB
            .build()

        ExoPlayer.Builder(context)
            .setMediaSourceFactory(mediaSourceFactory)
            .setLoadControl(loadControl)
            .build()
            .also { player ->
                player.addAnalyticsListener(PlaybackDiagnostics())
            }
    }

    // ── Player lifecycle: setup, listener, and release ──────────
    //
    // All in one DisposableEffect keyed on the player so add/remove/release
    // are guaranteed to pair.  The listener uses rememberUpdatedState refs
    // so it always calls the latest callbacks.
    DisposableEffect(exoPlayer) {
        val listener = object : Player.Listener {
            override fun onPlaybackStateChanged(playbackState: Int) {
                when (playbackState) {
                    Player.STATE_ENDED -> currentOnPlaybackEnded()
                    else -> {}
                }
            }

            override fun onIsPlayingChanged(isPlaying: Boolean) {
                if (!isPlaying && exoPlayer.playbackState != Player.STATE_ENDED) {
                    // Paused or buffering — report progress immediately.
                    // Guard: only report if we actually have a valid duration.
                    val duration = exoPlayer.duration
                    if (duration > 0 && duration != C.TIME_UNSET) {
                        currentOnProgressUpdate(exoPlayer.currentPosition, duration)
                    }
                }
            }

            override fun onPlayerError(error: PlaybackException) {
                val lastPosition = exoPlayer.currentPosition.coerceAtLeast(0)
                val message = when (error.errorCode) {
                    PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED ->
                        "Network connection failed"
                    PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT ->
                        "Connection timed out"
                    PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS ->
                        "Server error (HTTP ${error.cause?.message ?: "unknown"})"
                    PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND ->
                        "Media not found on server"
                    PlaybackException.ERROR_CODE_IO_UNSPECIFIED ->
                        "Network error"
                    PlaybackException.ERROR_CODE_DECODER_INIT_FAILED ->
                        "Unable to initialize decoder"
                    PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED ->
                        "Unsupported media format"
                    else ->
                        error.cause?.message ?: error.message ?: "Playback error"
                }
                // PlaybackDiagnostics already logged full details; this
                // routes the user-facing message + position to the ViewModel.
                currentOnError(message, lastPosition)
            }
        }

        exoPlayer.addListener(listener)
        exoPlayer.setMediaItem(MediaItem.fromUri(Uri.parse(streamUrl)))
        exoPlayer.prepare()
        exoPlayer.playWhenReady = true
        if (startPositionMs > 0) {
            exoPlayer.seekTo(startPositionMs)
        }

        DiagnosticLog.i(TAG, "ExoPlayer prepared (startPos=${startPositionMs}ms)")

        onDispose {
            DiagnosticLog.i(TAG, "Releasing ExoPlayer")
            // Report final position before releasing.
            val duration = exoPlayer.duration
            if (duration > 0 && duration != C.TIME_UNSET) {
                currentOnProgressUpdate(exoPlayer.currentPosition, duration)
            }
            exoPlayer.removeListener(listener)
            exoPlayer.release()
        }
    }

    // ── Periodic progress tracking ──────────────────────────────
    //
    // Separate from the DisposableEffect because this is a long-running
    // coroutine, not setup/teardown.  Guarded against IllegalStateException
    // in case the player is released between delay() resuming and the
    // property access (the DisposableEffect onDispose runs on the main
    // thread during recomposition, while this coroutine resumes on main
    // after delay — tiny race window).
    LaunchedEffect(exoPlayer) {
        while (true) {
            delay(10_000)
            try {
                if (exoPlayer.isPlaying) {
                    val duration = exoPlayer.duration
                    if (duration > 0 && duration != C.TIME_UNSET) {
                        currentOnProgressUpdate(exoPlayer.currentPosition, duration)
                    }
                }
            } catch (_: IllegalStateException) {
                // Player was released — stop the loop.
                break
            }
        }
    }

    // ── ExoPlayer view ──────────────────────────────────────────
    AndroidView(
        factory = { ctx ->
            PlayerView(ctx).apply {
                player = exoPlayer
                useController = true
            }
        },
        modifier = Modifier.fillMaxSize(),
    )
}
