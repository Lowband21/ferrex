package com.ferrex.android.ui.search

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.unit.dp
import coil.ImageLoader
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.image.TmdbImageFallbackPolicy
import com.ferrex.android.core.library.CachedMediaLookupKey
import com.ferrex.android.core.library.CachedMediaReference
import com.ferrex.android.core.library.CachedMediaType
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.search.MediaSearchOutcome
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.SearchDetailTarget
import com.ferrex.android.core.search.SearchFailureKind
import com.ferrex.android.core.search.SearchMediaType
import com.ferrex.android.core.search.SearchResultRow
import com.ferrex.android.ui.components.FerrexAsyncImage
import com.ferrex.android.ui.components.FerrexImageFallback
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private const val SEARCH_DEBOUNCE_MILLIS = 350L
private const val PRODUCT_COPY_ALLOWS_PUBLIC_CDN_IMAGES = false

@Composable
fun PhoneSearchPanel(
    scope: ServerCacheScope,
    searchRepository: MediaSearchRepository?,
    imageRepository: ImageRepository?,
    imagePipeline: FerrexImagePipeline?,
    onOpenResult: (SearchDetailTarget) -> Unit,
    modifier: Modifier = Modifier,
) {
    var query by remember(scope.directoryName) { mutableStateOf("") }
    var retryNonce by remember(scope.directoryName) { mutableStateOf(0) }
    var uiState by remember(scope.directoryName) { mutableStateOf<PhoneSearchUiState>(PhoneSearchUiState.Idle) }

    LaunchedEffect(searchRepository, scope.directoryName, query, retryNonce) {
        val trimmed = query.trim()
        if (searchRepository == null) {
            uiState = PhoneSearchUiState.Unavailable
            return@LaunchedEffect
        }
        if (trimmed.isEmpty()) {
            uiState = PhoneSearchUiState.Idle
            return@LaunchedEffect
        }
        if (trimmed.length < 2) {
            uiState = PhoneSearchUiState.KeepTyping
            return@LaunchedEffect
        }
        delay(SEARCH_DEBOUNCE_MILLIS)
        uiState = PhoneSearchUiState.Loading(trimmed)
        uiState = PhoneSearchUiState.Loaded(searchRepository.search(scope, trimmed))
    }

    val rows = ((uiState as? PhoneSearchUiState.Loaded)?.outcome as? MediaSearchOutcome.Results)?.rows.orEmpty()
    val visibleKeys = remember(rows) {
        rows.filterIsInstance<SearchResultRow.Resolved>()
            .mapNotNull { it.imageKey }
            .distinct()
    }
    val imageLoader = remember(imagePipeline, scope.directoryName) { imagePipeline?.imageLoader(scope) }
    var resolutions by remember(scope.directoryName, visibleKeys) {
        mutableStateOf<Map<ImageRequestKey, ImageResolution>>(emptyMap())
    }

    LaunchedEffect(imageRepository, scope.directoryName, visibleKeys) {
        resolutions = if (imageRepository != null && visibleKeys.isNotEmpty()) {
            imageRepository.resolveImages(scope, visibleKeys)
        } else {
            emptyMap()
        }
    }

    fun retrySearch() {
        retryNonce += 1
    }

    Column(modifier = modifier.fillMaxWidth()) {
        Text(
            text = "Search cached media",
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            modifier = Modifier.padding(top = 6.dp),
            text = "Search posts the JSON media query contract and resolves results through the scoped library cache. Cache misses stay visible with retry instead of being dropped.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        OutlinedTextField(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 12.dp),
            value = query,
            onValueChange = { query = it },
            label = { Text("Movies, shows, seasons, episodes…") },
            singleLine = true,
            enabled = searchRepository != null,
            trailingIcon = {
                if (query.isNotEmpty()) {
                    TextButton(onClick = {
                        query = ""
                        uiState = PhoneSearchUiState.Idle
                    }) {
                        Text("Clear")
                    }
                }
            },
        )
        when (val state = uiState) {
            PhoneSearchUiState.Idle -> SearchCopy("Enter at least two characters to search the current server.")
            PhoneSearchUiState.KeepTyping -> SearchCopy("Keep typing to start search.")
            PhoneSearchUiState.Unavailable -> SearchCopy("Search is unavailable until the protected API client is ready.")
            is PhoneSearchUiState.Loading -> SearchLoading(state.query)
            is PhoneSearchUiState.Loaded -> SearchOutcomeContent(
                outcome = state.outcome,
                imageLoader = imageLoader,
                scope = scope,
                onOpenResult = onOpenResult,
                onRetry = { retrySearch() },
                onClear = {
                    query = ""
                    uiState = PhoneSearchUiState.Idle
                },
                resolveImage = { key -> resolutions[key] },
            )
        }
    }
}

