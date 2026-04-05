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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.ui.PlayerView
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient

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
                ErrorScreen(message = state.message)
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
                )
            }
        }
    }
}

@Composable
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
private fun PlayerContent(
    streamUrl: String,
    startPositionMs: Long,
    okHttpClient: OkHttpClient,
    onProgressUpdate: (positionMs: Long, durationMs: Long) -> Unit,
    onPlaybackEnded: () -> Unit,
    onError: ((String) -> Unit)? = null,
) {
    val context = LocalContext.current

    // Use OkHttp as ExoPlayer's HTTP backend so the AuthInterceptor
    // injects the Bearer token into stream requests.
    val exoPlayer = remember {
        val dataSourceFactory = OkHttpDataSource.Factory(okHttpClient)
        val mediaSourceFactory = DefaultMediaSourceFactory(dataSourceFactory)

        ExoPlayer.Builder(context)
            .setMediaSourceFactory(mediaSourceFactory)
            .build()
            .apply {
                setMediaItem(MediaItem.fromUri(Uri.parse(streamUrl)))
                prepare()
                playWhenReady = true
                if (startPositionMs > 0) {
                    seekTo(startPositionMs)
                }
            }
    }

    // Progress tracking coroutine — every 10 seconds
    LaunchedEffect(exoPlayer) {
        while (true) {
            delay(10_000)
            if (exoPlayer.isPlaying) {
                onProgressUpdate(
                    exoPlayer.currentPosition,
                    exoPlayer.duration.coerceAtLeast(0),
                )
            }
        }
    }

    // Player listener for pause/stop/end events
    LaunchedEffect(exoPlayer) {
        val listener = object : Player.Listener {
            override fun onPlaybackStateChanged(playbackState: Int) {
                when (playbackState) {
                    Player.STATE_ENDED -> onPlaybackEnded()
                    else -> {}
                }
            }

            override fun onIsPlayingChanged(isPlaying: Boolean) {
                if (!isPlaying && exoPlayer.playbackState != Player.STATE_ENDED) {
                    // Paused — report progress immediately
                    onProgressUpdate(
                        exoPlayer.currentPosition,
                        exoPlayer.duration.coerceAtLeast(0),
                    )
                }
            }

            override fun onPlayerError(error: PlaybackException) {
                val cause = error.cause?.message ?: error.message ?: "Unknown playback error"
                android.util.Log.e("PlayerContent", "Playback error: $cause", error)
                onError?.invoke(cause)
            }
        }
        exoPlayer.addListener(listener)
    }

    // ExoPlayer view
    AndroidView(
        factory = { ctx ->
            PlayerView(ctx).apply {
                player = exoPlayer
                useController = true
            }
        },
        modifier = Modifier.fillMaxSize(),
    )

    // Release player on dispose
    DisposableEffect(Unit) {
        onDispose {
            // Final progress report
            if (exoPlayer.duration > 0) {
                onProgressUpdate(exoPlayer.currentPosition, exoPlayer.duration)
            }
            exoPlayer.release()
        }
    }
}
