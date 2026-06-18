package com.ferrex.android.ui.theaterplate

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.theaterplate.TheaterPlateAnalysis
import com.ferrex.android.core.theaterplate.TheaterPlateColor
import com.ferrex.android.core.theaterplate.TheaterPlateDownsample
import com.ferrex.android.core.theaterplate.TheaterPlateGradeClass
import com.ferrex.android.core.theaterplate.TheaterPlateViewport
import com.ferrex.android.core.theaterplate.TheaterPlateViewportClass
import com.ferrex.android.ui.theme.FerrexDesignTokens
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** Artwork availability adaptations that Theater Plate must label explicitly when contrast relies on a fallback. */
enum class TheaterPlateBackdropAdaptation(
    val explicitLabel: String?,
) {
    Ready(explicitLabel = null),
    MissingBackdrop(explicitLabel = "Missing backdrop"),
    LowQuality(explicitLabel = "Low-quality backdrop"),
    StaleOffline(explicitLabel = "Stale/offline artwork"),
    ;

    val requiresExplicitLabel: Boolean get() = explicitLabel != null

    companion object {
        fun fromAnalysis(analysis: TheaterPlateAnalysis): TheaterPlateBackdropAdaptation = if (analysis.grade.isMissingBackdrop) {
            MissingBackdrop
        } else {
            Ready
        }
    }
}

/** Shared phone, compact, and 10-foot density families for stage layout and semantic surfaces. */
enum class FerrexStageDensityFamily {
    Compact,
    Standard,
    TenFoot,
    ;

    companion object {
        fun forViewport(viewport: TheaterPlateViewport): FerrexStageDensityFamily = when (viewport.viewportClass) {
            TheaterPlateViewportClass.Compact -> Compact
            TheaterPlateViewportClass.Detail -> Standard
            TheaterPlateViewportClass.TenFoot -> TenFoot
        }
    }
}

@Immutable
data class FerrexStageDensityTokens(
    val outerPaddingHorizontal: Dp,
    val outerPaddingVertical: Dp,
    val contentGap: Dp,
    val surfaceGap: Dp,
    val maxContentWidth: Dp,
    val minInteractiveSize: Dp,
    val backdropBandMinHeight: Dp,
)

fun FerrexStageDensityFamily.tokens(): FerrexStageDensityTokens = when (this) {
    FerrexStageDensityFamily.Compact -> FerrexStageDensityTokens(
        outerPaddingHorizontal = 18.dp,
        outerPaddingVertical = 20.dp,
        contentGap = 12.dp,
        surfaceGap = 8.dp,
        maxContentWidth = 720.dp,
        minInteractiveSize = 48.dp,
        backdropBandMinHeight = 132.dp,
    )
    FerrexStageDensityFamily.Standard -> FerrexStageDensityTokens(
        outerPaddingHorizontal = FerrexDesignTokens.Space.ScreenPhoneHorizontal,
        outerPaddingVertical = FerrexDesignTokens.Space.ScreenPhoneVertical,
        contentGap = FerrexDesignTokens.Space.Lg,
        surfaceGap = FerrexDesignTokens.Space.Md,
        maxContentWidth = 880.dp,
        minInteractiveSize = 48.dp,
        backdropBandMinHeight = 180.dp,
    )
    FerrexStageDensityFamily.TenFoot -> FerrexStageDensityTokens(
        outerPaddingHorizontal = FerrexDesignTokens.Space.ScreenTvHorizontal,
        outerPaddingVertical = FerrexDesignTokens.Space.ScreenTvVertical,
        contentGap = FerrexDesignTokens.Space.Xxl,
        surfaceGap = FerrexDesignTokens.Space.Lg,
        maxContentWidth = FerrexDesignTokens.Tv.DetailMaxWidth,
        minInteractiveSize = FerrexDesignTokens.Focus.TvButtonMinHeight,
        backdropBandMinHeight = 260.dp,
    )
}

