package com.ferrex.android.ui.player

import com.ferrex.android.core.playback.PlaybackFailure
import com.ferrex.android.core.playback.PlaybackFailureKind
import com.ferrex.android.core.playback.PlaybackRecoveryActions
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhonePlayerRecoveryPresentationTest {
    @Test
    fun loadingOverlayUsesFlatInlineTreatmentAndKeepsRecoveryActions() {
        val presentation = PhonePlaybackRecoveryPresenter.loading(
            message = "Preparing cached stream",
            diagnosticsAvailable = true,
        )

        assertEquals(PhonePlaybackPanelTreatment.InlineFlat, presentation.treatment)
        assertTrue(presentation.contentDescription.contains("Preparing cached stream"))
        assertEquals(
            listOf(
                "Back to details",
                "Change server",
                "Sign out",
                "Diagnostics / Export diagnostics",
            ),
            presentation.actions.map { it.label },
        )
    }

    @Test
    fun errorOverlayKeepsRetryRecoveryAndAuthCopyWithoutRoundedPanelContract() {
        val failure = PlaybackFailure(
            kind = PlaybackFailureKind.Unauthorized,
            message = "Playback authorization expired.",
            httpStatusCode = 401,
        )

        val presentation = PhonePlaybackRecoveryPresenter.error(
            failure = failure,
            actions = PlaybackRecoveryActions.forFailure(failure),
            diagnosticsAvailable = true,
        )

        assertEquals(PhonePlaybackPanelTreatment.InlineFlat, presentation.treatment)
        assertTrue(presentation.supportingText.contains("HTTP 401"))
        assertTrue(presentation.supportingText.contains("Change server and Sign out remain available"))
        assertTrue(presentation.actions.any { it.label == "Retry playback" })
        assertTrue(presentation.actions.any { it.label == "Change server" })
        assertTrue(presentation.actions.any { it.label == "Sign out" })
        assertTrue(presentation.actions.any { it.label == "Diagnostics / Export diagnostics" })
    }

    @Test
    fun diagnosticsActionIsOmittedWhenUnavailableButCoreRecoveryStaysVisible() {
        val failure = PlaybackFailure(
            kind = PlaybackFailureKind.Network,
            message = "Network unavailable.",
        )

        val presentation = PhonePlaybackRecoveryPresenter.error(
            failure = failure,
            actions = PlaybackRecoveryActions.forFailure(failure),
            diagnosticsAvailable = false,
        )

        assertFalse(presentation.actions.any { it.label == "Diagnostics / Export diagnostics" })
        assertTrue(presentation.actions.any { it.label == "Retry playback" })
        assertTrue(presentation.actions.any { it.label == "Back to details" })
        assertTrue(presentation.actions.any { it.label == "Change server" })
        assertTrue(presentation.actions.any { it.label == "Sign out" })
    }
}
