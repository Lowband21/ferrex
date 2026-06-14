package com.ferrex.android.ui.theme

import androidx.compose.ui.graphics.Color
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.statusTone
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class FerrexDesignTokensTest {
    @Test
    fun paletteDoesNotReuseLegacyMobileOrangeOrTvCyan() {
        assertFalse(FerrexDesignTokens.Palette.SignalCyan == Color(0xFFFFB35C))
        assertFalse(FerrexDesignTokens.Palette.SignalCyan == Color(0xFF83D6FF))
        assertFalse(FerrexDesignTokens.Palette.PrivateViolet == Color(0xFFFFB35C))
    }

    @Test
    fun recoveryActionRolesMapToDistinctStatusTones() {
        assertEquals(FerrexStatusTone.Primary, FerrexActionRole.Primary.statusTone())
        assertEquals(FerrexStatusTone.Secondary, FerrexActionRole.Secondary.statusTone())
        assertEquals(FerrexStatusTone.Retry, FerrexActionRole.Retry.statusTone())
        assertEquals(FerrexStatusTone.DestructiveReset, FerrexActionRole.DestructiveReset.statusTone())
        assertEquals(FerrexStatusTone.Cache, FerrexActionRole.Cache.statusTone())
        assertEquals(FerrexStatusTone.StaleOffline, FerrexActionRole.StaleOffline.statusTone())
        assertEquals(FerrexStatusTone.Error, FerrexActionRole.Error.statusTone())
    }
}