@Immutable
data class TheaterPlateStageLayoutSpec(
    val viewportWidth: Float,
    val viewportHeight: Float,
    val backdropBandHeight: Dp,
    val readabilityLobeRadius: Float,
    val vignetteRadius: Float,
    val contentStartX: Float,
    val contentBaselineY: Float,
    val contentMaxWidth: Dp,
    val horizontalPadding: Dp,
    val verticalPadding: Dp,
) {
    fun finiteFloatValues(): List<Float> = listOf(
        viewportWidth,
        viewportHeight,
        backdropBandHeight.value,
        readabilityLobeRadius,
        vignetteRadius,
        contentStartX,
        contentBaselineY,
        contentMaxWidth.value,
        horizontalPadding.value,
        verticalPadding.value,
    )

    companion object {
        fun forViewport(
            viewportWidth: Float,
            viewportHeight: Float,
            density: FerrexStageDensityFamily,
        ): TheaterPlateStageLayoutSpec {
            val tokens = density.tokens()
            val safeWidth = viewportWidth.finiteOr(1f).coerceAtLeast(1f)
            val safeHeight = viewportHeight.finiteOr(1f).coerceAtLeast(1f)
            val longEdge = max(safeWidth, safeHeight)
            val shortEdge = min(safeWidth, safeHeight)
            val bandFraction = when (density) {
                FerrexStageDensityFamily.Compact -> 0.38f
                FerrexStageDensityFamily.Standard -> 0.42f
                FerrexStageDensityFamily.TenFoot -> 0.48f
            }
            val maxBandHeight = (safeHeight * 0.72f).coerceAtLeast(1f)
            val minBandHeight = min(tokens.backdropBandMinHeight.value, maxBandHeight)
            val bandHeight = (safeHeight * bandFraction)
                .coerceIn(minBandHeight, maxBandHeight)
                .finiteOr(minBandHeight)
            val lobeRadius = (longEdge * when (density) {
                FerrexStageDensityFamily.Compact -> 0.48f
                FerrexStageDensityFamily.Standard -> 0.54f
                FerrexStageDensityFamily.TenFoot -> 0.60f
            }).finiteOr(longEdge)
            val contentWidth = min(tokens.maxContentWidth.value, (safeWidth - tokens.outerPaddingHorizontal.value * 2f).coerceAtLeast(1f))
            val contentStart = ((safeWidth - contentWidth) / 2f).coerceAtLeast(0f)

            return TheaterPlateStageLayoutSpec(
                viewportWidth = safeWidth,
                viewportHeight = safeHeight,
                backdropBandHeight = bandHeight.dp,
                readabilityLobeRadius = lobeRadius,
                vignetteRadius = (longEdge * 0.86f).finiteOr(longEdge),
                contentStartX = contentStart,
                contentBaselineY = (shortEdge * 0.58f).finiteOr(tokens.outerPaddingVertical.value),
                contentMaxWidth = contentWidth.dp,
                horizontalPadding = tokens.outerPaddingHorizontal.ensureFinite(24.dp),
                verticalPadding = tokens.outerPaddingVertical.ensureFinite(32.dp),
            )
        }
    }
}

