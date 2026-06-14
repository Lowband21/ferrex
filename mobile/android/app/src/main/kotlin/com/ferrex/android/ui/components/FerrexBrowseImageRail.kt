package com.ferrex.android.ui.components

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.image.TmdbImageFallbackPolicy
import com.ferrex.android.core.library.BrowseImageCard
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.ServerCacheScope
import kotlinx.coroutines.launch

private const val PRODUCT_COPY_ALLOWS_PUBLIC_CDN_IMAGES = false

@Composable
fun FerrexBrowseImageRail(
    repositoryState: LibraryRepositoryState?,
    scope: ServerCacheScope,
    imageRepository: ImageRepository?,
    imagePipeline: FerrexImagePipeline?,
    modifier: Modifier = Modifier,
    maxImages: Int = 12,
    itemWidth: Dp = 132.dp,
    horizontalAlignment: Alignment.Horizontal = Alignment.Start,
) {
    val cards = remember(repositoryState?.movieAccessor, repositoryState?.seriesAccessor, maxImages) {
        repositoryState?.movieAccessor?.primaryImageCards(maxImages)
            ?: repositoryState?.seriesAccessor?.primaryImageCards(maxImages)
            ?: emptyList()
    }
    val primaryKeys = remember(repositoryState?.movieAccessor, repositoryState?.seriesAccessor) {
        buildSet {
            repositoryState?.movieAccessor?.primaryImageKeys()?.let(::addAll)
            repositoryState?.seriesAccessor?.primaryImageKeys()?.let(::addAll)
        }
    }
    val visibleKeys = remember(cards, primaryKeys) {
        cards.map { it.key }
            .filter { it in primaryKeys }
            .distinct()
    }
    val imageLoader = remember(imagePipeline, scope) { imagePipeline?.imageLoader(scope) }
    var resolutions by remember(scope.directoryName, visibleKeys) {
        mutableStateOf<Map<ImageRequestKey, ImageResolution>>(emptyMap())
    }
    val coroutineScope = rememberCoroutineScope()

    LaunchedEffect(imageRepository, scope, visibleKeys) {
        resolutions = if (imageRepository != null && visibleKeys.isNotEmpty()) {
            imageRepository.resolveImages(scope, visibleKeys)
        } else {
            emptyMap()
        }
    }

    if (cards.isEmpty()) return

    Column(
        modifier = modifier.fillMaxWidth(),
        horizontalAlignment = horizontalAlignment,
    ) {
        Text(
            text = "Browse artwork",
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            modifier = Modifier.padding(top = 6.dp),
            text = "Visible primary image keys resolve through the Ferrex manifest; poster IID fallback stays poster-only and public TMDB CDN fallback is disabled unless product copy opts in.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (imageRepository == null || imageLoader == null) {
            Text(
                modifier = Modifier.padding(top = 8.dp),
                text = "Image pipeline unavailable.",
                style = MaterialTheme.typography.bodySmall,
            )
            return@Column
        }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 12.dp)
                .horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            cards.forEach { card ->
                val resolution = resolutions[card.key]
                val fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
                    card.runtimeFallback(scope.canonicalServerUrl)
                } else {
                    null
                }
                Column(modifier = Modifier.width(itemWidth)) {
                    FerrexAsyncImage(
                        resolution = resolution,
                        imageLoader = imageLoader,
                        contentDescription = card.title,
                        category = card.key.category,
                        fallback = fallback,
                    )
                    Text(
                        modifier = Modifier.padding(top = 6.dp),
                        text = card.title,
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = "${card.subtitle} • ${resolution?.label ?: "manifest lookup queued"}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        Button(
            modifier = Modifier.padding(top = 12.dp),
            enabled = visibleKeys.isNotEmpty(),
            onClick = {
                coroutineScope.launch {
                    resolutions = imageRepository.retryPendingOrFailed(scope, visibleKeys)
                }
            },
        ) {
            Text("Retry visible images")
        }
    }
}

private fun BrowseImageCard.runtimeFallback(serverUrl: String): FerrexImageFallback? {
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
