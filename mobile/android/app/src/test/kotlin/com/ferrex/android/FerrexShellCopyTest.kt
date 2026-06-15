package com.ferrex.android

import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FerrexShellCopyTest {
    @Test
    fun mobileAndTvShellsUseDistinctRecoveryCopy() {
        assertNotEquals(FerrexShellCopy.MOBILE_TITLE, FerrexShellCopy.TV_TITLE)
        assertNotEquals(FerrexShellCopy.MOBILE_SUBTITLE, FerrexShellCopy.TV_SUBTITLE)
    }

    @Test
    fun shellCopyNamesRecoveryExits() {
        assertTrue(FerrexShellCopy.MOBILE_BODY.contains("recovery exits"))
        assertTrue(FerrexShellCopy.TV_BODY.contains("recovery actions"))
    }
}
