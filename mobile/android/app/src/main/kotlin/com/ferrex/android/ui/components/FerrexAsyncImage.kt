package com.ferrex.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import coil.ImageLoader
import coil.compose.AsyncImage
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageResolution

@Composable
fun FerrexAsyncImage(
    resolution: ImageResolution?,
    imageLoader: ImageLoader,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    category: BrowseImageCategory = resolution?.key?.category ?: BrowseImageCategory.Poster,
) {
    val baseModifier = modifier
        .fillMaxWidth()
        .aspectRatio(category.placeholderAspectRatio)
    when (resolution) {
        is ImageResolution.Ready -> Box(modifier = baseModifier) {
            AsyncImage(
                model = resolution.url,
                imageLoader = imageLoader,
                contentDescription = contentDescription,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
            if (resolution.stale) {
                Text(
                    modifier = Modifier
                        .align(Alignment.TopStart)
                        .background(MaterialTheme.colorScheme.secondaryContainer)
                        .padding(horizontal = 8.dp, vertical = 4.dp),
                    text = "Offline image",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSecondaryContainer,
                )
            }
        }
        is ImageResolution.Pending -> ImageStatePlaceholder(
            label = stalePrefix(resolution.stale) + "Image pending. Retry after ${resolution.retryAfterMillis} ms.",
            modifier = baseModifier,
        )
        is ImageResolution.Failed -> ImageStatePlaceholder(
            label = stalePrefix(resolution.stale) + resolution.reason,
            modifier = baseModifier,
        )
        is ImageResolution.Placeholder, null -> ImageStatePlaceholder(
            label = (resolution as? ImageResolution.Placeholder)?.reason ?: "Image unavailable",
            modifier = baseModifier,
        )
    }
}

private fun stalePrefix(stale: Boolean): String = if (stale) "Offline image. " else ""

@Composable
private fun ImageStatePlaceholder(
    label: String,
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
    }
}