@Composable
fun PhoneSearchDetailScreen(
    scope: ServerCacheScope,
    mediaType: SearchMediaType,
    mediaId: String,
    libraryId: String?,
    libraryRepository: LibraryRepository?,
    imageRepository: ImageRepository?,
    imagePipeline: FerrexImagePipeline?,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var refreshNonce by remember(scope.directoryName, mediaType, mediaId) { mutableStateOf(0) }
    val reference = remember(libraryRepository, scope.directoryName, mediaType, mediaId, refreshNonce) {
        libraryRepository?.resolveCachedMedia(scope, CachedMediaLookupKey(mediaType.toCachedType(), mediaId))
    }
    val imageKey = reference?.imageKey
    val imageLoader = remember(imagePipeline, scope.directoryName) { imagePipeline?.imageLoader(scope) }
    var resolution by remember(scope.directoryName, imageKey, refreshNonce) { mutableStateOf<ImageResolution?>(null) }
    val coroutineScope = rememberCoroutineScope()

    LaunchedEffect(imageRepository, scope.directoryName, imageKey, refreshNonce) {
        resolution = if (imageRepository != null && imageKey != null) {
            imageRepository.resolveImages(scope, listOf(imageKey))[imageKey]
        } else {
            null
        }
    }

    Column(modifier = modifier.fillMaxWidth()) {
        Text(
            text = reference?.title ?: "Media detail",
            style = MaterialTheme.typography.headlineSmall,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            modifier = Modifier.padding(top = 8.dp),
            text = "Route: ${mediaType.routeSegment}/$mediaId${libraryId?.let { " in library $it" }.orEmpty()}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (reference != null) {
            Text(
                modifier = Modifier.padding(top = 14.dp),
                text = reference.detailCopy(),
                style = MaterialTheme.typography.bodyMedium,
            )
            if (imageLoader != null && imageKey != null) {
                Box(modifier = Modifier.padding(top = 12.dp).width(180.dp)) {
                    FerrexAsyncImage(
                        resolution = resolution,
                        imageLoader = imageLoader,
                        contentDescription = reference.title,
                        category = imageKey.category,
                        fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
                            reference.runtimeFallback(scope.canonicalServerUrl)
                        } else {
                            null
                        },
                    )
                }
            }
        } else {
            Text(
                modifier = Modifier.padding(top = 14.dp),
                text = "This detail route has media type, media id, and ${if (libraryId == null) "no known library id" else "library id $libraryId"}, but the scoped cache does not currently contain the referenced bundle. Retry sync here, or go back to search where sign-out, server change, and reset remain available.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
        }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Button(onClick = onBack, modifier = Modifier.weight(1f)) {
                Text("Back")
            }
            TextButton(
                onClick = {
                    coroutineScope.launch {
                        libraryRepository?.resyncCachedMediaForSearch(scope, CachedMediaLookupKey(mediaType.toCachedType(), mediaId))
                        refreshNonce += 1
                    }
                },
                modifier = Modifier.weight(1f),
            ) {
                Text("Retry sync")
            }
        }
    }
}

