package com.ferrex.android.ui.search

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.TheaterPlateDensityRole
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theme.FerrexDesignTokens
import com.ferrex.android.ui.theaterplate.FerrexStageDensityFamily
import com.ferrex.android.ui.theaterplate.tokens
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
    initialQuery: String = "",
    searchDebounceMillis: Long = SEARCH_DEBOUNCE_MILLIS,
    density: FerrexStageDensityFamily = FerrexStageDensityFamily.Standard,
) {
    var query by remember(scope.directoryName, initialQuery) { mutableStateOf(initialQuery) }
    var retryNonce by remember(scope.directoryName) { mutableStateOf(0) }
    var uiState by remember(scope.directoryName) { mutableStateOf<PhoneSearchUiState>(PhoneSearchUiState.Idle) }
    val typographyDensity = density.toSearchTypographyDensity()

    LaunchedEffect(searchRepository, scope.directoryName, query, retryNonce, searchDebounceMillis) {
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
        if (searchDebounceMillis > 0L) {
            delay(searchDebounceMillis)
        }
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

    Column(
        modifier = modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Phone.SearchPanel),
        verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(FerrexQaTags.Phone.SearchHeader)
                .semantics { contentDescription = "Flat search query section with query field and clear action" },
            verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
        ) {
            TheaterPlateText(
                text = "Search cached media",
                role = TheaterPlateTypographyRole.SectionTitle,
                densityRole = typographyDensity,
                maxLines = 2,
            )
            TheaterPlateText(
                text = "Query the server and resolve rows through cached library data. Retry keeps cache misses repairable.",
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = typographyDensity,
                maxLines = 3,
            )
            OutlinedTextField(
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(FerrexQaTags.Phone.SearchField)
                    .semantics { contentDescription = "Search media query field" },
                value = query,
                onValueChange = { query = it },
                label = { Text("Movies, shows, seasons, episodes…") },
                singleLine = true,
                enabled = searchRepository != null,
            )
            if (query.isNotEmpty()) {
                FerrexActionButton(
                    label = "Clear search",
                    role = FerrexActionRole.Secondary,
                    onClick = {
                        query = ""
                        uiState = PhoneSearchUiState.Idle
                    },
                    modifier = Modifier.fillMaxWidth(),
                    testTag = FerrexQaTags.Phone.searchAction("clear"),
                    contentDescription = "Clear search query",
                )
            }
        }
        when (val state = uiState) {
            PhoneSearchUiState.Idle -> SearchCopy("Enter at least two characters to search the current server.", density)
            PhoneSearchUiState.KeepTyping -> SearchCopy("Keep typing to start search.", density)
            PhoneSearchUiState.Unavailable -> SearchCopy("Search is unavailable until the protected API client is ready.", density, error = true)
            is PhoneSearchUiState.Loading -> SearchLoading(state.query, density)
            is PhoneSearchUiState.Loaded -> SearchOutcomeContent(
                outcome = state.outcome,
                imageLoader = imageLoader,
                scope = scope,
                density = density,
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
    density: FerrexStageDensityFamily,
    onOpenResult: (SearchDetailTarget) -> Unit,
    onRetry: () -> Unit,
    onClear: () -> Unit,
    resolveImage: (ImageRequestKey) -> ImageResolution?,
    onOpenDiagnostics: (() -> Unit)?,
) {
    when (outcome) {
        MediaSearchOutcome.Idle -> SearchCopy("Enter at least two characters to search the current server.", density)
        is MediaSearchOutcome.NoResults -> {
            SearchStatusCard(
                title = "No results",
                body = "No cached media matched “${outcome.query}”. Try a shorter title or alternate spelling, then retry sync if the item should be cached.",
                density = density,
            )
            SearchActionRow(onRetry = onRetry, onClear = onClear, density = density)
        }
        is MediaSearchOutcome.Failure -> {
            val title = when (outcome.kind) {
                SearchFailureKind.NetworkOffline -> "Search is offline"
                SearchFailureKind.Http -> "Search HTTP error"
                SearchFailureKind.Server -> "Server search error"
                SearchFailureKind.InvalidResponse -> "Search response changed"
            }
            SearchStatusCard(title = title, body = outcome.message, tone = FerrexStatusTone.Error, density = density)
            SearchActionRow(
                onRetry = onRetry,
                onClear = onClear,
                retryEnabled = outcome.retryable,
                onOpenDiagnostics = onOpenDiagnostics,
                density = density,
            )
        }
        is MediaSearchOutcome.Results -> {
            if (outcome.staleCache) {
                SearchStatusCard(
                    title = "Cache needs retry",
                    body = "Resolved rows use the current scoped cache. Cache misses stay visible and can be repaired with Retry sync.",
                    tone = FerrexStatusTone.StaleOffline,
                    density = density,
                )
            }
            if (imageLoader == null && outcome.rows.any { it is SearchResultRow.Resolved && it.imageKey != null }) {
                SearchStatusCard(
                    title = "Images unavailable",
                    body = "The image pipeline is unavailable; search keeps poster slots visible with placeholders instead of dropping results.",
                    tone = FerrexStatusTone.StaleOffline,
                    density = density,
                )
            }
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(FerrexQaTags.Phone.SearchResults)
                    .semantics { contentDescription = "Flat search results section with ${outcome.rows.size} row${if (outcome.rows.size == 1) "" else "s"}" },
                verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
            ) {
                TheaterPlateText(
                    text = "Search results",
                    role = TheaterPlateTypographyRole.SectionTitle,
                    densityRole = density.toSearchTypographyDensity(),
                )
                outcome.rows.take(12).forEach { row ->
                    when (row) {
                        is SearchResultRow.Resolved -> ResolvedSearchRow(
                            row = row,
                            resolution = row.imageKey?.let(resolveImage),
                            scope = scope,
                            imageLoader = imageLoader,
                            density = density,
                            onOpenResult = onOpenResult,
                        )
                        is SearchResultRow.CacheMiss -> CacheMissRow(
                            row = row,
                            onRetry = onRetry,
                            onClear = onClear,
                            onOpenDiagnostics = onOpenDiagnostics,
                            density = density,
                        )
                    }
                }
                if (outcome.rows.size > 12) {
                    TheaterPlateText(
                        text = "Showing 12 of ${outcome.rows.size} results. Narrow the query for more focused rows.",
                        role = TheaterPlateTypographyRole.StatusCopy,
                        densityRole = density.toSearchTypographyDensity(),
                        maxLines = 3,
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
    density: FerrexStageDensityFamily,
    onOpenResult: (SearchDetailTarget) -> Unit,
) {
    val typographyDensity = density.toSearchTypographyDensity()
    val resultTag = FerrexQaTags.Phone.searchResult("${row.sourceId.type.routeSegment}-${row.sourceId.id}")
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .testTag(resultTag)
            .semantics { contentDescription = "Search result ${row.title}. ${row.subtitle}. Action: Open." },
        horizontalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
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
            TheaterPlateText(
                text = row.sourceId.type.jsonVariant,
                role = TheaterPlateTypographyRole.Metadata,
                densityRole = typographyDensity,
                color = MaterialTheme.colorScheme.primary,
            )
            TheaterPlateText(
                text = row.title,
                role = TheaterPlateTypographyRole.RailTitle,
                densityRole = typographyDensity,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            TheaterPlateText(
                text = row.subtitle,
                role = TheaterPlateTypographyRole.RailSubtitle,
                densityRole = typographyDensity,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            TheaterPlateText(
                text = "Image ${resolution?.label ?: "queued"}",
                role = TheaterPlateTypographyRole.Metadata,
                densityRole = typographyDensity,
                maxLines = 1,
            )
        }
        FerrexActionButton(
            label = "Open",
            role = FerrexActionRole.Primary,
            onClick = { onOpenResult(row.target) },
            modifier = Modifier.width(92.dp),
            testTag = FerrexQaTags.Phone.searchAction("open-${row.sourceId.type.routeSegment}-${row.sourceId.id}"),
            contentDescription = "Open ${row.title}",
        )
    }
}

@Composable
private fun CacheMissRow(
    row: SearchResultRow.CacheMiss,
    onRetry: () -> Unit,
    onClear: () -> Unit,
    onOpenDiagnostics: (() -> Unit)?,
    density: FerrexStageDensityFamily,
) {
    SearchStatusCard(
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
        density = density,
        testTag = FerrexQaTags.Phone.searchResult("cache-miss-${row.sourceId.type.routeSegment}-${row.sourceId.id}"),
    )
    SearchActionRow(
        onRetry = onRetry,
        onClear = onClear,
        retryEnabled = row.retryable,
        onOpenDiagnostics = onOpenDiagnostics,
        density = density,
    )
}

@Composable
private fun SearchActionRow(
    onRetry: () -> Unit,
    onClear: () -> Unit,
    retryEnabled: Boolean = true,
    onOpenDiagnostics: (() -> Unit)? = null,
    density: FerrexStageDensityFamily,
) {
    val modifier = Modifier
        .fillMaxWidth()
        .testTag(FerrexQaTags.Phone.SearchActions)
    if (density == FerrexStageDensityFamily.Compact) {
        Column(
            modifier = modifier,
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        ) {
            FerrexActionButton(
                label = "Retry sync / search",
                role = FerrexActionRole.Retry,
                enabled = retryEnabled,
                onClick = onRetry,
                modifier = Modifier.fillMaxWidth(),
                testTag = FerrexQaTags.Phone.searchAction("retry"),
            )
            FerrexActionButton(
                label = "Clear search",
                role = FerrexActionRole.Secondary,
                onClick = onClear,
                modifier = Modifier.fillMaxWidth(),
                testTag = FerrexQaTags.Phone.searchAction("clear-results"),
            )
            onOpenDiagnostics?.let {
                FerrexActionButton(
                    label = "Diagnostics",
                    role = FerrexActionRole.Secondary,
                    onClick = it,
                    modifier = Modifier.fillMaxWidth(),
                    testTag = FerrexQaTags.Phone.searchAction("diagnostics"),
                )
            }
        }
    } else {
        Row(
            modifier = modifier,
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        ) {
            FerrexActionButton(
                label = "Retry sync / search",
                role = FerrexActionRole.Retry,
                enabled = retryEnabled,
                onClick = onRetry,
                modifier = Modifier.weight(1f),
                testTag = FerrexQaTags.Phone.searchAction("retry"),
            )
            FerrexActionButton(
                label = "Clear search",
                role = FerrexActionRole.Secondary,
                onClick = onClear,
                modifier = Modifier.weight(1f),
                testTag = FerrexQaTags.Phone.searchAction("clear-results"),
            )
            onOpenDiagnostics?.let {
                FerrexActionButton(
                    label = "Diagnostics",
                    role = FerrexActionRole.Secondary,
                    onClick = it,
                    modifier = Modifier.weight(1f),
                    testTag = FerrexQaTags.Phone.searchAction("diagnostics"),
                )
            }
        }
    }
}

@Composable
private fun SearchLoading(query: String, density: FerrexStageDensityFamily) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) { contentDescription = "Searching $query" },
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CircularProgressIndicator()
        TheaterPlateText(
            text = "Searching “$query”…",
            role = TheaterPlateTypographyRole.StatusCopy,
            densityRole = density.toSearchTypographyDensity(),
        )
    }
}

@Composable
private fun SearchStatusCard(
    title: String,
    body: String,
    density: FerrexStageDensityFamily,
    tone: FerrexStatusTone = FerrexStatusTone.Secondary,
    testTag: String = FerrexQaTags.Phone.searchStatus(title),
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .testTag(testTag)
            .semantics(mergeDescendants = true) { contentDescription = "$title. $body" },
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
    ) {
        TheaterPlateText(
            text = title,
            role = TheaterPlateTypographyRole.StatusTitle,
            densityRole = density.toSearchTypographyDensity(),
            color = tone.searchTitleColor(),
        )
        TheaterPlateText(
            text = body,
            role = TheaterPlateTypographyRole.StatusCopy,
            densityRole = density.toSearchTypographyDensity(),
            maxLines = 4,
        )
    }
}

@Composable
private fun SearchCopy(message: String, density: FerrexStageDensityFamily, error: Boolean = false) {
    TheaterPlateText(
        text = message,
        role = TheaterPlateTypographyRole.StatusCopy,
        modifier = Modifier
            .fillMaxWidth()
            .semantics { contentDescription = message },
        densityRole = density.toSearchTypographyDensity(),
        color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = 3,
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

private fun FerrexStageDensityFamily.toSearchTypographyDensity(): TheaterPlateDensityRole = when (this) {
    FerrexStageDensityFamily.Compact -> TheaterPlateDensityRole.PhonePortrait
    FerrexStageDensityFamily.Standard -> TheaterPlateDensityRole.PhoneLandscape
    FerrexStageDensityFamily.TenFoot -> TheaterPlateDensityRole.Tv1080p
}

@Composable
private fun FerrexStatusTone.searchTitleColor() = when (this) {
    FerrexStatusTone.Error,
    FerrexStatusTone.DestructiveReset -> MaterialTheme.colorScheme.error
    FerrexStatusTone.StaleOffline -> MaterialTheme.colorScheme.tertiary
    else -> MaterialTheme.colorScheme.primary
}

private sealed interface PhoneSearchUiState {
    data object Idle : PhoneSearchUiState
    data object KeepTyping : PhoneSearchUiState
    data object Unavailable : PhoneSearchUiState
    data class Loading(val query: String) : PhoneSearchUiState
    data class Loaded(val outcome: MediaSearchOutcome) : PhoneSearchUiState
}
