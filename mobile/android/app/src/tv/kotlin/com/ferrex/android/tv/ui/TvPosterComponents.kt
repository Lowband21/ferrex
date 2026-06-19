package com.ferrex.android.tv.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import coil.ImageLoader
import com.ferrex.android.core.browse.LibraryMediaCard
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.image.TmdbImageFallbackPolicy
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.mediaart.MediaRailIdentityResolver
import com.ferrex.android.core.mediaart.MediaRailItemIdentity
import com.ferrex.android.core.watch.ContinueWatchingCard
import com.ferrex.android.tv.ui.foundation.TvFocusableSurface
import com.ferrex.android.tv.ui.foundation.TvFocusRestorer
import com.ferrex.android.ui.theme.TvFocusTreatmentRole
import com.ferrex.android.ui.components.FerrexAsyncImage
import com.ferrex.android.ui.components.FerrexImageFallback
import com.ferrex.android.ui.components.TheaterPlateDensityRole
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theaterplate.FerrexStageDensityFamily
import com.ferrex.android.ui.theaterplate.FerrexStageSurface
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceTone
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceVariant
import com.ferrex.android.ui.theme.FerrexDesignTokens

@Composable
internal fun TvPosterRow(
    title: String?,
    supportingText: String,
    entries: List<TvPosterEntry>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRestorer: TvFocusRestorer,
    surfaceKey: String,
    autoFocus: Boolean,
    onSelect: (TvPosterEntry) -> Unit,
) {
    if (entries.isEmpty()) return
    val railItems = remember(surfaceKey, entries) {
        entries.zip(
            MediaRailIdentityResolver.assign(
                railKey = surfaceKey,
                stableIds = entries.map { it.stableKey },
            ),
        ).map { (entry, identity) -> TvRailPosterItem(entry, identity) }
    }
    val keys = railItems.map { it.identity.renderKey }
    val requesters = remember(keys) { keys.associateWith { FocusRequester() } }
    val restoredKey = keys.firstOrNull()?.let { fallback ->
        focusRestorer.restoreItem(surfaceKey, keys, fallback)
    }
    LaunchedEffect(autoFocus, restoredKey, keys) {
        if (autoFocus && restoredKey != null) {
            runCatching { requesters[restoredKey]?.requestFocus() }
        }
    }
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.RailBand,
        density = FerrexStageDensityFamily.TenFoot,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = Modifier.fillMaxWidth(),
        contentDescription = title ?: "$surfaceKey media shelf",
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            title?.let { TvSectionHeader(it) }
            TheaterPlateText(
                text = supportingText,
                role = TheaterPlateTypographyRole.RailSubtitle,
                densityRole = TheaterPlateDensityRole.Tv1080p,
                maxLines = 3,
            )
            LazyRow(
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
                contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Lg),
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(FerrexQaTags.Tv.surface(surfaceKey))
                    .focusGroup(),
            ) {
                items(railItems, key = { it.identity.focusKey }) { railItem ->
                    val entry = railItem.entry
                    val itemKey = railItem.identity.renderKey
                    TvPosterCard(
                        entry = entry,
                        imageResolutions = imageResolutions,
                        imageLoader = imageLoader,
                        scope = scope,
                        focusRequester = requesters[itemKey],
                        semanticLabel = railItem.identity.semanticLabel(entry.title),
                        onFocused = { focusRestorer.record(surfaceKey, itemKey) },
                        onSelect = { onSelect(entry) },
                        modifier = Modifier.width(FerrexDesignTokens.Poster.TvWidth),
                        testTag = FerrexQaTags.Tv.poster(surfaceKey, itemKey),
                    )
                }
            }
        }
    }
}

@Composable
internal fun TvPosterCard(
    entry: TvPosterEntry,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRequester: FocusRequester?,
    semanticLabel: String = entry.title,
    onFocused: () -> Unit,
    onSelect: () -> Unit,
    modifier: Modifier = Modifier,
    density: TvPosterCardDensity = TvPosterCardDensity.Standard,
    testTag: String? = null,
) {
    TvFocusableSurface(
        onClick = onSelect,
        semanticLabel = entry.contentDescription(semanticLabel),
        modifier = modifier,
        focusRequester = focusRequester,
        minHeight = density.minHeight,
        focusTreatmentRole = density.focusTreatmentRole,
        contentPadding = density.contentPadding,
        testTag = testTag,
        onFocused = onFocused,
    ) {
        when (density) {
            TvPosterCardDensity.Standard -> StandardTvPosterCardContent(
                entry = entry,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
            )
            TvPosterCardDensity.DenseGrid -> DenseTvPosterCardContent(
                entry = entry,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
            )
        }
    }
}

