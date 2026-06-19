package com.ferrex.android.ui.recovery

import com.ferrex.android.ui.diagnostics.PhoneDiagnosticsPresentation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneRecoveryPresentationTest {
    @Test
    fun diagnosticsActionsStayFlatWithoutVisibleBackButton() {
        assertNull(PhoneDiagnosticsPresentation.VisibleBackActionLabel)
        assertEquals(
            listOf("Export / Share diagnostics", "Clear diagnostics/logs"),
            PhoneDiagnosticsPresentation.actionLabels(exportRunning = false, clearRunning = false),
        )
        assertEquals(
            listOf("Preparing export…", "Clearing diagnostics…"),
            PhoneDiagnosticsPresentation.actionLabels(exportRunning = true, clearRunning = true),
        )
    }

    @Test
    fun diagnosticsStatusBandsExposeStableTagsAndMergedDescriptions() {
        val tag = PhoneDiagnosticsPresentation.statusTag("Diagnostics action failed")
        val description = PhoneDiagnosticsPresentation.statusDescription(
            title = "Diagnostics action failed",
            body = "Retry or use Android back.",
        )

        assertEquals("phone.diagnostics.status.diagnostics-action-failed", tag)
        assertEquals("Diagnostics action failed. Retry or use Android back.", description)
    }

    @Test
    fun authRecoveryStatusBandsKeepInlineSemanticsWithoutVisibleBackButton() {
        assertNull(PhoneRecoveryPresentation.VisibleBackActionLabel)
        assertEquals("phone.recovery.status.sign-in", PhoneRecoveryPresentation.statusTag("sign-in"))
        assertTrue(
            PhoneRecoveryPresentation.statusDescription("Sign-in status", "Session expired.")
                .contains("Sign-in status. Session expired."),
        )
    }
}
