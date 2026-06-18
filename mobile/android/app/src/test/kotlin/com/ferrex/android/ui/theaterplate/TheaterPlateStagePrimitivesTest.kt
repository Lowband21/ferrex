package com.ferrex.android.ui.theaterplate

import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.theaterplate.TheaterPlateAnalysis
import com.ferrex.android.core.theaterplate.TheaterPlateColor
import com.ferrex.android.core.theaterplate.TheaterPlateDownsample
import com.ferrex.android.core.theaterplate.TheaterPlateGrade
import com.ferrex.android.core.theaterplate.TheaterPlateGradeClass
import com.ferrex.android.core.theaterplate.TheaterPlateGradeControls
import com.ferrex.android.core.theaterplate.TheaterPlateLocalLuma
import com.ferrex.android.core.theaterplate.TheaterPlatePalette
import com.ferrex.android.core.theaterplate.TheaterPlateSourceContext
import com.ferrex.android.core.theaterplate.TheaterPlateViewport
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TheaterPlateStagePrimitivesTest {
    @Test
    fun surfaceVariantsMapToSemanticLowRadiusTokens() {
        val denseVariants = FerrexStageSurfaceVariant.entries
            .filter { it.tokenSpec(FerrexStageDensityFamily.Standard).denseBand }
            .toSet()

        assertEquals(
            setOf(
                FerrexStageSurfaceVariant.ControlShelf,
                FerrexStageSurfaceVariant.RailBand,
                FerrexStageSurfaceVariant.FactRibbon,
                FerrexStageSurfaceVariant.StatusSlab,
            ),
            denseVariants,
        )
        assertEquals(FerrexStageSurfaceTone.Primary, FerrexStageSurfaceVariant.ControlShelf.defaultTone())
        assertEquals(FerrexStageSurfaceTone.StaleOffline, FerrexStageSurfaceVariant.EmptyState.defaultTone())
        assertEquals(FerrexStageSurfaceTone.Warning, FerrexStageSurfaceVariant.NoticeSlab.defaultTone())

        FerrexStageDensityFamily.entries.forEach { density ->
            FerrexStageSurfaceVariant.entries.forEach { variant ->
                val token = variant.tokenSpec(density)

                assertEquals(variant, token.variant)
                assertEquals(density, token.density)
                assertEquals(variant.semanticName, token.semanticName)
                assertTrue("${variant.name} semantic name", token.semanticName.isNotBlank())
                assertTrue("${variant.name} radius should stay low", token.cornerRadius.value <= 14f)
                assertFinitePositive("${variant.name} horizontal padding", token.horizontalPadding)
                assertFinitePositive("${variant.name} vertical padding", token.verticalPadding)
                assertFinitePositive("${variant.name} min height", token.minHeight)
                assertTrue("${variant.name} container alpha", token.containerAlpha in 0f..1f)
                assertTrue("${variant.name} border alpha", token.borderAlpha in 0f..1f)
            }
        }
    }

    @Test
    fun gradeControlsMapToFiniteStageVisuals() {
        TheaterPlateGradeClass.entries.forEach { gradeClass ->
            val controls = controlsFor(gradeClass)
            val visuals = TheaterPlateStageVisuals.fromAnalysis(analysisFor(gradeClass, controls))

            assertTrue("$gradeClass finite controls", visuals.finiteFloatValues().all { it.isFinite() && it in 0f..1f })
            assertEquals(controls.highlightCompression, visuals.highlightCompression, 0.0001f)
            assertEquals(controls.desaturation.coerceIn(0f, 1f), visuals.desaturation, 0.0001f)
            assertTrue("$gradeClass ambient colors", visuals.ambientColors.size >= 4)
            if (gradeClass == TheaterPlateGradeClass.MissingBackdrop) {
                assertEquals(TheaterPlateBackdropAdaptation.MissingBackdrop, visuals.adaptation)
                assertEquals("Missing backdrop", visuals.explicitStateLabel)
                assertEquals(0f, visuals.backdropOpacity, 0.0001f)
            } else {
                assertEquals(TheaterPlateBackdropAdaptation.Ready, visuals.adaptation)
                assertEquals(null, visuals.explicitStateLabel)
                assertTrue("$gradeClass backdrop opacity", visuals.backdropOpacity > 0f)
            }
        }
    }

    @Test
    fun staleAndLowQualityAdaptationsAddLabelsAndSaferContrast() {
        val analysis = analysisFor(TheaterPlateGradeClass.Balanced, controlsFor(TheaterPlateGradeClass.Balanced))
        val ready = TheaterPlateStageVisuals.fromAnalysis(analysis, TheaterPlateBackdropAdaptation.Ready)
        val stale = TheaterPlateStageVisuals.fromAnalysis(analysis, TheaterPlateBackdropAdaptation.StaleOffline)
        val lowQuality = TheaterPlateStageVisuals.fromAnalysis(analysis, TheaterPlateBackdropAdaptation.LowQuality)

        assertEquals("Stale/offline artwork", stale.explicitStateLabel)
        assertEquals("Low-quality backdrop", lowQuality.explicitStateLabel)
        assertTrue(stale.scrimOpacity >= ready.scrimOpacity)
        assertTrue(stale.ambientOpacity >= ready.ambientOpacity)
        assertTrue(stale.backdropOpacity < ready.backdropOpacity)
        assertTrue(stale.desaturation >= 0.20f)
        assertTrue(lowQuality.scrimOpacity >= ready.scrimOpacity)
        assertTrue(lowQuality.backdropOpacity < ready.backdropOpacity)
    }

    @Test
    fun densityFamiliesScaleFromViewportClass() {
        assertEquals(FerrexStageDensityFamily.Compact, FerrexStageDensityFamily.forViewport(TheaterPlateViewport.of(800, 600)))
        assertEquals(FerrexStageDensityFamily.Standard, FerrexStageDensityFamily.forViewport(TheaterPlateViewport.of(1280, 720)))
        assertEquals(FerrexStageDensityFamily.TenFoot, FerrexStageDensityFamily.forViewport(TheaterPlateViewport.of(1920, 1080)))

        val compact = FerrexStageDensityFamily.Compact.tokens()
        val standard = FerrexStageDensityFamily.Standard.tokens()
        val tenFoot = FerrexStageDensityFamily.TenFoot.tokens()

        assertTrue(compact.outerPaddingHorizontal < standard.outerPaddingHorizontal)
        assertTrue(standard.outerPaddingHorizontal < tenFoot.outerPaddingHorizontal)
        assertTrue(standard.contentGap < tenFoot.contentGap)
        assertTrue(compact.backdropBandMinHeight < tenFoot.backdropBandMinHeight)
        assertTrue(standard.minInteractiveSize < tenFoot.minInteractiveSize)
    }

    @Test
    fun layoutSpecsNeverEmitNaNOrInfiniteValues() {
        val hostileViewports = listOf(
            Float.NaN to Float.POSITIVE_INFINITY,
            -80f to 0f,
            1f to 1f,
            3840f to 2160f,
        )

        FerrexStageDensityFamily.entries.forEach { density ->
            hostileViewports.forEach { (width, height) ->
                val spec = TheaterPlateStageLayoutSpec.forViewport(width, height, density)

                assertTrue(
                    "$density $width x $height finite",
                    spec.finiteFloatValues().all { it.isFinite() && it >= 0f },
                )
                assertTrue(spec.backdropBandHeight.value <= maxOf(1f, spec.viewportHeight * 0.72f) + 0.001f)
                assertTrue(spec.contentMaxWidth.value <= density.tokens().maxContentWidth.value)
            }
        }
    }

    private fun controlsFor(gradeClass: TheaterPlateGradeClass): TheaterPlateGradeControls = when (gradeClass) {
        TheaterPlateGradeClass.MissingBackdrop -> TheaterPlateGradeControls(0.25f, 0.48f, 0.62f, 0.0f, 0.05f, 0.015f)
        TheaterPlateGradeClass.Busy -> TheaterPlateGradeControls(0.70f, 0.72f, 0.54f, 0.38f, 0.28f, 0.035f)
        TheaterPlateGradeClass.Bright -> TheaterPlateGradeControls(0.78f, 0.66f, 0.40f, 0.50f, 0.16f, 0.020f)
        TheaterPlateGradeClass.Dark -> TheaterPlateGradeControls(0.18f, 0.34f, 0.48f, 0.66f, 0.04f, 0.012f)
        TheaterPlateGradeClass.Saturated -> TheaterPlateGradeControls(0.38f, 0.54f, 0.50f, 0.58f, 0.22f, 0.018f)
        TheaterPlateGradeClass.LowDetail -> TheaterPlateGradeControls(0.30f, 0.46f, 0.62f, 0.34f, 0.08f, 0.020f)
        TheaterPlateGradeClass.Balanced -> TheaterPlateGradeControls(0.34f, 0.50f, 0.46f, 0.60f, 0.08f, 0.016f)
    }

    private fun analysisFor(
        gradeClass: TheaterPlateGradeClass,
        controls: TheaterPlateGradeControls,
    ): TheaterPlateAnalysis {
        val missing = gradeClass == TheaterPlateGradeClass.MissingBackdrop
        val stage = TheaterPlateColor.rgb(18, 20, 24)
        val context = if (missing) {
            TheaterPlateSourceContext.missingBackdrop(TheaterPlateViewport.of(1280, 720))
        } else {
            TheaterPlateSourceContext.backdrop(
                request = ImageRequestKey("00000000-0000-0000-0000-000000000459", BrowseImageCategory.Backdrop),
                token = "stage-${gradeClass.name.lowercase()}",
                viewport = TheaterPlateViewport.of(1280, 720),
            )
        }
        val grade = TheaterPlateGrade(
            gradeClass = gradeClass,
            isMissingBackdrop = missing,
            isBright = gradeClass == TheaterPlateGradeClass.Bright,
            isDark = gradeClass == TheaterPlateGradeClass.Dark,
            isBusy = gradeClass == TheaterPlateGradeClass.Busy,
            isSaturated = gradeClass == TheaterPlateGradeClass.Saturated,
            isLowDetail = gradeClass == TheaterPlateGradeClass.LowDetail,
            controls = controls,
            stageColor = stage,
        )

        return TheaterPlateAnalysis(
            context = context,
            sourceDimensions = if (missing) null else 64 to 36,
            downsample = TheaterPlateDownsample.solid(TheaterPlateColor.rgb(36, 54, 84), width = 4, height = 3),
            palette = TheaterPlatePalette(
                dominant = TheaterPlateColor.rgb(36, 54, 84),
                accent = TheaterPlateColor.rgb(103, 232, 249),
                muted = TheaterPlateColor.rgb(30, 41, 59),
                stage = stage,
            ),
            averageLuminance = 0.32f,
            medianLuminance = 0.30f,
            p95Luminance = 0.72f,
            averageSaturation = 0.40f,
            edgeDensity = if (gradeClass == TheaterPlateGradeClass.Busy) 0.32f else 0.08f,
            edgeEnergy = if (gradeClass == TheaterPlateGradeClass.Busy) 0.18f else 0.06f,
            localLuma = TheaterPlateLocalLuma(
                columns = 2,
                rows = 2,
                cells = listOf(0.22f, 0.36f, 0.30f, 0.48f),
                min = 0.22f,
                max = 0.48f,
            ),
            grade = grade,
        )
    }

    private fun assertFinitePositive(name: String, value: Dp) {
        assertFalse("$name must not be NaN", value.value.isNaN())
        assertTrue("$name finite", value.value.isFinite())
        assertTrue("$name positive", value.value > 0f)
    }
}