/** Stable visual controls consumed by Compose drawing. They are derived from CPU analysis, never from drawing-time bitmap reads. */
@Immutable
data class TheaterPlateStageVisuals(
    val baseColor: Color,
    val ambientColors: List<Color>,
    val accentColor: Color,
    val scrimOpacity: Float,
    val ambientOpacity: Float,
    val backdropOpacity: Float,
    val plateOpacity: Float,
    val desaturation: Float,
    val readabilityLobeOpacity: Float,
    val vignetteOpacity: Float,
    val grainOpacity: Float,
    val highlightCompression: Float,
    val backdropBandFraction: Float,
    val adaptation: TheaterPlateBackdropAdaptation,
    val explicitStateLabel: String?,
    val grainSeed: Int,
) {
    fun finiteFloatValues(): List<Float> = listOf(
        scrimOpacity,
        ambientOpacity,
        backdropOpacity,
        plateOpacity,
        desaturation,
        readabilityLobeOpacity,
        vignetteOpacity,
        grainOpacity,
        highlightCompression,
        backdropBandFraction,
    )

    companion object {
        fun fromAnalysis(
            analysis: TheaterPlateAnalysis,
            adaptation: TheaterPlateBackdropAdaptation = TheaterPlateBackdropAdaptation.fromAnalysis(analysis),
        ): TheaterPlateStageVisuals {
            val controls = analysis.grade.controls
            val stage = analysis.grade.stageColor
            val stageColor = stage.toComposeColor()
            val ambientOpacity = controls.ambientOpacity.controlOrZero()
            val scrim = controls.scrimOpacity.controlOrZero()
            val plate = controls.plateOpacity.controlOrZero()
            val grain = controls.grainOpacity.controlOrZero()
            val adjustedBackdropOpacity = when (adaptation) {
                TheaterPlateBackdropAdaptation.Ready -> plate.coerceIn(0.18f, 0.72f)
                TheaterPlateBackdropAdaptation.MissingBackdrop -> 0f
                TheaterPlateBackdropAdaptation.LowQuality -> (plate * 0.52f).coerceIn(0f, 0.36f)
                TheaterPlateBackdropAdaptation.StaleOffline -> (plate * 0.42f).coerceIn(0f, 0.30f)
            }
            val adjustedScrim = when (adaptation) {
                TheaterPlateBackdropAdaptation.Ready -> scrim
                TheaterPlateBackdropAdaptation.MissingBackdrop -> max(scrim, 0.52f)
                TheaterPlateBackdropAdaptation.LowQuality -> (scrim + 0.08f).controlOrZero()
                TheaterPlateBackdropAdaptation.StaleOffline -> max(scrim, 0.60f)
            }
            val adjustedAmbient = when (adaptation) {
                TheaterPlateBackdropAdaptation.Ready -> ambientOpacity
                TheaterPlateBackdropAdaptation.MissingBackdrop -> max(ambientOpacity, 0.62f)
                TheaterPlateBackdropAdaptation.LowQuality -> max(ambientOpacity, 0.54f)
                TheaterPlateBackdropAdaptation.StaleOffline -> max(ambientOpacity, 0.56f)
            }
            val adjustedGrain = when (adaptation) {
                TheaterPlateBackdropAdaptation.LowQuality -> max(grain, 0.030f)
                TheaterPlateBackdropAdaptation.StaleOffline -> max(grain, 0.022f)
                else -> grain
            }
            val bandFraction = when (analysis.grade.gradeClass) {
                TheaterPlateGradeClass.Bright -> 0.54f
                TheaterPlateGradeClass.Dark -> 0.46f
                TheaterPlateGradeClass.Busy -> 0.50f
                TheaterPlateGradeClass.Saturated -> 0.50f
                TheaterPlateGradeClass.LowDetail -> 0.42f
                TheaterPlateGradeClass.MissingBackdrop -> 0.36f
                TheaterPlateGradeClass.Balanced -> 0.48f
            }

            return TheaterPlateStageVisuals(
                baseColor = stageColor,
                ambientColors = analysis.downsample.toAmbientColors(stage, analysis.palette.accent, adjustedAmbient),
                accentColor = analysis.palette.accent.toComposeColor(),
                scrimOpacity = adjustedScrim.controlOrZero(),
                ambientOpacity = adjustedAmbient.controlOrZero(),
                backdropOpacity = adjustedBackdropOpacity.controlOrZero(),
                plateOpacity = plate,
                desaturation = when (adaptation) {
                    TheaterPlateBackdropAdaptation.StaleOffline -> max(controls.desaturation, 0.20f)
                    TheaterPlateBackdropAdaptation.LowQuality -> max(controls.desaturation, 0.16f)
                    else -> controls.desaturation
                }.controlOrZero(),
                readabilityLobeOpacity = (adjustedScrim * 0.72f + 0.18f).controlOrZero(),
                vignetteOpacity = (0.32f + controls.highlightCompression.controlOrZero() * 0.30f).controlOrZero(),
                grainOpacity = adjustedGrain.controlOrZero(),
                highlightCompression = controls.highlightCompression.controlOrZero(),
                backdropBandFraction = bandFraction.controlOrZero(),
                adaptation = adaptation,
                explicitStateLabel = adaptation.explicitLabel,
                grainSeed = stableGrainSeed(analysis),
            )
        }
    }
}

enum class FerrexStageSurfaceVariant(val semanticName: String) {
    ProjectionShelf("projection shelf"),
    ControlShelf("control shelf"),
    RailBand("rail band"),
    FactRibbon("fact ribbon"),
    NoticeSlab("notice slab"),
    EmptyState("empty state"),
    StatusSlab("status slab"),
}

enum class FerrexStageSurfaceTone {
    Neutral,
    Primary,
    Cache,
    StaleOffline,
    Warning,
    Error,
}

@Immutable
data class FerrexStageSurfaceTokenSpec(
    val variant: FerrexStageSurfaceVariant,
    val density: FerrexStageDensityFamily,
    val horizontalPadding: Dp,
    val verticalPadding: Dp,
    val cornerRadius: Dp,
    val minHeight: Dp,
    val borderWidth: Dp,
    val containerAlpha: Float,
    val borderAlpha: Float,
    val denseBand: Boolean,
    val semanticName: String,
)