@Composable
private fun StandardTvPosterCardContent(
    entry: TvPosterEntry,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ProjectionShelf,
        density = FerrexStageDensityFamily.TenFoot,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = Modifier.fillMaxWidth(),
        contentDescription = null,
    ) {
        Column(modifier = Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            Poster(
                imageKey = entry.imageKey,
                title = entry.title,
                fallbackPath = entry.publicFallbackPath,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
            )
            TheaterPlateText(
                text = entry.title,
                role = TheaterPlateTypographyRole.RailTitle,
                densityRole = TheaterPlateDensityRole.Tv1080p,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            TheaterPlateText(
                text = entry.subtitle,
                role = TheaterPlateTypographyRole.RailSubtitle,
                densityRole = TheaterPlateDensityRole.Tv1080p,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            entry.tertiary?.let {
                TheaterPlateText(
                    text = it,
                    role = TheaterPlateTypographyRole.FactLabel,
                    densityRole = TheaterPlateDensityRole.Tv1080p,
                )
            }
        }
    }
}

@Composable
private fun DenseTvPosterCardContent(
    entry: TvPosterEntry,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.DenseLibraryGrid.tvCopyGap),
    ) {
        Poster(
            imageKey = entry.imageKey,
            title = entry.title,
            fallbackPath = entry.publicFallbackPath,
            imageResolutions = imageResolutions,
            imageLoader = imageLoader,
            scope = scope,
        )
        TheaterPlateText(
            text = entry.title,
            role = TheaterPlateTypographyRole.RailTitle,
            densityRole = TheaterPlateDensityRole.Tv1080p,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        TheaterPlateText(
            text = entry.subtitle,
            role = TheaterPlateTypographyRole.RailSubtitle,
            densityRole = TheaterPlateDensityRole.Tv1080p,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        (entry.libraryName ?: entry.tertiary)?.let {
            TheaterPlateText(
                text = it,
                role = TheaterPlateTypographyRole.FactLabel,
                densityRole = TheaterPlateDensityRole.Tv1080p,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
internal fun Poster(
    imageKey: ImageRequestKey?,
    title: String,
    fallbackPath: String?,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
) {
    if (imageKey == null || imageLoader == null) {
        PosterPlaceholder(if (imageKey == null) "No poster" else "Images unavailable")
        return
    }
    val resolution = imageResolutions[imageKey]
    FerrexAsyncImage(
        resolution = resolution,
        imageLoader = imageLoader,
        contentDescription = title,
        category = imageKey.category,
        fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
            runtimeFallback(scope.canonicalServerUrl, imageKey, fallbackPath)
        } else {
            null
        },
    )
}

@Composable
internal fun PosterPlaceholder(label: String) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(FerrexDesignTokens.Poster.AspectRatio)
            .background(FerrexDesignTokens.Palette.PosterFallback, FerrexDesignTokens.Shapes.PosterImage),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
    }
}

internal fun ContinueWatchingCard.toPosterEntry(): TvPosterEntry = TvPosterEntry(
    stableKey = stableKey,
    title = title,
    subtitle = subtitle,
    tertiary = progressLabel,
    imageKey = imageKey,
    publicFallbackPath = null,
    route = route,
    badges = listOfNotNull(progressLabel?.takeIf { it.isNotBlank() }?.let { "Progress: $it" }),
)

internal fun LibraryMediaCard.toPosterEntry(): TvPosterEntry = TvPosterEntry(
    stableKey = stableKey,
    title = title,
    subtitle = subtitle,
    tertiary = libraryName,
    imageKey = imageKey,
    publicFallbackPath = publicFallbackPath,
    route = route,
    libraryName = libraryName,
    badges = listOf(route.mediaType.displayName),
)

internal fun runtimeFallback(
    serverUrl: String,
    key: ImageRequestKey,
    publicFallbackPath: String?,
): FerrexImageFallback? {
    val iidUrl = PosterOnlyIidFallback.url(serverUrl, key)
    val tmdbUrl = TmdbImageFallbackPolicy.publicCdnUrl(
        publicPath = publicFallbackPath,
        category = key.category,
        productCopyAllowsPublicCdn = PRODUCT_COPY_ALLOWS_PUBLIC_CDN_IMAGES,
    )
    return when {
        iidUrl != null -> FerrexImageFallback(iidUrl, "Poster IID fallback")
        tmdbUrl != null -> FerrexImageFallback(tmdbUrl, "TMDB fallback")
        else -> null
    }
}

internal data class TvPosterEntry(
    val stableKey: String,
    val title: String,
    val subtitle: String,
    val tertiary: String?,
    val imageKey: ImageRequestKey?,
    val publicFallbackPath: String?,
    val route: MediaRouteArgs?,
    val libraryName: String? = null,
    val badges: List<String> = emptyList(),
)

internal enum class TvPosterCardDensity {
    Standard,
    DenseGrid,
}

internal fun TvPosterEntry.contentDescription(
    semanticLabel: String,
    actionLabel: String = "Open",
): String = buildList {
    add(semanticLabel.takeIf { it.isNotBlank() } ?: title)
    subtitle.takeIf { it.isNotBlank() }?.let(::add)
    libraryName?.takeIf { it.isNotBlank() }?.let { add("Library: $it") }
    if (libraryName == null) tertiary?.takeIf { it.isNotBlank() }?.let(::add)
    badges.filter { it.isNotBlank() }.forEach(::add)
    add("Action: $actionLabel")
}.distinct().joinToString(". ")

internal val TvPosterCardDensity.minHeight
    get() = when (this) {
        TvPosterCardDensity.Standard -> FerrexDesignTokens.Poster.TvCardMinHeight
        TvPosterCardDensity.DenseGrid -> FerrexDesignTokens.DenseLibraryGrid.tv.cardMinHeight
    }

internal val TvPosterCardDensity.focusTreatmentRole
    get() = when (this) {
        TvPosterCardDensity.Standard -> TvFocusTreatmentRole.Action
        TvPosterCardDensity.DenseGrid -> TvFocusTreatmentRole.MediaArt
    }

internal val TvPosterCardDensity.contentPadding
    get() = when (this) {
        TvPosterCardDensity.Standard -> PaddingValues(
            horizontal = FerrexDesignTokens.Space.Xxl,
            vertical = FerrexDesignTokens.Space.Md,
        )
        TvPosterCardDensity.DenseGrid -> PaddingValues(
            horizontal = FerrexDesignTokens.DenseLibraryGrid.tvCardHorizontalPadding,
            vertical = FerrexDesignTokens.DenseLibraryGrid.tvCardVerticalPadding,
        )
    }

internal data class TvRailPosterItem(
    val entry: TvPosterEntry,
    val identity: MediaRailItemIdentity,
)

internal const val PRODUCT_COPY_ALLOWS_PUBLIC_CDN_IMAGES = false
