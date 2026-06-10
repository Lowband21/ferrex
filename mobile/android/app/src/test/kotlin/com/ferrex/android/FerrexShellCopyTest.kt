package com.ferrex.android

import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FerrexShellCopyTest {
    @Test
    fun mobileAndTvShellsUseDistinctEntryCopy() {
        assertNotEquals(FerrexShellCopy.MOBILE_TITLE, FerrexShellCopy.TV_TITLE)
        assertNotEquals(FerrexShellCopy.MOBILE_SUBTITLE, FerrexShellCopy.TV_SUBTITLE)
    }

    @Test
    fun shellCopyIdentifiesBuildVariants() {
        assertTrue(FerrexShellCopy.MOBILE_BODY.contains("Mobile"))
        assertTrue(FerrexShellCopy.TV_BODY.contains("10-foot"))
    }
}