fun FerrexStageSurfaceVariant.defaultTone(): FerrexStageSurfaceTone = when (this) {
    FerrexStageSurfaceVariant.ProjectionShelf -> FerrexStageSurfaceTone.Neutral
    FerrexStageSurfaceVariant.ControlShelf -> FerrexStageSurfaceTone.Primary
    FerrexStageSurfaceVariant.RailBand -> FerrexStageSurfaceTone.Neutral
    FerrexStageSurfaceVariant.FactRibbon -> FerrexStageSurfaceTone.Cache
    FerrexStageSurfaceVariant.NoticeSlab -> FerrexStageSurfaceTone.Warning
    FerrexStageSurfaceVariant.EmptyState -> FerrexStageSurfaceTone.StaleOffline
    FerrexStageSurfaceVariant.StatusSlab -> FerrexStageSurfaceTone.StaleOffline
}

fun FerrexStageSurfaceVariant.tokenSpec(density: FerrexStageDensityFamily): FerrexStageSurfaceTokenSpec {
    val compact = density == FerrexStageDensityFamily.Compact
    val tenFoot = density == FerrexStageDensityFamily.TenFoot
    val densityTokens = density.tokens()
    val horizontal = when (this) {
        FerrexStageSurfaceVariant.FactRibbon -> if (tenFoot) 18.dp else if (compact) 10.dp else 14.dp
        FerrexStageSurfaceVariant.RailBand -> if (tenFoot) 22.dp else if (compact) 12.dp else 16.dp
        FerrexStageSurfaceVariant.ControlShelf -> if (tenFoot) 24.dp else if (compact) 14.dp else 18.dp
        else -> if (tenFoot) 28.dp else if (compact) 16.dp else 20.dp
    }
    val vertical = when (this) {
        FerrexStageSurfaceVariant.FactRibbon -> if (tenFoot) 10.dp else if (compact) 6.dp else 8.dp
        FerrexStageSurfaceVariant.RailBand -> if (tenFoot) 12.dp else if (compact) 8.dp else 10.dp
        FerrexStageSurfaceVariant.ControlShelf -> if (tenFoot) 14.dp else if (compact) 10.dp else 12.dp
        else -> if (tenFoot) 18.dp else if (compact) 12.dp else 14.dp
    }
    val minHeight = when (this) {
        FerrexStageSurfaceVariant.ProjectionShelf -> densityTokens.minInteractiveSize + if (tenFoot) 24.dp else 12.dp
        FerrexStageSurfaceVariant.ControlShelf -> densityTokens.minInteractiveSize
        FerrexStageSurfaceVariant.RailBand -> if (tenFoot) 56.dp else 44.dp
        FerrexStageSurfaceVariant.FactRibbon -> if (tenFoot) 46.dp else if (compact) 34.dp else 40.dp
        FerrexStageSurfaceVariant.NoticeSlab -> if (tenFoot) 76.dp else if (compact) 52.dp else 64.dp
        FerrexStageSurfaceVariant.EmptyState -> if (tenFoot) 126.dp else if (compact) 80.dp else 104.dp
        FerrexStageSurfaceVariant.StatusSlab -> if (tenFoot) 64.dp else if (compact) 44.dp else 52.dp
    }

    return FerrexStageSurfaceTokenSpec(
        variant = this,
        density = density,
        horizontalPadding = horizontal,
        verticalPadding = vertical,
        cornerRadius = when (this) {
            FerrexStageSurfaceVariant.ProjectionShelf -> 14.dp
            FerrexStageSurfaceVariant.ControlShelf -> 12.dp
            FerrexStageSurfaceVariant.RailBand -> 10.dp
            FerrexStageSurfaceVariant.FactRibbon -> 8.dp
            FerrexStageSurfaceVariant.NoticeSlab -> 12.dp
            FerrexStageSurfaceVariant.EmptyState -> 14.dp
            FerrexStageSurfaceVariant.StatusSlab -> 10.dp
        },
        minHeight = minHeight,
        borderWidth = if (tenFoot) FerrexDesignTokens.Focus.TvRestingBorder else 1.dp,
        containerAlpha = when (this) {
            FerrexStageSurfaceVariant.ProjectionShelf -> 0.58f
            FerrexStageSurfaceVariant.ControlShelf -> 0.48f
            FerrexStageSurfaceVariant.RailBand -> 0.34f
            FerrexStageSurfaceVariant.FactRibbon -> 0.26f
            FerrexStageSurfaceVariant.NoticeSlab -> 0.52f
            FerrexStageSurfaceVariant.EmptyState -> 0.42f
            FerrexStageSurfaceVariant.StatusSlab -> 0.46f
        },
        borderAlpha = when (this) {
            FerrexStageSurfaceVariant.ProjectionShelf -> 0.34f
            FerrexStageSurfaceVariant.ControlShelf -> 0.54f
            FerrexStageSurfaceVariant.RailBand -> 0.44f
            FerrexStageSurfaceVariant.FactRibbon -> 0.38f
            FerrexStageSurfaceVariant.NoticeSlab -> 0.62f
            FerrexStageSurfaceVariant.EmptyState -> 0.50f
            FerrexStageSurfaceVariant.StatusSlab -> 0.52f
        },
        denseBand = this in denseBandVariants,
        semanticName = semanticName,
    )
}

