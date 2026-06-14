package com.ferrex.android.ui.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.statusTone
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.qa.FerrexVisualQaSamples
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
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

    @Test
    fun coreTextAndActionColorsMeetContrastTargets() {
        listOf(
            ContrastCheck("primary text on background", FerrexDesignTokens.Palette.TextPrimary, FerrexDesignTokens.Palette.SlateCanvas),
            ContrastCheck("secondary text on panel", FerrexDesignTokens.Palette.TextSecondary, FerrexDesignTokens.Palette.SlatePanel),
            ContrastCheck("muted/offline text on panel", FerrexDesignTokens.Palette.TextMuted, FerrexDesignTokens.Palette.SlatePanel),
            ContrastCheck("primary action content", FerrexDesignTokens.Palette.SlateBlack, FerrexDesignTokens.Palette.SignalCyan),
            ContrastCheck("secondary action accent", FerrexDesignTokens.Palette.PrivateViolet, FerrexDesignTokens.Palette.SlatePanel),
            ContrastCheck("destructive action content", FerrexDesignTokens.Palette.SlateBlack, FerrexDesignTokens.Palette.Error),
            ContrastCheck("error container copy", FerrexDesignTokens.Palette.TextPrimary, FerrexDesignTokens.Palette.ErrorDim),
            ContrastCheck("focus wash copy", FerrexDesignTokens.Palette.TextPrimary, FerrexDesignTokens.Palette.FocusWash.compositeOver(FerrexDesignTokens.Palette.SlatePanel, alpha = 0x33 / 255f)),
        ).forEach { it.assertPasses() }
    }

    @Test
    fun statusToneContrastSamplesCoverEveryToneAndMeetContrastTargets() {
        val samples = FerrexVisualQaSamples.statusToneSamples

        assertEquals(FerrexStatusTone.entries.toSet(), samples.map { it.tone }.toSet())
        assertEquals(FerrexActionRole.entries.toSet(), samples.map { it.actionRole }.toSet())
        assertEquals(samples.size, samples.map { it.testTag }.distinct().size)

        samples.forEach { sample ->
            assertEquals(sample.tone, sample.actionRole.statusTone())
            assertTrue("${sample.id} tag must be stable", sample.testTag.isNotBlank())
            assertTrue("${sample.id} description must be stable", sample.contentDescription.isNotBlank())

            val blendedContainer = sample.container.compositeOver(sample.blendBackground)
            ContrastCheck("${sample.id} content", sample.content, blendedContainer).assertPasses()
            ContrastCheck("${sample.id} accent", sample.accent, blendedContainer).assertPasses()
        }
    }

    @Test
    fun visualQaSurfaceSamplesExposeStableTagsAndDescriptions() {
        val samples = FerrexVisualQaSamples.phoneSurfaces + FerrexVisualQaSamples.tvFocusableSurfaces

        assertEquals(samples.size, samples.map { it.id }.distinct().size)
        assertEquals(samples.size, samples.map { it.testTag }.distinct().size)
        samples.forEach { sample ->
            assertTrue("${sample.id} tag", sample.testTag.matches(Regex("[a-z0-9_.-]+")))
            assertTrue("${sample.id} description", sample.contentDescription.length >= 16)
            assertTrue("${sample.id} evidence path", sample.evidencePath.isNotBlank())
        }
    }

    @Test
    fun dynamicQaTagsAreSanitizedAndNamespaced() {
        assertEquals("tv.action.library-tabs.tab-movies", FerrexQaTags.Tv.action("library tabs", "tab:Movies"))
        assertEquals("tv.poster.continue-watching.movie-101", FerrexQaTags.Tv.poster("Continue Watching", "movie:101"))
        assertEquals("phone.shell.nav.accountserver", FerrexQaTags.Phone.navItem("AccountServer"))
        assertEquals("status-card.stale-offline", FerrexQaTags.Shared.statusCard("Stale / Offline"))
    }

    private data class ContrastCheck(
        val name: String,
        val foreground: Color,
        val background: Color,
        val minimumRatio: Double = WCAG_AA_NORMAL_TEXT_RATIO,
    ) {
        fun assertPasses() {
            val actual = foreground.contrastRatio(background)
            assertTrue("$name contrast $actual < $minimumRatio", actual >= minimumRatio)
        }
    }
}

private const val WCAG_AA_NORMAL_TEXT_RATIO = 4.5

private fun Color.contrastRatio(other: Color): Double {
    val first = relativeLuminance()
    val second = other.relativeLuminance()
    return (max(first, second) + 0.05) / (min(first, second) + 0.05)
}

private fun Color.relativeLuminance(): Double {
    fun channel(component: Float): Double {
        val value = component.toDouble()
        return if (value <= 0.03928) value / 12.92 else ((value + 0.055) / 1.055).pow(2.4)
    }
    return 0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
}

private fun Color.compositeOver(background: Color, alpha: Float = this.alpha): Color {
    val outAlpha = alpha + background.alpha * (1f - alpha)
    if (outAlpha == 0f) return Color.Transparent
    return Color(
        red = ((red * alpha) + (background.red * background.alpha * (1f - alpha))) / outAlpha,
        green = ((green * alpha) + (background.green * background.alpha * (1f - alpha))) / outAlpha,
        blue = ((blue * alpha) + (background.blue * background.alpha * (1f - alpha))) / outAlpha,
        alpha = outAlpha,
    )
}
