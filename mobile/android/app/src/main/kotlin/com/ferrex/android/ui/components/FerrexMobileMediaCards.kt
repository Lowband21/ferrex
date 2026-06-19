package com.ferrex.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil.ImageLoader
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.mediaart.MediaArtFallback
import com.ferrex.android.core.mediaart.MediaArtFallbackPolicy
import com.ferrex.android.core.mediaart.MediaArtObject
import com.ferrex.android.core.mediaart.MediaArtVisualState
import com.ferrex.android.core.mediaart.MediaRailIdentityResolver
import com.ferrex.android.core.mediaart.MediaRailItemIdentity
import com.ferrex.android.core.mediaart.runtimeFallback
import com.ferrex.android.ui.theaterplate.FerrexStageDensityFamily
import com.ferrex.android.ui.theaterplate.FerrexStageSurface
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceTone
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceVariant
import com.ferrex.android.ui.theaterplate.tokens
import com.ferrex.android.ui.theme.FerrexDesignTokens

/** Touch-native card layouts for phone Theater Plate media surfaces. */
enum class MobileMediaCardLayout {
    Hero,
    Rail,
    CompactRail,
    Grid,
    DenseGrid,
    ;

    val horizontal: Boolean get() = this == Hero

    val titleMaxLines: Int
        get() = when (this) {
            Hero -> 2
            Rail,
            Grid -> 2
            CompactRail,
            DenseGrid -> 1
        }

    val subtitleMaxLines: Int
        get() = when (this) {
            Hero -> 2
            Rail,
            Grid -> 2
            CompactRail,
            DenseGrid -> 1
        }

    val metadataMaxLines: Int
        get() = when (this) {
            DenseGrid -> 1
            else -> 2
        }

    val visibleBadgeLimit: Int
        get() = when (this) {
            DenseGrid -> 2
            else -> 5
        }

    val showsActionBadge: Boolean get() = this != DenseGrid

    val copyGap: Dp
        get() = when (this) {
            DenseGrid -> FerrexDesignTokens.Space.Xxs
            else -> FerrexDesignTokens.Space.Xs
        }
}

enum class MobileMediaWatchState(val label: String) {
    Unknown("Watch state unavailable"),
    Unwatched("Unwatched"),
    InProgress("In progress"),
    Watched("Watched"),
}

@Immutable
data class MobileMediaCardState(
    val progressFraction: Float? = null,
    val progressLabel: String? = null,
    val watchState: MobileMediaWatchState? = null,
    val artworkLabels: List<String> = emptyList(),
    val actionLabel: String? = null,
    val actionRole: FerrexActionRole = FerrexActionRole.Primary,
    val enabled: Boolean = true,
) {
    init {
        require(progressFraction == null || progressFraction in 0f..1f) { "Progress must be normalized" }
    }

    val visibleBadges: List<String> = buildList {
        watchState?.takeIf { it != MobileMediaWatchState.Unknown }?.let { add(it.label) }
        progressLabel?.takeIf { it.isNotBlank() }?.let { add(it) }
        artworkLabels.filter { it.isNotBlank() }.forEach(::add)
        actionLabel?.takeIf { it.isNotBlank() }?.let { add("Action: $it") }
        if (!enabled) add("Unavailable")
    }.distinct()

    fun contentDescription(
        title: String,
        subtitle: String?,
        metadata: String?,
    ): String = buildList {
        add(title)
        subtitle?.takeIf { it.isNotBlank() }?.let(::add)
        metadata?.takeIf { it.isNotBlank() }?.let(::add)
        visibleBadges.forEach(::add)
    }.joinToString(". ")
}

object MobileMediaCardPresenter {
    fun state(
        art: MediaArtObject,
        resolution: ImageResolution?,
        fallback: MediaArtFallback?,
        progressFraction: Float? = null,
        progressLabel: String? = null,
        watchState: MobileMediaWatchState? = inferredWatchState(progressFraction),
        actionLabel: String? = null,
        actionRole: FerrexActionRole = FerrexActionRole.Primary,
        enabled: Boolean = true,
    ): MobileMediaCardState = MobileMediaCardState(
        progressFraction = progressFraction?.coerceIn(0f, 1f),
        progressLabel = progressLabel,
        watchState = watchState,
        artworkLabels = artworkLabels(art, resolution, fallback),
        actionLabel = actionLabel,
        actionRole = actionRole,
        enabled = enabled,
    )