@Composable
fun TheaterPlateStage(
    analysis: TheaterPlateAnalysis,
    modifier: Modifier = Modifier,
    adaptation: TheaterPlateBackdropAdaptation = TheaterPlateBackdropAdaptation.fromAnalysis(analysis),
    density: FerrexStageDensityFamily = FerrexStageDensityFamily.forViewport(analysis.context.viewport),
    contentDescription: String? = null,
    contentMaxWidth: Dp? = null,
    showStateLabel: Boolean = true,
    backdrop: (@Composable BoxScope.() -> Unit)? = null,
    content: @Composable BoxScope.() -> Unit,
) {
    BoxWithConstraints(
        modifier = modifier
            .fillMaxSize()
            .then(stageContentDescription(contentDescription)),
    ) {
        val layoutSpec = remember(maxWidth, maxHeight, density) {
            TheaterPlateStageLayoutSpec.forViewport(maxWidth.value, maxHeight.value, density)
        }
        TheaterPlateBackground(
            analysis = analysis,
            modifier = Modifier.matchParentSize(),
            adaptation = adaptation,
            density = density,
            layoutSpec = layoutSpec,
            showStateLabel = showStateLabel,
            backdrop = backdrop,
        )
        Box(
            modifier = Modifier
                .align(Alignment.TopCenter)
                .padding(
                    horizontal = layoutSpec.horizontalPadding,
                    vertical = layoutSpec.verticalPadding,
                )
                .widthIn(max = contentMaxWidth ?: layoutSpec.contentMaxWidth),
            content = content,
        )
    }
}

@Composable
fun TheaterPlateBackground(
    analysis: TheaterPlateAnalysis,
    modifier: Modifier = Modifier,
    adaptation: TheaterPlateBackdropAdaptation = TheaterPlateBackdropAdaptation.fromAnalysis(analysis),
    density: FerrexStageDensityFamily = FerrexStageDensityFamily.forViewport(analysis.context.viewport),
    layoutSpec: TheaterPlateStageLayoutSpec? = null,
    showStateLabel: Boolean = true,
    backdrop: (@Composable BoxScope.() -> Unit)? = null,
) {
    val visuals = remember(analysis, adaptation) { TheaterPlateStageVisuals.fromAnalysis(analysis, adaptation) }
    val baseModifier = modifier
        .clipToBounds()
        .background(visuals.baseColor)

    if (layoutSpec == null) {
        BoxWithConstraints(modifier = baseModifier) {
            val resolvedLayoutSpec = remember(maxWidth, maxHeight, density) {
                TheaterPlateStageLayoutSpec.forViewport(maxWidth.value, maxHeight.value, density)
            }
            TheaterPlateBackgroundLayers(
                visuals = visuals,
                layoutSpec = resolvedLayoutSpec,
                showStateLabel = showStateLabel,
                backdrop = backdrop,
            )
        }
    } else {
        Box(modifier = baseModifier) {
            TheaterPlateBackgroundLayers(
                visuals = visuals,
                layoutSpec = layoutSpec,
                showStateLabel = showStateLabel,
                backdrop = backdrop,
            )
        }
    }
}

