package com.ferrex.android.ui.search

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
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
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.search.MediaSearchOutcome
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.SearchDetailTarget
import com.ferrex.android.core.search.SearchFailureKind
import com.ferrex.android.core.search.SearchResultRow
import com.ferrex.android.ui.components.FerrexActionButton
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexAsyncImage
import com.ferrex.android.ui.components.FerrexImageFallback
import com.ferrex.android.ui.components.FerrexPosterPlaceholder
import com.ferrex.android.ui.components.FerrexStatusCard
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.theme.FerrexDesignTokens
import kotlinx.coroutines.delay

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
    onOpenDiagnostics: (() -> Unit)? = null,
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
                onOpenDiagnostics = onOpenDiagnostics,
            )
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
    onOpenDiagnostics: (() -> Unit)?,
) {
    when (outcome) {
        MediaSearchOutcome.Idle -> SearchCopy("Enter at least two characters to search the current server.")
        is MediaSearchOutcome.NoResults -> {
            SearchStatusCard(
                title = "No results",
                body = "No cached media matched “${outcome.query}”. Try a shorter title or alternate spelling, then retry sync if the item should be cached.",
            )
            SearchActionRow(onRetry = onRetry, onClear = onClear)
        }
        is MediaSearchOutcome.Failure -> {
            val title = when (outcome.kind) {
                SearchFailureKind.NetworkOffline -> "Search is offline"
                SearchFailureKind.Http -> "Search HTTP error"
                SearchFailureKind.Server -> "Server search error"
                SearchFailureKind.InvalidResponse -> "Search response changed"
            }
            SearchStatusCard(title = title, body = outcome.message, tone = FerrexStatusTone.Error)
            SearchActionRow(onRetry = onRetry, onClear = onClear, retryEnabled = outcome.retryable, onOpenDiagnostics = onOpenDiagnostics)
        }
        is MediaSearchOutcome.Results -> {
            if (outcome.staleCache) {
                SearchStatusCard(
                    title = "Cache needs retry",
                    body = "Resolved rows use the current scoped cache. Cache misses stay visible and can be repaired with Retry sync.",
                    tone = FerrexStatusTone.StaleOffline,
                )
            }
            if (imageLoader == null && outcome.rows.any { it is SearchResultRow.Resolved && it.imageKey != null }) {
                SearchStatusCard(
                    title = "Images unavailable",
                    body = "The image pipeline is unavailable; search keeps poster slots visible with placeholders instead of dropping results.",
                    tone = FerrexStatusTone.StaleOffline,
                )
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
                        is SearchResultRow.CacheMiss -> CacheMissRow(row = row, onRetry = onRetry, onClear = onClear, onOpenDiagnostics = onOpenDiagnostics)
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
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = FerrexDesignTokens.Shapes.Card,
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, MaterialTheme.colorScheme.outline.copy(alpha = 0.45f)),
    ) {
        Row(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Md),
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val imageKey = row.imageKey
            if (imageLoader != null && imageKey != null) {
                Box(modifier = Modifier.width(76.dp)) {
                    FerrexAsyncImage(
                        resolution = resolution,
                        imageLoader = imageLoader,
                        contentDescription = row.title,
                        category = imageKey.category,
                        fallback = if (resolution !is ImageResolution.Ready) {
                            row.runtimeFallback(scope.canonicalServerUrl)
                        } else {
                            null
                        },
                    )
                }
            } else {
                FerrexPosterPlaceholder(
                    label = if (imageKey == null) "No poster" else "Images unavailable",
                    modifier = Modifier.width(76.dp),
                )
            }
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
            ) {
                Text(
                    text = row.sourceId.type.jsonVariant,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary,
                )
                Text(
                    text = row.title,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = row.subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = "Image ${resolution?.label ?: "queued"}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            TextButton(onClick = { onOpenResult(row.target) }) {
                Text("Open")
            }
        }
    }
}

@Composable
private fun CacheMissRow(
    row: SearchResultRow.CacheMiss,
    onRetry: () -> Unit,
    onClear: () -> Unit,
    onOpenDiagnostics: (() -> Unit)?,
) {
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        FerrexStatusCard(
            title = row.title,
            body = buildString {
                append(row.message)
                if (row.attemptedLibraryIds.isNotEmpty()) {
                    append(" Attempted ")
                    append(row.attemptedLibraryIds.size)
                    append(" cached library root(s).")
                }
            },
            tone = FerrexStatusTone.Error,
        )
        SearchActionRow(onRetry = onRetry, onClear = onClear, retryEnabled = row.retryable, onOpenDiagnostics = onOpenDiagnostics)
    }
}

@Composable
private fun SearchActionRow(
    onRetry: () -> Unit,
    onClear: () -> Unit,
    retryEnabled: Boolean = true,
    onOpenDiagnostics: (() -> Unit)? = null,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = FerrexDesignTokens.Space.Sm),
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
    ) {
        FerrexActionButton(
            label = "Retry sync / search",
            role = FerrexActionRole.Retry,
            enabled = retryEnabled,
            onClick = onRetry,
            modifier = Modifier.weight(1f),
        )
        FerrexActionButton(
            label = "Clear search",
            role = FerrexActionRole.Secondary,
            onClick = onClear,
            modifier = Modifier.weight(1f),
        )
        onOpenDiagnostics?.let {
            FerrexActionButton(
                label = "Diagnostics",
                role = FerrexActionRole.Secondary,
                onClick = it,
                modifier = Modifier.weight(1f),
            )
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
private fun SearchStatusCard(
    title: String,
    body: String,
    tone: FerrexStatusTone = FerrexStatusTone.Secondary,
) {
    FerrexStatusCard(
        modifier = Modifier.padding(top = FerrexDesignTokens.Space.Md),
        title = title,
        body = body,
        tone = tone,
    )
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