    fun artworkLabels(
        art: MediaArtObject,
        resolution: ImageResolution?,
        fallback: MediaArtFallback?,
    ): List<String> = MediaArtVisualState.from(art, resolution, fallback).screenshotLabels

    fun inferredWatchState(progressFraction: Float?): MobileMediaWatchState? = when {
        progressFraction == null -> null
        progressFraction >= 0.95f -> MobileMediaWatchState.Watched
        progressFraction > 0f -> MobileMediaWatchState.InProgress
        else -> MobileMediaWatchState.Unwatched
    }
}

@Composable
fun FerrexMobileMediaCard(
    title: String,
    subtitle: String?,
    metadata: String?,
    art: MediaArtObject,
    resolution: ImageResolution?,
    imageLoader: ImageLoader?,
    serverUrl: String,
    modifier: Modifier = Modifier,
    density: FerrexStageDensityFamily = FerrexStageDensityFamily.Standard,
    layout: MobileMediaCardLayout = MobileMediaCardLayout.Rail,
    state: MobileMediaCardState? = null,
    fallbackPolicy: MediaArtFallbackPolicy = MediaArtFallbackPolicy(),
    testTag: String? = null,
    contentDescription: String? = null,
    onClick: (() -> Unit)? = null,
) {
    val fallback = remember(art, resolution, serverUrl, fallbackPolicy) {
        if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
            art.runtimeFallback(serverUrl, fallbackPolicy)
        } else {
            null
        }
    }
    val baseState = state ?: MobileMediaCardPresenter.state(
        art = art,
        resolution = resolution,
        fallback = fallback,
        actionLabel = if (onClick != null) "Open" else null,
    )
    val cardState = if (baseState.artworkLabels.isEmpty()) {
        baseState.copy(artworkLabels = MobileMediaCardPresenter.artworkLabels(art, resolution, fallback))
    } else {
        baseState
    }
    val description = contentDescription?.let { base ->
        (listOf(base) + cardState.visibleBadges).filter { it.isNotBlank() }.distinct().joinToString(". ")
    } ?: cardState.contentDescription(title, subtitle, metadata)
    val tone = cardState.surfaceTone()

    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ProjectionShelf,
        density = density,
        tone = tone,
        enabled = cardState.enabled,
        modifier = modifier,
        onClick = onClick,
        contentDescription = description,
        testTag = testTag,
    ) {
        if (layout.horizontal) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                MobileMediaArtwork(
                    art = art,
                    resolution = resolution,
                    imageLoader = imageLoader,
                    fallback = fallback,
                    contentDescription = description,
                    modifier = Modifier.width(layout.heroArtWidth(density)),
                )
                MobileMediaCardCopy(
                    title = title,
                    subtitle = subtitle,
                    metadata = metadata,
                    state = cardState,
                    density = density,
                    layout = layout,
                    modifier = Modifier.weight(1f),
                    onClick = onClick,
                )
            }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
                MobileMediaArtwork(
                    art = art,
                    resolution = resolution,
                    imageLoader = imageLoader,
                    fallback = fallback,
                    contentDescription = description,
                )
                MobileMediaCardCopy(
                    title = title,
                    subtitle = subtitle,
                    metadata = metadata,
                    state = cardState,
                    density = density,
                    layout = layout,
                    onClick = onClick,
                )
            }
        }
    }
}

@Composable
fun <T> FerrexMobileMediaRail(
    railKey: String,
    title: String,
    items: List<T>,
    itemStableId: (T) -> String,
    density: FerrexStageDensityFamily,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    testTag: String? = null,
    contentDescription: String? = null,
    itemContent: @Composable (item: T, identity: MediaRailItemIdentity) -> Unit,
) {
    val identifiedItems = remember(railKey, items) { items.withRailIdentities(railKey, itemStableId) }
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.RailBand,
        density = density,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = modifier.fillMaxWidth(),
        testTag = testTag,
        contentDescription = contentDescription ?: "$title rail. ${items.size} item${if (items.size == 1) "" else "s"}.",
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
            TheaterPlateText(
                text = title,
                role = TheaterPlateTypographyRole.SectionTitle,
                densityRole = density.toMobileTypographyDensity(),
                maxLines = 2,
            )
            subtitle?.takeIf { it.isNotBlank() }?.let {
                TheaterPlateText(
                    text = it,
                    role = TheaterPlateTypographyRole.RailSubtitle,
                    densityRole = density.toMobileTypographyDensity(),
                    maxLines = 3,
                )
            }
            LazyRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
                contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Xs),
            ) {
                items(identifiedItems, key = { it.identity.renderKey }) { identified ->
                    itemContent(identified.item, identified.identity)
                }
            }
        }
    }
}