@Composable
fun FerrexStageSurface(
    variant: FerrexStageSurfaceVariant,
    modifier: Modifier = Modifier,
    density: FerrexStageDensityFamily = FerrexStageDensityFamily.Standard,
    tone: FerrexStageSurfaceTone = variant.defaultTone(),
    enabled: Boolean = true,
    onClick: (() -> Unit)? = null,
    contentDescription: String? = null,
    testTag: String? = null,
    content: @Composable BoxScope.() -> Unit,
) {
    val tokenSpec = remember(variant, density) { variant.tokenSpec(density) }
    val colors = tone.colors(tokenSpec)
    val clickableModifier = if (onClick != null) {
        Modifier.clickable(
            enabled = enabled,
            role = Role.Button,
            onClick = onClick,
        )
    } else {
        Modifier
    }

    Surface(
        modifier = modifier
            .defaultMinSize(minHeight = tokenSpec.minHeight)
            .then(testTagModifier(testTag))
            .then(clickableModifier)
            .then(stageSurfaceSemantics(contentDescription, onClick != null)),
        shape = RoundedCornerShape(tokenSpec.cornerRadius),
        color = colors.container,
        contentColor = colors.content,
        border = BorderStroke(tokenSpec.borderWidth, colors.border),
        tonalElevation = 0.dp,
        shadowElevation = 0.dp,
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .padding(
                    horizontal = tokenSpec.horizontalPadding,
                    vertical = tokenSpec.verticalPadding,
                ),
            content = content,
        )
    }
}

@Composable
private fun BoxScope.TheaterPlateBackgroundLayers(
    visuals: TheaterPlateStageVisuals,
    layoutSpec: TheaterPlateStageLayoutSpec,
    showStateLabel: Boolean,
    backdrop: (@Composable BoxScope.() -> Unit)?,
) {
    Canvas(modifier = Modifier.matchParentSize()) {
        drawTheaterPlateBase(visuals)
    }
    if (backdrop != null && visuals.backdropOpacity > 0f) {
        Box(
            modifier = Modifier
                .align(Alignment.TopCenter)
                .fillMaxWidth()
                .height(layoutSpec.backdropBandHeight)
                .alpha(visuals.backdropOpacity)
                .clipToBounds()
                .semantics { contentDescription = "Theater Plate backdrop band" },
        ) {
            backdrop()
        }
    }
    Canvas(modifier = Modifier.matchParentSize()) {
        drawTheaterPlateOverlay(visuals)
        drawTheaterPlateGrain(visuals)
    }
    if (showStateLabel) {
        visuals.explicitStateLabel?.let { label ->
            TheaterPlateStateLabel(
                label = label,
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(FerrexDesignTokens.Space.Md),
            )
        }
    }
}

@Composable
private fun TheaterPlateStateLabel(
    label: String,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.semantics { contentDescription = label },
        shape = RoundedCornerShape(8.dp),
        color = FerrexDesignTokens.Palette.SlateBlack.copy(alpha = 0.78f),
        contentColor = FerrexDesignTokens.Palette.TextPrimary,
        border = BorderStroke(1.dp, FerrexDesignTokens.Palette.TextMuted.copy(alpha = 0.62f)),
        tonalElevation = 0.dp,
        shadowElevation = 0.dp,
    ) {
        Text(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
            text = label,
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.SemiBold,
            color = FerrexDesignTokens.Palette.TextPrimary,
        )
    }
}

@Immutable
private data class StageSurfaceColors(
    val container: Color,
    val content: Color,
    val border: Color,
)

@Composable
private fun FerrexStageSurfaceTone.colors(tokenSpec: FerrexStageSurfaceTokenSpec): StageSurfaceColors {
    val palette = FerrexDesignTokens.Palette
    val base = when (this) {
        FerrexStageSurfaceTone.Neutral -> palette.SlatePanel
        FerrexStageSurfaceTone.Primary -> palette.SignalCyanDim
        FerrexStageSurfaceTone.Cache -> palette.PrivateVioletDim
        FerrexStageSurfaceTone.StaleOffline -> palette.SlateElevated
        FerrexStageSurfaceTone.Warning -> palette.Warning
        FerrexStageSurfaceTone.Error -> palette.ErrorDim
    }
    val content = when (this) {
        FerrexStageSurfaceTone.StaleOffline -> palette.TextSecondary
        else -> palette.TextPrimary
    }
    val border = when (this) {
        FerrexStageSurfaceTone.Neutral -> palette.SlateLine
        FerrexStageSurfaceTone.Primary -> palette.SignalCyan
        FerrexStageSurfaceTone.Cache -> palette.PrivateViolet
        FerrexStageSurfaceTone.StaleOffline -> palette.TextMuted
        FerrexStageSurfaceTone.Warning -> palette.Warning
        FerrexStageSurfaceTone.Error -> palette.Error
    }

    return StageSurfaceColors(
        container = base.copy(alpha = tokenSpec.containerAlpha.controlOrZero()),
        content = content,
        border = border.copy(alpha = tokenSpec.borderAlpha.controlOrZero()),
    )
}

