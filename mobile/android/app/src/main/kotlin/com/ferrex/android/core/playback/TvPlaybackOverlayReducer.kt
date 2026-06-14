package com.ferrex.android.core.playback

enum class TvTrackPickerKind {
    Audio,
    Subtitles,
}

data class TvPlaybackOverlayUiState(
    val controlsVisible: Boolean = true,
    val picker: TvTrackPickerKind? = null,
    val isPlaying: Boolean = false,
)

enum class TvPlaybackOverlayEvent {
    Back,
    DpadCenter,
    DpadLeft,
    DpadRight,
    DpadVertical,
    AutoHideTimeout,
    OpenAudioPicker,
    OpenSubtitlePicker,
    PickerDismissed,
    PickerSelected,
    PlaybackStarted,
    PlaybackStopped,
}

enum class TvPlaybackOverlayEffect {
    None,
    ExitPlayback,
    TogglePlayPause,
    SeekBackward,
    SeekForward,
    RestoreSafeFocus,
}

object TvPlaybackOverlayReducer {
    fun reduce(
        state: TvPlaybackOverlayUiState,
        event: TvPlaybackOverlayEvent,
    ): Pair<TvPlaybackOverlayUiState, TvPlaybackOverlayEffect> = when (event) {
        TvPlaybackOverlayEvent.Back -> when {
            state.picker != null -> state.copy(picker = null, controlsVisible = true) to TvPlaybackOverlayEffect.RestoreSafeFocus
            !state.controlsVisible -> state.copy(controlsVisible = true) to TvPlaybackOverlayEffect.RestoreSafeFocus
            else -> state to TvPlaybackOverlayEffect.ExitPlayback
        }

        TvPlaybackOverlayEvent.DpadCenter -> if (state.picker == null && !state.controlsVisible) {
            state.copy(controlsVisible = true) to TvPlaybackOverlayEffect.TogglePlayPause
        } else {
            state to TvPlaybackOverlayEffect.None
        }

        TvPlaybackOverlayEvent.DpadLeft -> if (state.picker == null && !state.controlsVisible) {
            state.copy(controlsVisible = true) to TvPlaybackOverlayEffect.SeekBackward
        } else {
            state to TvPlaybackOverlayEffect.None
        }

        TvPlaybackOverlayEvent.DpadRight -> if (state.picker == null && !state.controlsVisible) {
            state.copy(controlsVisible = true) to TvPlaybackOverlayEffect.SeekForward
        } else {
            state to TvPlaybackOverlayEffect.None
        }

        TvPlaybackOverlayEvent.DpadVertical -> if (state.picker == null && !state.controlsVisible) {
            state.copy(controlsVisible = true) to TvPlaybackOverlayEffect.RestoreSafeFocus
        } else {
            state to TvPlaybackOverlayEffect.None
        }

        TvPlaybackOverlayEvent.AutoHideTimeout -> if (state.isPlaying && state.controlsVisible && state.picker == null) {
            state.copy(controlsVisible = false) to TvPlaybackOverlayEffect.None
        } else {
            state to TvPlaybackOverlayEffect.None
        }

        TvPlaybackOverlayEvent.OpenAudioPicker -> state.copy(
            controlsVisible = true,
            picker = TvTrackPickerKind.Audio,
        ) to TvPlaybackOverlayEffect.None

        TvPlaybackOverlayEvent.OpenSubtitlePicker -> state.copy(
            controlsVisible = true,
            picker = TvTrackPickerKind.Subtitles,
        ) to TvPlaybackOverlayEffect.None

        TvPlaybackOverlayEvent.PickerDismissed,
        TvPlaybackOverlayEvent.PickerSelected -> state.copy(
            controlsVisible = true,
            picker = null,
        ) to TvPlaybackOverlayEffect.RestoreSafeFocus

        TvPlaybackOverlayEvent.PlaybackStarted -> state.copy(isPlaying = true) to TvPlaybackOverlayEffect.None
        TvPlaybackOverlayEvent.PlaybackStopped -> state.copy(
            isPlaying = false,
            controlsVisible = true,
            picker = null,
        ) to TvPlaybackOverlayEffect.RestoreSafeFocus
    }
}
