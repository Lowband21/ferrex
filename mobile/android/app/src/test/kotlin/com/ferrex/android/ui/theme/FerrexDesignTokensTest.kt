package com.ferrex.android.ui.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexRecoveryActionKind
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.TheaterPlateComponentMigrationNotes
import com.ferrex.android.ui.components.TheaterPlateDensityRole
import com.ferrex.android.ui.components.TheaterPlateTypographyGroup
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.components.defaultMaxLines
import com.ferrex.android.ui.components.requiredTheaterPlateRecoveryActions
import com.ferrex.android.ui.components.spec
import com.ferrex.android.ui.components.statusTone
import com.ferrex.android.ui.components.theaterPlateDensityForViewport
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
    fun theaterPlateDensityRolesCoverPhoneFoldableAndTvProfiles() {
        assertEquals(TheaterPlateDensityRole.PhonePortrait, theaterPlateDensityForViewport(393, 852, isTv = false))
        assertEquals(TheaterPlateDensityRole.PhoneLandscape, theaterPlateDensityForViewport(900, 540, isTv = false))
        assertEquals(TheaterPlateDensityRole.PhoneLandscape, theaterPlateDensityForViewport(840, 1200, isTv = false))
        assertEquals(TheaterPlateDensityRole.Tv1080p, theaterPlateDensityForViewport(1920, 1080, isTv = true))
        assertEquals(TheaterPlateDensityRole.Tv4kScaled, theaterPlateDensityForViewport(3840, 2160, isTv = true))

        val specs = TheaterPlateDensityRole.entries.associateWith { it.spec() }
        assertEquals(TheaterPlateDensityRole.entries.toSet(), specs.keys)
        assertTrue(specs.getValue(TheaterPlateDensityRole.PhonePortrait).minInteractiveHeight >= 48.dp)
        assertTrue(specs.getValue(TheaterPlateDensityRole.Tv1080p).minInteractiveHeight >= FerrexDesignTokens.Focus.TvButtonMinHeight)
        assertTrue(specs.getValue(TheaterPlateDensityRole.Tv4kScaled).contentMaxWidth > specs.getValue(TheaterPlateDensityRole.Tv1080p).contentMaxWidth)
        assertTrue(specs.getValue(TheaterPlateDensityRole.Tv4kScaled).typeScale > specs.getValue(TheaterPlateDensityRole.PhonePortrait).typeScale)
    }

    @Test
    fun theaterPlateTypographyRolesCoverEditorialRecoveryAndTvFocusCopy() {
        assertEquals(17, TheaterPlateTypographyRole.entries.size)
        assertEquals(
            TheaterPlateTypographyGroup.entries.toSet(),
            TheaterPlateTypographyRole.entries.map { it.group }.toSet(),
        )
        assertEquals(
            setOf(
                "hero-eyebrow",
                "hero-title",
                "hero-subtitle",
                "hero-body",
                "metadata",
                "section-title",
                "fact-label",
                "fact-value",
                "rail-title",
                "rail-subtitle",
                "action-label",
                "action-subtitle",
                "status-title",
                "status-copy",
                "recovery-title",
                "recovery-copy",
                "tv-focus-helper-label",
            ),
            TheaterPlateTypographyRole.entries.map { it.key }.toSet(),
        )
        assertTrue(TheaterPlateTypographyRole.HeroTitle.defaultMaxLines(TheaterPlateDensityRole.PhonePortrait) >= 3)
        assertTrue(TheaterPlateTypographyRole.TvFocusHelperLabel.defaultMaxLines(TheaterPlateDensityRole.Tv1080p) >= 2)
    }

    @Test
    fun requiredRecoveryActionsPreserveTonesAndAvoidAppDataWipes() {
        val actions = requiredTheaterPlateRecoveryActions()

        assertEquals(FerrexRecoveryActionKind.entries.map { it.key }, actions.map { it.key })
        assertEquals(FerrexActionRole.Retry, actions.first { it.kind == FerrexRecoveryActionKind.Retry }.role)
        assertEquals(FerrexActionRole.Cache, actions.first { it.kind == FerrexRecoveryActionKind.ClearCache }.role)
        assertEquals(FerrexActionRole.DestructiveReset, actions.first { it.kind == FerrexRecoveryActionKind.ResetConnection }.role)
        assertEquals(FerrexStatusTone.DestructiveReset, actions.first { it.kind == FerrexRecoveryActionKind.ResetConnection }.tone)
        actions.forEach { action ->
            assertFalse("${action.key} must not require app-data wipes", action.requiresAppDataWipe)
            assertFalse(action.label.contains("wipe", ignoreCase = true))
            assertFalse(action.label.contains("clear app data", ignoreCase = true))
            assertTrue("${action.key} subtitle", action.subtitle.isNotBlank())
            assertEquals(action.role.statusTone(), action.tone)
        }
        assertEquals(
            listOf("retry", "sign-out", "change-server", "reset-connection", "diagnostics"),
            requiredTheaterPlateRecoveryActions(includeCacheClear = false).map { it.key },
        )
    }

    @Test
    fun theaterPlateMigrationNotesPreserveRouteCallbacksForFutureStacks() {
        val notes = TheaterPlateComponentMigrationNotes.all.associateBy { it.componentName }

        assertEquals(
            setOf("FerrexStatusCard", "FerrexActionButton", "FerrexPosterCard", "TV focus actions"),
            notes.keys,
        )
        assertTrue(notes.getValue("FerrexStatusCard").preservedCallbacks.contains("FerrexStatusAction.onClick"))
        assertTrue(notes.getValue("FerrexActionButton").preservedCallbacks.contains("onClick"))
        assertTrue(notes.getValue("FerrexPosterCard").preservedCallbacks.contains("onClick"))
        assertTrue(notes.getValue("TV focus actions").preservedCallbacks.contains("focus restoration key"))
        assertTrue(notes.getValue("FerrexPosterCard").migrationNote.contains("LOW-447"))
        assertTrue(notes.getValue("FerrexStatusCard").migrationNote.contains("LOW-448"))
        assertTrue(notes.getValue("TV focus actions").migrationNote.contains("LOW-449"))
        notes.values.forEach { note ->
            assertTrue(note.foundationSeam.isNotBlank())
            assertTrue(note.preservedCallbacks.size >= 4)
            assertTrue(note.migrationNote.isNotBlank())
        }
    }

    @Test
    fun tvFocusTreatmentsKeepMediaArtGroundingDeterministic() {
        val action = FerrexDesignTokens.Focus.tvTreatment(TvFocusTreatmentRole.Action)
        val mediaArt = FerrexDesignTokens.Focus.tvTreatment(TvFocusTreatmentRole.MediaArt)
        val recovery = FerrexDesignTokens.Focus.tvTreatment(TvFocusTreatmentRole.Recovery)
        val destructive = FerrexDesignTokens.Focus.tvTreatment(TvFocusTreatmentRole.Destructive)
        val helper = FerrexDesignTokens.Focus.tvTreatment(TvFocusTreatmentRole.Helper)

        assertEquals(FerrexDesignTokens.Focus.TvFocusedScale, action.focusedScale)
        assertEquals(FerrexDesignTokens.Focus.TvFocusedBorder, action.focusedBorder)
        assertTrue(mediaArt.focusedScale < action.focusedScale)
        assertTrue(mediaArt.focusedBorder < action.focusedBorder)
        assertTrue(mediaArt.mediaGroundingClearance > mediaArt.focusedBorder)
        assertEquals(FerrexDesignTokens.Focus.TvFocusedBorder, recovery.focusedBorder)
        assertEquals(FerrexDesignTokens.Focus.TvFocusedElevation, destructive.focusedElevation)
        assertTrue(helper.focusedElevation < action.focusedElevation)
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
        assertEquals("phone.theater-plate.action.playback-entry.primary", FerrexQaTags.TheaterPlate.action("phone", "Playback Entry", "Primary"))
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
