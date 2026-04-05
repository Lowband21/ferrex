package com.ferrex.android.ui.player

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.api.StreamingClient
import com.ferrex.android.core.media.WatchProgressTracker
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import javax.inject.Inject

@HiltViewModel
class PlayerViewModel @Inject constructor(
    savedStateHandle: SavedStateHandle,
    private val serverConfig: ServerConfig,
    @StreamingClient val streamingClient: OkHttpClient,
    private val progressTracker: WatchProgressTracker,
) : ViewModel() {

    // Media ID from navigation args
    val mediaId: String = savedStateHandle.get<String>("mediaId") ?: ""

    private val _playerState = MutableStateFlow<PlayerState>(PlayerState.Loading)
    val playerState: StateFlow<PlayerState> = _playerState.asStateFlow()

    init {
        if (mediaId.isNotEmpty()) {
            fetchPlaybackTicket()
        }
    }

    private fun fetchPlaybackTicket() {
        viewModelScope.launch {
            try {
                _playerState.value = PlayerState.Loading

                // For direct play, the stream URL is simply /api/v1/stream/{id}
                // The auth token is injected by the OkHttp interceptor
                val streamUrl = "${serverConfig.serverUrl}/api/v1/stream/$mediaId"

                // TODO: Fetch playback ticket for token-gated access
                // For v1, direct stream URL with auth header works
                _playerState.value = PlayerState.Ready(
                    streamUrl = streamUrl,
                    startPositionMs = 0L, // TODO: load from watch state
                )
            } catch (e: Exception) {
                _playerState.value = PlayerState.Error(
                    e.localizedMessage ?: "Failed to prepare playback"
                )
            }
        }
    }

    fun reportProgress(positionMs: Long, durationMs: Long) {
        if (mediaId.isEmpty() || durationMs <= 0) return

        viewModelScope.launch {
            progressTracker.reportProgress(
                mediaId = mediaId,
                positionSeconds = positionMs / 1000.0,
                durationSeconds = durationMs / 1000.0,
            )
        }
    }

    fun onPlaybackEnded() {
        // Report final progress
        viewModelScope.launch {
            progressTracker.reportProgress(
                mediaId = mediaId,
                positionSeconds = -1.0, // Server interprets as completed
                durationSeconds = -1.0,
            )
        }
    }
}

sealed interface PlayerState {
    data object Loading : PlayerState
    data class Ready(
        val streamUrl: String,
        val startPositionMs: Long = 0,
    ) : PlayerState
    data class Error(val message: String) : PlayerState
}