private val denseBandVariants = setOf(
    FerrexStageSurfaceVariant.ControlShelf,
    FerrexStageSurfaceVariant.RailBand,
    FerrexStageSurfaceVariant.FactRibbon,
    FerrexStageSurfaceVariant.StatusSlab,
)

private fun DrawScope.drawTheaterPlateBase(visuals: TheaterPlateStageVisuals) {
    if (!size.isFiniteAndPositive()) return
    drawRect(color = visuals.baseColor)
    val ambientColors = visuals.ambientColors.ifEmpty { listOf(visuals.baseColor, visuals.accentColor) }
    drawRect(
        brush = Brush.linearGradient(
            colors = ambientColors,
            start = Offset.Zero,
            end = Offset(size.width, size.height * 0.82f),
        ),
        alpha = visuals.ambientOpacity,
    )
    drawRect(
        brush = Brush.verticalGradient(
            colors = listOf(
                visuals.accentColor.copy(alpha = 0.18f),
                visuals.baseColor.copy(alpha = 0.08f),
                Color.Transparent,
            ),
            startY = 0f,
            endY = size.height * visuals.backdropBandFraction.coerceIn(0.24f, 0.72f),
        ),
        size = Size(size.width, size.height * visuals.backdropBandFraction.coerceIn(0.24f, 0.72f)),
    )
    drawCircle(
        brush = Brush.radialGradient(
            colors = listOf(
                Color.Black.copy(alpha = visuals.readabilityLobeOpacity * 0.48f),
                Color.Transparent,
            ),
            center = Offset(size.width * 0.18f, size.height * 0.66f),
            radius = max(size.width, size.height) * 0.54f,
        ),
        radius = max(size.width, size.height) * 0.54f,
        center = Offset(size.width * 0.18f, size.height * 0.66f),
    )
    drawCircle(
        brush = Brush.radialGradient(
            colors = listOf(
                visuals.baseColor.copy(alpha = visuals.readabilityLobeOpacity * 0.36f),
                Color.Transparent,
            ),
            center = Offset(size.width * 0.84f, size.height * 0.82f),
            radius = max(size.width, size.height) * 0.42f,
        ),
        radius = max(size.width, size.height) * 0.42f,
        center = Offset(size.width * 0.84f, size.height * 0.82f),
    )
}

private fun DrawScope.drawTheaterPlateOverlay(visuals: TheaterPlateStageVisuals) {
    if (!size.isFiniteAndPositive()) return
    drawRect(color = Color.Black.copy(alpha = visuals.scrimOpacity * 0.18f))
    drawRect(
        brush = Brush.verticalGradient(
            colors = listOf(
                Color.Transparent,
                visuals.baseColor.copy(alpha = visuals.scrimOpacity * 0.72f),
                Color.Black.copy(alpha = visuals.scrimOpacity * 0.54f),
            ),
            startY = size.height * 0.20f,
            endY = size.height,
        ),
    )
    drawRect(
        brush = Brush.horizontalGradient(
            colors = listOf(
                Color.Black.copy(alpha = visuals.scrimOpacity * 0.52f),
                Color.Transparent,
                Color.Black.copy(alpha = visuals.scrimOpacity * 0.34f),
            ),
            startX = 0f,
            endX = size.width,
        ),
    )
    drawRect(
        brush = Brush.radialGradient(
            colors = listOf(
                Color.Transparent,
                Color.Black.copy(alpha = visuals.vignetteOpacity),
            ),
            center = Offset(size.width * 0.52f, size.height * 0.44f),
            radius = max(size.width, size.height) * 0.86f,
        ),
    )
}