@Composable
fun <T> FerrexMobileMediaGrid(
    gridKey: String,
    items: List<T>,
    itemStableId: (T) -> String,
    columns: GridCells,
    modifier: Modifier = Modifier,
    testTag: String? = null,
    contentDescription: String? = null,
    contentPadding: PaddingValues = PaddingValues(FerrexDesignTokens.Space.None),
    horizontalArrangement: Arrangement.Horizontal = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
    verticalArrangement: Arrangement.Vertical = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
    itemContent: @Composable (item: T, identity: MediaRailItemIdentity) -> Unit,
) {
    val identifiedItems = remember(gridKey, items) { items.withRailIdentities(gridKey, itemStableId) }
    LazyVerticalGrid(
        columns = columns,
        modifier = modifier
            .fillMaxWidth()
            .then(if (testTag == null) Modifier else Modifier.testTag(testTag))
            .then(
                Modifier.semantics {
                    this.contentDescription = contentDescription
                        ?: "Media grid. ${items.size} item${if (items.size == 1) "" else "s"}."
                },
            ),
        contentPadding = contentPadding,
        horizontalArrangement = horizontalArrangement,
        verticalArrangement = verticalArrangement,
    ) {
        items(identifiedItems, key = { it.identity.renderKey }) { identified ->
            itemContent(identified.item, identified.identity)
        }
    }
}

@Composable
private fun MobileMediaArtwork(
    art: MediaArtObject,
    resolution: ImageResolution?,
    imageLoader: ImageLoader?,
    fallback: MediaArtFallback?,
    contentDescription: String,
    modifier: Modifier = Modifier,
) {
    if (imageLoader != null) {
        FerrexMediaArt(
            art = art,
            resolution = resolution,
            imageLoader = imageLoader,
            contentDescription = contentDescription,
            modifier = modifier,
            fallback = fallback,
        )
    } else {
        MobileMediaArtworkPlaceholder(
            art = art,
            label = if (art.requestKey == null) art.fallbackLabel else "Images unavailable",
            contentDescription = contentDescription,
            modifier = modifier,
        )
    }
}

@Composable
private fun MobileMediaArtworkPlaceholder(
    art: MediaArtObject,
    label: String,
    contentDescription: String,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .aspectRatio(art.displaySize.aspectRatio)
            .mobileMediaArtHeightBounds(art)
            .background(FerrexDesignTokens.Palette.PosterFallback, FerrexDesignTokens.Shapes.PosterImage)
            .semantics(mergeDescendants = true) { this.contentDescription = contentDescription },
        contentAlignment = Alignment.Center,
    ) {
        TheaterPlateText(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Sm),
            text = label,
            role = TheaterPlateTypographyRole.Metadata,
            maxLines = 3,
        )
    }
}

