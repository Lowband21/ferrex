package com.ferrex.android.core.playback

import org.junit.Assert.assertEquals
import org.junit.Test

class TvPlaybackOverlayReducerTest {
    @Test
    fun backShowsHiddenControlsBeforeExiting() {
        val hidden = TvPlaybackOverlayUiState(controlsVisible = false, isPlaying = true)

        val (shown, firstEffect) = TvPlaybackOverlayReducer.reduce(hidden, TvPlaybackOverlayEvent.Back)
        val (_, secondEffect) = TvPlaybackOverlayReducer.reduce(shown, TvPlaybackOverlayEvent.Back)

        assertEquals(true, shown.controlsVisible)
        assertEquals(TvPlaybackOverlayEffect.RestoreSafeFocus, firstEffect)
        assertEquals(TvPlaybackOverlayEffect.ExitPlayback, secondEffect)
    }

    @Test
    fun hiddenDpadCenterAndSeekKeysProducePlaybackEffectsAndShowControls() {
        val hidden = TvPlaybackOverlayUiState(controlsVisible = false, isPlaying = true)

        val (centerState, centerEffect) = TvPlaybackOverlayReducer.reduce(hidden, TvPlaybackOverlayEvent.DpadCenter)
        val (leftState, leftEffect) = TvPlaybackOverlayReducer.reduce(hidden, TvPlaybackOverlayEvent.DpadLeft)
        val (rightState, rightEffect) = TvPlaybackOverlayReducer.reduce(hidden, TvPlaybackOverlayEvent.DpadRight)

        assertEquals(true, centerState.controlsVisible)
        assertEquals(TvPlaybackOverlayEffect.TogglePlayPause, centerEffect)
        assertEquals(true, leftState.controlsVisible)
        assertEquals(TvPlaybackOverlayEffect.SeekBackward, leftEffect)
        assertEquals(true, rightState.controlsVisible)
        assertEquals(TvPlaybackOverlayEffect.SeekForward, rightEffect)
    }

    @Test
    fun pickerBackAndSelectionClosePickerAndRestoreFocus() {
        val pickerOpen = TvPlaybackOverlayUiState(
            controlsVisible = true,
            picker = TvTrackPickerKind.Subtitles,
            isPlaying = true,
        )

        val (dismissed, dismissEffect) = TvPlaybackOverlayReducer.reduce(pickerOpen, TvPlaybackOverlayEvent.Back)
        val (selected, selectEffect) = TvPlaybackOverlayReducer.reduce(pickerOpen, TvPlaybackOverlayEvent.PickerSelected)

        assertEquals(null, dismissed.picker)
        assertEquals(true, dismissed.controlsVisible)
        assertEquals(TvPlaybackOverlayEffect.RestoreSafeFocus, dismissEffect)
        assertEquals(null, selected.picker)
        assertEquals(TvPlaybackOverlayEffect.RestoreSafeFocus, selectEffect)
    }

    @Test
    fun autoHideOnlyRunsWhilePlayingAndNoPickerIsOpen() {
        val playing = TvPlaybackOverlayUiState(controlsVisible = true, isPlaying = true)
        val paused = TvPlaybackOverlayUiState(controlsVisible = true, isPlaying = false)
        val picker = TvPlaybackOverlayUiState(controlsVisible = true, picker = TvTrackPickerKind.Audio, isPlaying = true)

        val (hidden, _) = TvPlaybackOverlayReducer.reduce(playing, TvPlaybackOverlayEvent.AutoHideTimeout)
        val (pausedAfter, _) = TvPlaybackOverlayReducer.reduce(paused, TvPlaybackOverlayEvent.AutoHideTimeout)
        val (pickerAfter, _) = TvPlaybackOverlayReducer.reduce(picker, TvPlaybackOverlayEvent.AutoHideTimeout)

        assertEquals(false, hidden.controlsVisible)
        assertEquals(true, pausedAfter.controlsVisible)
        assertEquals(true, pickerAfter.controlsVisible)
        assertEquals(TvTrackPickerKind.Audio, pickerAfter.picker)
    }
}