@Composable
private fun SearchOutcomeContent(
    outcome: MediaSearchOutcome,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    onOpenResult: (SearchDetailTarget) -> Unit,
    onRetry: () -> Unit,
    onClear: () -> Unit,
    resolveImage: (ImageRequestKey) -> ImageResolution?,
) {
    when (outcome) {
        MediaSearchOutcome.Idle -> SearchCopy("Enter at least two characters to search the current server.")
        is MediaSearchOutcome.NoResults -> {
            SearchCopy("No results for “${outcome.query}”. Try a shorter title or alternate spelling.")
            SearchActionRow(onRetry = onRetry, onClear = onClear)
        }
        is MediaSearchOutcome.Failure -> {
            val prefix = when (outcome.kind) {
                SearchFailureKind.NetworkOffline -> "Search is offline. "
                SearchFailureKind.Http -> "Search HTTP error. "
                SearchFailureKind.Server -> "Server search error. "
                SearchFailureKind.InvalidResponse -> "Search response changed. "
            }
            SearchCopy(prefix + outcome.message, error = true)
            SearchActionRow(onRetry = onRetry, onClear = onClear, retryEnabled = outcome.retryable)
        }
        is MediaSearchOutcome.Results -> {
            if (outcome.staleCache) {
                SearchCopy("Cache is stale or retryable; resolved rows use the current scoped cache and misses can be repaired with Retry sync.")
            }
            if (imageLoader == null && outcome.rows.any { it is SearchResultRow.Resolved && it.imageKey != null }) {
                SearchCopy("Image pipeline unavailable; showing image placeholders.")
            }
            Column(
                modifier = Modifier.padding(top = 12.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                outcome.rows.take(12).forEach { row ->
                    when (row) {
                        is SearchResultRow.Resolved -> ResolvedSearchRow(
                            row = row,
                            resolution = row.imageKey?.let(resolveImage),
                            scope = scope,
                            imageLoader = imageLoader,
                            onOpenResult = onOpenResult,
                        )
                        is SearchResultRow.CacheMiss -> CacheMissRow(row = row, onRetry = onRetry, onClear = onClear)
                    }
                }
                if (outcome.rows.size > 12) {
                    Text(
                        text = "Showing 12 of ${outcome.rows.size} results. Narrow the query for more focused rows.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

@Composable
private fun ResolvedSearchRow(
    row: SearchResultRow.Resolved,
    resolution: ImageResolution?,
    scope: ServerCacheScope,
    imageLoader: ImageLoader?,
    onOpenResult: (SearchDetailTarget) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .padding(10.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val imageKey = row.imageKey
        if (imageLoader != null && imageKey != null) {
            Box(modifier = Modifier.width(72.dp)) {
                FerrexAsyncImage(
                    resolution = resolution,
                    imageLoader = imageLoader,
                    contentDescription = row.title,
                    category = imageKey.category,
                    fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
                        row.runtimeFallback(scope.canonicalServerUrl)
                    } else {
                        null
                    },
                )
            }
        } else {
            Box(
                modifier = Modifier
                    .width(72.dp)
                    .background(MaterialTheme.colorScheme.surface),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    modifier = Modifier.padding(8.dp),
                    text = "Image unavailable",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = row.title,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                modifier = Modifier.padding(top = 4.dp),
                text = "${row.subtitle} • image ${resolution?.label ?: "queued"}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        TextButton(onClick = { onOpenResult(row.target) }) {
            Text("Open")
        }
    }
}

@Composable
private fun CacheMissRow(
    row: SearchResultRow.CacheMiss,
    onRetry: () -> Unit,
    onClear: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.errorContainer)
            .padding(10.dp),
    ) {
        Text(
            text = row.title,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onErrorContainer,
        )
        Text(
            modifier = Modifier.padding(top = 4.dp),
            text = row.message,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onErrorContainer,
        )
        SearchActionRow(onRetry = onRetry, onClear = onClear, retryEnabled = row.retryable)
    }
}

@Composable
private fun SearchActionRow(
    onRetry: () -> Unit,
    onClear: () -> Unit,
    retryEnabled: Boolean = true,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Button(onClick = onRetry, enabled = retryEnabled, modifier = Modifier.weight(1f)) {
            Text("Retry sync / search")
        }
        TextButton(onClick = onClear, modifier = Modifier.weight(1f)) {
            Text("Clear search")
        }
    }
}

@Composable
private fun SearchLoading(query: String) {
    Row(
        modifier = Modifier.padding(top = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CircularProgressIndicator()
        Text("Searching “$query”…", style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun SearchCopy(message: String, error: Boolean = false) {
    Text(
        modifier = Modifier.padding(top = 12.dp),
        text = message,
        style = MaterialTheme.typography.bodySmall,
        color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

private fun SearchMediaType.toCachedType(): CachedMediaType = when (this) {
    SearchMediaType.Movie -> CachedMediaType.Movie
    SearchMediaType.Series -> CachedMediaType.Series
    SearchMediaType.Season -> CachedMediaType.Season
    SearchMediaType.Episode -> CachedMediaType.Episode
}

private fun CachedMediaReference.detailCopy(): String = when (this) {
    is CachedMediaReference.Movie -> "Cached movie from library $libraryId."
    is CachedMediaReference.Series -> "Cached series from library $libraryId."
    is CachedMediaReference.Season -> "Cached season $seasonNumber from library $libraryId; search routes this to series $seriesId."
    is CachedMediaReference.Episode -> "Cached episode S$seasonNumber E$episodeNumber from library $libraryId; search routes this to series $seriesId."
}

private fun CachedMediaReference.runtimeFallback(serverUrl: String): FerrexImageFallback? = runtimeFallback(
    serverUrl = serverUrl,
    key = imageKey,
    publicFallbackPath = publicFallbackPath,
)

private fun SearchResultRow.Resolved.runtimeFallback(serverUrl: String): FerrexImageFallback? = runtimeFallback(
    serverUrl = serverUrl,
    key = imageKey,
    publicFallbackPath = publicFallbackPath,
)

private fun runtimeFallback(
    serverUrl: String,
    key: ImageRequestKey?,
    publicFallbackPath: String?,
): FerrexImageFallback? {
    key ?: return null
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

private sealed interface PhoneSearchUiState {
    data object Idle : PhoneSearchUiState
    data object KeepTyping : PhoneSearchUiState
    data object Unavailable : PhoneSearchUiState
    data class Loading(val query: String) : PhoneSearchUiState
    data class Loaded(val outcome: MediaSearchOutcome) : PhoneSearchUiState
}
