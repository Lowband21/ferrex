package com.ferrex.android.ui.tv

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ChevronLeft
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.media3.common.Player
import com.ferrex.android.ui.player.PlayerScreen
import com.ferrex.android.ui.player.PlayerState
import com.ferrex.android.ui.player.PlayerViewModel
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient

/** TV player shell that keeps player plumbing shared and overlays TV-native controls. */
@Composable
fun TvPlayerScreen(
    viewModel: PlayerViewModel,
    okHttpClient: OkHttpClient,
    onBack: () -> Unit,
) {
    val playerState by viewModel.playerState.collectAsState()
    var player by remember { mutableStateOf<Player?>(null) }
    var controlsVisible by remember { mutableStateOf(true) }

    BackHandler {
        if (player != null && !controlsVisible) {
            controlsVisible = true
        } else {
            onBack()
        }
    }

    // Drop player ref when playback is no longer active so the overlay
    // does not hold a released ExoPlayer instance.
    LaunchedEffect(playerState) {
        if (playerState !is PlayerState.Ready) {
            player = null
            controlsVisible = true
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        PlayerScreen(
            viewModel = viewModel,
            okHttpClient = okHttpClient,
            useBuiltInController = false,
            onPlayerReady = { player = it },
        )
        player?.let {
            TvPlayerOverlay(
                player = it,
                controlsVisible = controlsVisible,
                onControlsVisibleChange = { controlsVisible = it },
                onBack = onBack,
            )
        }
    }
}

/**
 * TV-native player overlay with D-pad-friendly controls.
 *
 * Features:
 * - Large, focusable play/pause and seek buttons with visible focus halos
 * - Progress bar with current/duration time labels
 * - Auto-hide after 5 s of inactivity while playing
 * - D-pad OK toggles play/pause when controls are hidden
 * - D-pad Left/Right seeks when controls are hidden
 * - Any D-pad press restores hidden controls
 * - Back shows hidden controls first; pressing Back again leaves playback
 */
@Composable
fun TvPlayerOverlay(
    player: Player,
    controlsVisible: Boolean,
    onControlsVisibleChange: (Boolean) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var isPlaying by remember { mutableStateOf(player.isPlaying) }
    var position by remember { mutableLongStateOf(player.currentPosition) }
    var duration by remember { mutableLongStateOf(player.duration.coerceAtLeast(0L)) }

    // Auto-hide while playing.
    LaunchedEffect(controlsVisible, isPlaying) {
        if (controlsVisible && isPlaying) {
            delay(5_000)
            onControlsVisibleChange(false)
        }
    }

    // Sync state from ExoPlayer.
    DisposableEffect(player) {
        val listener = object : Player.Listener {
            override fun onIsPlayingChanged(playing: Boolean) {
                isPlaying = playing
            }

            override fun onPlaybackStateChanged(playbackState: Int) {
                if (playbackState == Player.STATE_READY) {
                    duration = player.duration.coerceAtLeast(0L)
                }
            }

            override fun onPositionDiscontinuity(
                oldPosition: Player.PositionInfo,
                newPosition: Player.PositionInfo,
                reason: Int
            ) {
                position = player.currentPosition
            }
        }
        player.addListener(listener)
        onDispose {
            try {
                player.removeListener(listener)
            } catch (_: IllegalStateException) {
                // Player was already released.
            }
        }
    }

    // Poll position for smooth progress-bar updates.
    LaunchedEffect(player, isPlaying) {
        while (true) {
            delay(500)
            try {
                position = player.currentPosition
                duration = player.duration.coerceAtLeast(0L)
            } catch (_: IllegalStateException) {
                break
            }
        }
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                when (event.key) {
                    Key.DirectionCenter, Key.Enter -> {
                        if (!controlsVisible) {
                            if (player.isPlaying) player.pause() else player.play()
                            onControlsVisibleChange(true)
                            true
                        } else {
                            false
                        }
                    }

                    Key.DirectionLeft -> {
                        if (!controlsVisible) {
                            player.seekTo((player.currentPosition - 10_000).coerceAtLeast(0))
                            onControlsVisibleChange(true)
                            true
                        } else false
                    }

                    Key.DirectionRight -> {
                        if (!controlsVisible) {
                            val dur = player.duration.coerceAtLeast(0L)
                            player.seekTo(
                                (player.currentPosition + 30_000).coerceAtMost(dur)
                            )
                            onControlsVisibleChange(true)
                            true
                        } else false
                    }

                    Key.DirectionUp, Key.DirectionDown -> {
                        if (!controlsVisible) {
                            onControlsVisibleChange(true)
                            true
                        } else false
                    }

                    else -> false
                }
            }
    ) {
        // Gradient scrim behind controls.
        AnimatedVisibility(
            visible = controlsVisible,
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(
                        Brush.verticalGradient(
                            colors = listOf(
                                Color.Black.copy(alpha = 0.6f),
                                Color.Transparent,
                                Color.Black.copy(alpha = 0.7f),
                            ),
                        ),
                    ),
            )
        }

        // Back button.
        AnimatedVisibility(
            visible = controlsVisible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.align(Alignment.TopStart),
        ) {
            TvControlButton(
                onClick = onBack,
                modifier = Modifier
                    .padding(start = 32.dp, top = 32.dp)
                    .semantics { contentDescription = "Back" },
            ) {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = null,
                )
                Spacer(Modifier.width(8.dp))
                Text("Back")
            }
        }

        // Bottom controls.
        AnimatedVisibility(
            visible = controlsVisible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.align(Alignment.BottomCenter),
        ) {
            Column(
                modifier = Modifier
                    .padding(bottom = 48.dp, start = 48.dp, end = 48.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                // Progress bar.
                if (duration > 0) {
                    val progress = (position.toFloat() / duration).coerceIn(0f, 1f)
                    LinearProgressIndicator(
                        progress = { progress },
                        modifier = Modifier
                            .width(520.dp)
                            .padding(bottom = 12.dp),
                        color = Color.White,
                        trackColor = Color.White.copy(alpha = 0.3f),
                    )

                    Row(
                        modifier = Modifier.width(520.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text(
                            text = formatTime(position),
                            color = Color.White.copy(alpha = 0.85f),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Text(
                            text = formatTime(duration),
                            color = Color.White.copy(alpha = 0.85f),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }

                    Spacer(Modifier.height(20.dp))
                }

                // Transport buttons.
                Row(
                    horizontalArrangement = Arrangement.spacedBy(20.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TvControlButton(
                        onClick = {
                            player.seekTo(
                                (player.currentPosition - 10_000).coerceAtLeast(0)
                            )
                        },
                    ) {
                        Icon(Icons.Filled.ChevronLeft, contentDescription = null)
                        Text("-10s")
                    }

                    TvControlButton(
                        onClick = {
                            if (player.isPlaying) player.pause() else player.play()
                        },
                        modifier = Modifier.size(width = 72.dp, height = 56.dp),
                    ) {
                        Icon(
                            imageVector = if (player.isPlaying) {
                                Icons.Filled.Pause
                            } else {
                                Icons.Filled.PlayArrow
                            },
                            contentDescription = if (player.isPlaying) "Pause" else "Play",
                            modifier = Modifier.size(28.dp),
                        )
                    }

                    TvControlButton(
                        onClick = {
                            val dur = player.duration.coerceAtLeast(0L)
                            player.seekTo(
                                (player.currentPosition + 30_000).coerceAtMost(dur)
                            )
                        },
                    ) {
                        Text("+30s")
                        Icon(Icons.Filled.ChevronRight, contentDescription = null)
                    }
                }
            }
        }
    }
}

/** TV-optimized button with visible focus state for D-pad navigation. */
@Composable
private fun TvControlButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable RowScope.() -> Unit,
) {
    var isFocused by remember { mutableStateOf(false) }

    Button(
        onClick = onClick,
        modifier = modifier
            .onFocusChanged { isFocused = it.isFocused }
            .scale(if (isFocused) 1.08f else 1f)
            .border(
                width = if (isFocused) 2.dp else 0.dp,
                color = Color.White,
                shape = RoundedCornerShape(8.dp),
            ),
        colors = ButtonDefaults.buttonColors(
            containerColor = Color.Black.copy(alpha = 0.62f),
            contentColor = Color.White,
        ),
        shape = RoundedCornerShape(8.dp),
        content = content,
    )
}

private fun formatTime(ms: Long): String {
    val totalSeconds = ms / 1000
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    val seconds = totalSeconds % 60
    return if (hours > 0) {
        "%d:%02d:%02d".format(hours, minutes, seconds)
    } else {
        "%02d:%02d".format(minutes, seconds)
    }
}