@Composable
private fun MobileMediaCardCopy(
    title: String,
    subtitle: String?,
    metadata: String?,
    state: MobileMediaCardState,
    density: FerrexStageDensityFamily,
    layout: MobileMediaCardLayout,
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
) {
    val typographyDensity = density.toMobileTypographyDensity()
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(layout.copyGap),
    ) {
        MobileMediaBadges(
            labels = state.visibleBadges,
            density = density,
            maxVisible = layout.visibleBadgeLimit,
            includeActionBadges = layout.showsActionBadge,
        )
        TheaterPlateText(
            text = title,
            role = TheaterPlateTypographyRole.RailTitle,
            densityRole = typographyDensity,
            maxLines = layout.titleMaxLines,
        )
        subtitle?.takeIf { it.isNotBlank() }?.let {
            TheaterPlateText(
                text = it,
                role = TheaterPlateTypographyRole.RailSubtitle,
                densityRole = typographyDensity,
                maxLines = layout.subtitleMaxLines,
            )
        }
        metadata?.takeIf { it.isNotBlank() }?.let {
            TheaterPlateText(
                text = it,
                role = TheaterPlateTypographyRole.Metadata,
                densityRole = typographyDensity,
                maxLines = layout.metadataMaxLines,
            )
        }
        state.progressFraction?.let { progress ->
            MobileMediaProgressBar(progress = progress, label = state.progressLabel ?: "$title progress")
        }
        if (layout == MobileMediaCardLayout.Hero && onClick != null && state.actionLabel != null) {
            FerrexActionButton(
                label = state.actionLabel,
                role = state.actionRole,
                enabled = state.enabled,
                onClick = onClick,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun MobileMediaBadges(
    labels: List<String>,
    density: FerrexStageDensityFamily,
    maxVisible: Int,
    includeActionBadges: Boolean,
) {
    val visible = labels
        .filter { it.isNotBlank() && (includeActionBadges || !it.startsWith("Action:")) }
        .distinct()
        .take(maxVisible.coerceAtLeast(0))
    if (visible.isEmpty()) return

    val scrollState = rememberScrollState()
    Row(
        modifier = if (visible.size > 1) Modifier.horizontalScroll(scrollState) else Modifier,
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
    ) {
        visible.forEach { label ->
            MobileMediaBadge(label = label, density = density)
        }
    }
}

@Composable
private fun MobileMediaBadge(label: String, density: FerrexStageDensityFamily) {
    val colors = if (label.startsWith("Action:")) {
        FerrexActionRole.Primary.statusTone().colors()
    } else {
        FerrexStatusTone.Secondary.colors()
    }
    androidx.compose.material3.Surface(
        shape = FerrexDesignTokens.Shapes.Pill,
        color = colors.container,
        contentColor = colors.content,
        border = null,
        tonalElevation = 0.dp,
        shadowElevation = 0.dp,
    ) {
        TheaterPlateText(
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
            text = label,
            role = TheaterPlateTypographyRole.Metadata,
            densityRole = density.toMobileTypographyDensity(),
            color = colors.accent,
            maxLines = 1,
        )
    }
}

@Composable
private fun MobileMediaProgressBar(progress: Float, label: String) {
    val coerced = progress.coerceIn(0f, 1f)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(6.dp)
            .background(FerrexDesignTokens.Palette.SlateLine, FerrexDesignTokens.Shapes.Pill)
            .semantics { contentDescription = label },
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth(coerced)
                .height(6.dp)
                .background(FerrexDesignTokens.Palette.SignalCyan, FerrexDesignTokens.Shapes.Pill),
        )
    }
}

private data class IdentifiedMobileRailItem<T>(
    val item: T,
    val identity: MediaRailItemIdentity,
)

private fun <T> List<T>.withRailIdentities(
    railKey: String,
    itemStableId: (T) -> String,
): List<IdentifiedMobileRailItem<T>> {
    val identities = MediaRailIdentityResolver.assign(railKey, map(itemStableId))
    return zip(identities) { item, identity -> IdentifiedMobileRailItem(item, identity) }
}

private fun MobileMediaCardState.surfaceTone(): FerrexStageSurfaceTone = when {
    !enabled -> FerrexStageSurfaceTone.StaleOffline
    artworkLabels.any { it.contains("Failed", ignoreCase = true) } -> FerrexStageSurfaceTone.Warning
    artworkLabels.any { it.contains("Stale/offline", ignoreCase = true) || it.contains("Offline", ignoreCase = true) } -> FerrexStageSurfaceTone.StaleOffline
    watchState == MobileMediaWatchState.Watched -> FerrexStageSurfaceTone.Primary
    progressFraction != null -> FerrexStageSurfaceTone.Cache
    else -> FerrexStageSurfaceTone.Neutral
}

private fun MobileMediaCardLayout.heroArtWidth(density: FerrexStageDensityFamily): Dp = when (density) {
    FerrexStageDensityFamily.Compact -> 104.dp
    FerrexStageDensityFamily.Standard -> 124.dp
    FerrexStageDensityFamily.TenFoot -> 154.dp
}

private fun Modifier.mobileMediaArtHeightBounds(art: MediaArtObject): Modifier {
    val min = art.displaySize.minHeightDp?.dp
    val max = art.displaySize.maxHeightDp?.dp
    return if (min != null || max != null) {
        heightIn(min = min ?: 0.dp, max = max ?: Dp.Infinity)
    } else {
        this
    }
}

private fun FerrexStageDensityFamily.toMobileTypographyDensity(): TheaterPlateDensityRole = when (this) {
    FerrexStageDensityFamily.Compact -> TheaterPlateDensityRole.PhonePortrait
    FerrexStageDensityFamily.Standard -> TheaterPlateDensityRole.PhoneLandscape
    FerrexStageDensityFamily.TenFoot -> TheaterPlateDensityRole.Tv1080p
}
