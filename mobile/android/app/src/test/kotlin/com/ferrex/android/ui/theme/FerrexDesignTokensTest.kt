package com.ferrex.android.ui.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
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

    @Test
    fun tvTokensPreserveTenFootScaleWhileSharingThePalette() {
        assertEquals(56.dp, FerrexDesignTokens.Space.ScreenTvHorizontal)
        assertEquals(40.dp, FerrexDesignTokens.Space.ScreenTvVertical)
        assertEquals(190.dp, FerrexDesignTokens.Poster.TvWidth)
        assertEquals(190.dp, FerrexDesignTokens.Poster.TvGridMin)
        assertEquals(338.dp, FerrexDesignTokens.Poster.TvCardMinHeight)
        assertEquals(1560.dp, FerrexDesignTokens.Tv.HomeMaxWidth)
        assertEquals(1320.dp, FerrexDesignTokens.Tv.DetailMaxWidth)
    }
}
