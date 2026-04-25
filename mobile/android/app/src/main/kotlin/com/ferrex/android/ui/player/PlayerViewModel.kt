package com.ferrex.android.ui.player

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.api.StreamingClient
import com.ferrex.android.core.diagnostics.DiagnosticLog
import com.ferrex.android.core.media.WatchProgressTracker
import com.ferrex.android.core.watch.WatchService
import com.ferrex.android.core.watch.WatchStateCoordinator
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import javax.inject.Inject

private const val TAG = "PlayerVM"
private const val MAX_AUTO_RETRIES = 3

@HiltViewModel
class PlayerViewModel @Inject constructor(
    savedStateHandle: SavedStateHandle,
    private val serverConfig: ServerConfig,
    @StreamingClient val streamingClient: OkHttpClient,
    private val progressTracker: WatchProgressTracker,
    private val watchService: WatchService,
    private val watchStateCoordinator: WatchStateCoordinator,
) : ViewModel() {

    // Media ID from navigation args
    val mediaId: String = savedStateHandle.get<String>("mediaId") ?: ""
    private val forcedStartPositionMs: Long? = savedStateHandle.get<Long>("startPositionMs")

    private val _playerState = MutableStateFlow<PlayerState>(PlayerState.Loading)
    val playerState: StateFlow<PlayerState> = _playerState.asStateFlow()

    /** Tracks position across error/retry cycles so we resume, not restart. */
    private var lastKnownPositionMs: Long = 0L

    /** How many transparent retries we've done for the current failure burst. */
    private var autoRetryCount: Int = 0

    init {
        if (mediaId.isNotEmpty()) {
            prepareInitialPlayback()
        } else {
            DiagnosticLog.e(TAG, "No media ID provided")
            _playerState.value = PlayerState.Error(
                message = "No media ID provided",
                canRetry = false,
            )
        }
    }

    private fun prepareInitialPlayback() {
        forcedStartPositionMs?.let { explicitStartMs ->
            val startPositionMs = explicitStartMs.coerceAtLeast(0L)
            lastKnownPositionMs = startPositionMs
            preparePlayback(startPositionMs = startPositionMs)
            return
        }

        viewModelScope.launch {
            val startPositionMs = when (val result = watchService.getMediaProgress(mediaId)) {
                is ApiResult.Success -> {
                    val progress = result.data
                    if (progress != null && !progress.isCompleted && progress.position > 0.0) {
                        (progress.position * 1000.0).toLong().coerceAtLeast(0L)
                    } else {
                        0L
                    }
                }
                is ApiResult.HttpError -> {
                    DiagnosticLog.w(
                        TAG,
                        "Progress lookup failed for $mediaId: HTTP ${result.code} ${result.message}",
                    )
                    0L
                }
                is ApiResult.NetworkError -> {
                    DiagnosticLog.w(TAG, "Progress lookup failed for $mediaId", result.exception)
                    0L
                }
            }

            lastKnownPositionMs = startPositionMs
            preparePlayback(startPositionMs = startPositionMs)
        }
    }

    private fun preparePlayback(startPositionMs: Long) {
        viewModelScope.launch {
            try {
                _playerState.value = PlayerState.Loading

                val streamUrl = "${serverConfig.serverUrl}/api/v1/stream/$mediaId"
                DiagnosticLog.i(TAG, "Preparing playback: url=$streamUrl startPos=${startPositionMs}ms")

                // TODO: Fetch playback ticket for token-gated access
                // For v1, direct stream URL with auth header works
                _playerState.value = PlayerState.Ready(
                    streamUrl = streamUrl,
                    startPositionMs = startPositionMs,
                )
            } catch (e: Exception) {
                DiagnosticLog.e(TAG, "Failed to prepare playback", e)
                _playerState.value = PlayerState.Error(
                    message = e.localizedMessage ?: "Failed to prepare playback",
                    canRetry = true,
                )
            }
        }
    }

    /**
     * Called when ExoPlayer reports a playback error.
     *
     * If we haven't exhausted auto-retries, transparently re-prepares
     * playback from [lastPositionMs] after a progressive backoff delay.
     * The user sees a brief "Reconnecting…" loading state instead of an
     * error screen.
     *
     * After [MAX_AUTO_RETRIES] failures, surfaces the error to the user
     * with a manual retry button.
     */
    fun onPlayerError(message: String, lastPositionMs: Long) {
        lastKnownPositionMs = lastPositionMs.coerceAtLeast(0)
        autoRetryCount++

        DiagnosticLog.w(TAG,
            "Player error (attempt $autoRetryCount/$MAX_AUTO_RETRIES, " +
                "pos=${lastKnownPositionMs}ms): $message"
        )

        if (autoRetryCount <= MAX_AUTO_RETRIES) {
            // Transparent retry with progressive backoff
            viewModelScope.launch {
                _playerState.value = PlayerState.Loading
                val backoffMs = 1_500L * autoRetryCount
                DiagnosticLog.i(TAG, "Auto-retry in ${backoffMs}ms…")
                delay(backoffMs)
                preparePlayback(startPositionMs = lastKnownPositionMs)
            }
        } else {
            _playerState.value = PlayerState.Error(
                message = "$message\n\nPlayback failed after $MAX_AUTO_RETRIES retries.",
                canRetry = true,
            )
        }
    }

    /**
     * Manual retry (from the error screen button).
     * Resets the auto-retry counter so the user gets another full
     * round of transparent retries.
     */
    fun retry() {
        DiagnosticLog.i(TAG, "Manual retry from pos=${lastKnownPositionMs}ms")
        autoRetryCount = 0
        if (mediaId.isNotEmpty()) {
            preparePlayback(startPositionMs = lastKnownPositionMs)
        }
    }

    fun reportProgress(positionMs: Long, durationMs: Long) {
        if (mediaId.isEmpty() || durationMs <= 0) return

        // Track position for retry
        lastKnownPositionMs = positionMs

        viewModelScope.launch {
            progressTracker.reportProgress(
                mediaId = mediaId,
                positionSeconds = positionMs / 1000.0,
                durationSeconds = durationMs / 1000.0,
            )
        }
    }

    fun onPlaybackExit(positionMs: Long, durationMs: Long) {
        if (mediaId.isEmpty() || durationMs <= 0) return

        DiagnosticLog.i(TAG, "Playback exiting at ${positionMs}ms")
        lastKnownPositionMs = positionMs.coerceAtLeast(0L)

        reportAndInvalidate(
            positionMs = lastKnownPositionMs,
            durationMs = durationMs,
            reason = "playback exit",
        )
    }

    fun onPlaybackEnded(durationMs: Long) {
        if (mediaId.isEmpty() || durationMs <= 0) return

        DiagnosticLog.i(TAG, "Playback ended")
        lastKnownPositionMs = durationMs

        reportAndInvalidate(
            positionMs = durationMs,
            durationMs = durationMs,
            reason = "playback completion",
        )
    }

    private fun reportAndInvalidate(
        positionMs: Long,
        durationMs: Long,
        reason: String,
    ) {
        viewModelScope.launch {
            val reported = progressTracker.reportProgress(
                mediaId = mediaId,
                positionSeconds = positionMs / 1000.0,
                durationSeconds = durationMs / 1000.0,
            )
            if (reported) {
                watchStateCoordinator.notifyWatchStateChanged(reason)
            }
        }
    }
}

sealed interface PlayerState {
    data object Loading : PlayerState
    data class Ready(
        val streamUrl: String,
        val startPositionMs: Long = 0,
    ) : PlayerState
    data class Error(
        val message: String,
        val canRetry: Boolean = true,
    ) : PlayerState
}
