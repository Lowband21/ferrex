package com.ferrex.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import coil.ImageLoader
import coil.compose.AsyncImage
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.mediaart.MediaArtFallback
import com.ferrex.android.core.mediaart.MediaArtFitPolicy
import com.ferrex.android.core.mediaart.MediaArtGrounding
import com.ferrex.android.core.mediaart.MediaArtObject
import com.ferrex.android.core.mediaart.MediaArtRequest
import com.ferrex.android.core.mediaart.MediaArtVisualState
import com.ferrex.android.ui.theme.FerrexDesignTokens

typealias FerrexImageFallback = MediaArtFallback

@Composable
fun FerrexAsyncImage(
    resolution: ImageResolution?,
    imageLoader: ImageLoader,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    category: BrowseImageCategory = resolution?.key?.category ?: BrowseImageCategory.Poster,
    fallback: FerrexImageFallback? = null,
) {
    FerrexMediaArt(
        art = MediaArtObject.forCategory(
            category = category,
            request = resolution?.key?.let { MediaArtRequest(it) },
            fallbackLabel = defaultFallbackLabel(category),
        ),
        resolution = resolution,
        imageLoader = imageLoader,
        contentDescription = contentDescription,
        modifier = modifier,
        fallback = fallback,
    )
}

@Composable
fun FerrexMediaArt(
    art: MediaArtObject,
    resolution: ImageResolution?,
    imageLoader: ImageLoader,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    fallback: MediaArtFallback? = null,
) {
    val visualState = MediaArtVisualState.from(art, resolution, fallback)
    val shape = FerrexDesignTokens.Shapes.PosterImage
    val baseModifier = modifier
        .fillMaxWidth()
        .aspectRatio(art.displaySize.aspectRatio)
        .mediaArtHeightBounds(art)
        .mediaArtGrounding(art.treatment.grounding, shape)
        .clip(shape)
        .background(FerrexDesignTokens.Palette.PosterFallback)
        .then(
            art.targetIdentity?.let { identity ->
                Modifier
                    .testTag("media-art.${identity.focusKey}")
                    .semantics(mergeDescendants = true) {
                        this.contentDescription = contentDescription ?: identity.semanticLabel
                    }
            } ?: contentDescription?.let { description ->
                Modifier.semantics(mergeDescendants = true) {
                    this.contentDescription = description
                }
            } ?: Modifier,
        )

    when (visualState) {
        is MediaArtVisualState.Loaded -> ResolvedMediaArt(
            url = visualState.url,
            imageLoader = imageLoader,
            contentDescription = contentDescription,
            modifier = baseModifier,
            contentScale = art.treatment.fitPolicy.contentScale(),
            alignment = art.treatment.cropPolicy?.focalPoint?.toAlignment() ?: Alignment.Center,
            badges = visualState.screenshotLabels,
        )
        is MediaArtVisualState.Placeholder -> ImageStatePlaceholder(
            label = visualState.label,
            badges = visualState.screenshotLabels,
            modifier = baseModifier,
        )
    }
}

private fun Modifier.mediaArtHeightBounds(art: MediaArtObject): Modifier {
    val min = art.displaySize.minHeightDp?.dp
    val max = art.displaySize.maxHeightDp?.dp
    return if (min != null || max != null) {
        this.heightIn(min = min ?: 0.dp, max = max ?: androidx.compose.ui.unit.Dp.Infinity)
    } else {
        this
    }
}

private fun Modifier.mediaArtGrounding(
    grounding: MediaArtGrounding,
    shape: RoundedCornerShape,
): Modifier = when (grounding) {
    MediaArtGrounding.Flat -> this
    MediaArtGrounding.CardObject -> this.shadow(
        elevation = FerrexDesignTokens.Focus.TvRestingBorder,
        shape = shape,
        clip = false,
    )
    MediaArtGrounding.TheaterPlateContactShadow -> this.shadow(
        elevation = FerrexDesignTokens.Focus.TvFocusedElevation,
        shape = shape,
        clip = false,
    )
}

private fun MediaArtFitPolicy.contentScale(): ContentScale = when (this) {
    MediaArtFitPolicy.Contain -> ContentScale.Fit
    MediaArtFitPolicy.ArtDirectedCrop -> ContentScale.Crop
}

private fun com.ferrex.android.core.mediaart.MediaArtFocalPoint.toAlignment(): Alignment = when {
    y < 0.4f -> Alignment.TopCenter
    y > 0.6f -> Alignment.BottomCenter
    x < 0.4f -> Alignment.CenterStart
    x > 0.6f -> Alignment.CenterEnd
    else -> Alignment.Center
}

private fun defaultFallbackLabel(category: BrowseImageCategory): String = when (category) {
    BrowseImageCategory.Poster -> "No poster"
    BrowseImageCategory.Profile -> "No profile image"
    BrowseImageCategory.Backdrop -> "No backdrop"
    BrowseImageCategory.Episode -> "No still"
}

@Composable
private fun ResolvedMediaArt(
    url: String,
    imageLoader: ImageLoader,
    contentDescription: String?,
    modifier: Modifier,
    contentScale: ContentScale,
    alignment: Alignment,
    badges: List<String>,
) {
    Box(modifier = modifier) {
        AsyncImage(
            model = url,
            imageLoader = imageLoader,
            contentDescription = contentDescription,
            contentScale = contentScale,
            alignment = alignment,
            modifier = Modifier.fillMaxSize(),
        )
        MediaArtBadges(badges = badges)
    }
}

@Composable
private fun MediaArtBadges(badges: List<String>) {
    val visibleBadges = badges.filter { it.isNotBlank() }.distinct().take(3)
    if (visibleBadges.isEmpty()) return

    Column(
        modifier = Modifier
            .padding(FerrexDesignTokens.Space.Xs),
        horizontalAlignment = Alignment.Start,
    ) {
        visibleBadges.forEach { badge ->
            Text(
                modifier = Modifier
                    .padding(bottom = 3.dp)
                    .background(MaterialTheme.colorScheme.secondaryContainer, FerrexDesignTokens.Shapes.Pill)
                    .padding(horizontal = 8.dp, vertical = 4.dp),
                text = badge,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
                maxLines = 1,
            )
        }
    }
}

@Composable
private fun ImageStatePlaceholder(
    label: String,
    badges: List<String>,
    modifier: Modifier,
) {
    Box(
        modifier = modifier.background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            modifier = Modifier
                .fillMaxSize()
                .padding(12.dp),
            text = label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Box(modifier = Modifier.align(Alignment.TopStart)) {
            MediaArtBadges(badges = badges)
        }
    }
}