private fun DrawScope.drawTheaterPlateGrain(visuals: TheaterPlateStageVisuals) {
    if (!size.isFiniteAndPositive() || visuals.grainOpacity <= 0f) return
    val step = (min(size.width, size.height) / 36f).coerceIn(5f, 18f)
    val columns = ceil(size.width / step).roundToInt().coerceIn(1, 96)
    val rows = ceil(size.height / step).roundToInt().coerceIn(1, 64)
    val dotSize = (step * 0.26f).coerceAtLeast(1f)
    for (row in 0 until rows) {
        for (column in 0 until columns) {
            val noise = deterministicNoise(column, row, visuals.grainSeed)
            val bucket = noise and 0x07
            if (bucket > 4) continue
            val alpha = visuals.grainOpacity * (0.24f + bucket * 0.06f)
            val color = if ((noise and 0x08) == 0) Color.White else Color.Black
            drawRect(
                color = color.copy(alpha = alpha.controlOrZero()),
                topLeft = Offset(column * step, row * step),
                size = Size(dotSize, dotSize),
            )
        }
    }
}

private fun TheaterPlateDownsample.toAmbientColors(
    stage: TheaterPlateColor,
    accent: TheaterPlateColor,
    opacity: Float,
): List<Color> {
    val fallback = listOf(stage, stage.mix(accent, 0.28f), stage)
    val selected = if (pixels.isEmpty()) {
        fallback
    } else {
        listOf(
            sampleAt(0.12f, 0.18f) ?: fallback[0],
            sampleAt(0.55f, 0.30f) ?: fallback[1],
            sampleAt(0.86f, 0.42f) ?: fallback[2],
            sampleAt(0.42f, 0.78f) ?: fallback[0],
        )
    }
    return selected.mapIndexed { index, color ->
        val mixed = when (index) {
            0 -> color.mix(stage, 0.48f)
            1 -> color.mix(accent, 0.22f).mix(stage, 0.38f)
            2 -> color.mix(stage, 0.58f)
            else -> color.mix(stage, 0.68f)
        }
        mixed.toComposeColor(alpha = opacity.coerceIn(0.22f, 0.72f))
    }
}

private fun TheaterPlateDownsample.sampleAt(xFraction: Float, yFraction: Float): TheaterPlateColor? {
    if (width <= 0 || height <= 0 || pixels.isEmpty()) return null
    val x = ((width - 1) * xFraction.coerceIn(0f, 1f)).roundToInt().coerceIn(0, width - 1)
    val y = ((height - 1) * yFraction.coerceIn(0f, 1f)).roundToInt().coerceIn(0, height - 1)
    return pixels.getOrNull(y * width + x)
}

private fun TheaterPlateColor.toComposeColor(alpha: Float = 1f): Color = Color(
    red = r / 255f,
    green = g / 255f,
    blue = b / 255f,
    alpha = alpha.controlOrZero().takeIf { it > 0f } ?: 0f,
)

private fun stableGrainSeed(analysis: TheaterPlateAnalysis): Int {
    val token = analysis.context.source.token
        ?: analysis.context.source.request?.cacheKey
        ?: analysis.palette.dominant.toHex()
    return token.fold(0x2f6e2b1d) { acc, char -> (acc * 31) xor char.code }
}

private fun deterministicNoise(x: Int, y: Int, seed: Int): Int {
    var value = seed xor (x * 0x045d9f3b) xor (y * 0x119de1f3)
    value = value xor (value ushr 16)
    value *= 0x045d9f3b
    value = value xor (value ushr 16)
    return value
}

private fun Float.controlOrZero(): Float = if (isFinite()) coerceIn(0f, 1f) else 0f

private fun Float.finiteOr(default: Float): Float = if (isFinite()) this else default

private fun Dp.ensureFinite(default: Dp): Dp = if (value.isFinite()) this else default

private fun Size.isFiniteAndPositive(): Boolean = width.isFinite() && height.isFinite() && width > 0f && height > 0f

private fun stageContentDescription(description: String?): Modifier = if (description == null) {
    Modifier
} else {
    Modifier.semantics { contentDescription = description }
}

private fun stageSurfaceSemantics(description: String?, clickable: Boolean): Modifier = if (description == null && !clickable) {
    Modifier
} else {
    Modifier.semantics(mergeDescendants = false) {
        if (description != null) contentDescription = description
        if (clickable) role = Role.Button
    }
}

private fun testTagModifier(tag: String?): Modifier = if (tag == null) Modifier else Modifier.testTag(tag)
