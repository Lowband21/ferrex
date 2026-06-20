package com.ferrex.android.tv.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import coil.ImageLoader
import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.search.MediaSearchOutcome
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.SearchDetailTarget
import com.ferrex.android.core.search.SearchFailureKind
import com.ferrex.android.core.search.SearchMediaType
import com.ferrex.android.core.search.SearchResultRow
import com.ferrex.android.core.tvfocus.TvSearchFocusPolicy
import com.ferrex.android.tv.ui.foundation.TvActionPanel
import com.ferrex.android.tv.ui.foundation.TvActionPanelAction
import com.ferrex.android.tv.ui.foundation.TvActionRole
import com.ferrex.android.tv.ui.foundation.TvFocusableSurface
import com.ferrex.android.tv.ui.foundation.TvFocusRestorer
import com.ferrex.android.ui.components.FerrexAsyncImage
import com.ferrex.android.ui.components.rememberVisibleImageResolutionState
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theme.FerrexDesignTokens
import kotlinx.coroutines.delay

@Composable
internal fun TvSearchScreen(
    scope: ServerCacheScope,
    searchRepository: MediaSearchRepository?,
    imageRepository: ImageRepository?,
    imagePipeline: FerrexImagePipeline?,
    query: String,
    onQueryChange: (String) -> Unit,
    focusRestorer: TvFocusRestorer,
    onOpenResult: (SearchDetailTarget) -> Unit,
    onBack: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    BackHandler(onBack = onBack)
    var retryNonce by remember(scope.directoryName) { mutableStateOf(0) }
    var uiState by remember(scope.directoryName) { mutableStateOf<TvSearchUiState>(TvSearchUiState.Idle) }
    val focusRequester = remember { FocusRequester() }
    val lastSearchTarget = focusRestorer.state.lastTarget(focusRestorer.screen)
    val preferredSurface = lastSearchTarget?.surface ?: TvSearchFocusPolicy.SURFACE_FIELD

    LaunchedEffect(preferredSurface) {
        if (preferredSurface == TvSearchFocusPolicy.SURFACE_FIELD) {
            runCatching { focusRequester.requestFocus() }
        }
    }
    LaunchedEffect(searchRepository, scope.directoryName, query, retryNonce) {
        val trimmed = query.trim()
        if (searchRepository == null) {
            uiState = TvSearchUiState.Unavailable
            return@LaunchedEffect
        }
        if (trimmed.isEmpty()) {
            uiState = TvSearchUiState.Idle
            return@LaunchedEffect
        }
        if (trimmed.length < 2) {
            uiState = TvSearchUiState.KeepTyping
            return@LaunchedEffect
        }
        delay(SEARCH_DEBOUNCE_MILLIS)
        uiState = TvSearchUiState.Loading(trimmed)
        uiState = TvSearchUiState.Loaded(searchRepository.search(scope, trimmed))
    }

    val rows = ((uiState as? TvSearchUiState.Loaded)?.outcome as? MediaSearchOutcome.Results)?.rows.orEmpty()
    val visibleKeys = remember(rows) {
        rows.filterIsInstance<SearchResultRow.Resolved>().mapNotNull { it.imageKey }.distinctBy { it.cacheKey }.take(SEARCH_IMAGE_LOOKUP_LIMIT)
    }
    val imageLoader = remember(imagePipeline, scope.directoryName) { imagePipeline?.imageLoader(scope) }
    val resolutions = rememberVisibleImageResolutionState(
        scope = scope,
        imageRepository = imageRepository,
        visibleKeys = visibleKeys,
    ).resolutions

    TvFullScreenSurface {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .testTag(FerrexQaTags.Tv.Search),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
        ) {
            Text("Search", style = MaterialTheme.typography.displaySmall, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
            Text(
                text = "Type at least two characters. Cache misses stay visible with retry actions.",
                style = MaterialTheme.typography.titleMedium,
            )
            OutlinedTextField(
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(FerrexQaTags.Tv.SearchField)
                    .focusRequester(focusRequester)
                    .onFocusChanged {
                        if (it.isFocused) {
                            focusRestorer.record(TvSearchFocusPolicy.SURFACE_FIELD, TvSearchFocusPolicy.ITEM_QUERY)
                        }
                    }
                    .semantics { contentDescription = "Search movies, shows, seasons, and episodes" },
                value = query,
                onValueChange = onQueryChange,
                label = { Text("Movies, shows, seasons, episodes…") },
                singleLine = true,
                enabled = searchRepository != null,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { retryNonce += 1 }),
            )
            TvButtonRow(
                actions = listOf(
                    TvButtonAction("back", "Back to Home", TvActionRole.Back, onSelect = onBack),
                    TvButtonAction("retry", "Retry search", TvActionRole.Retry, enabled = query.trim().length >= 2, onSelect = { retryNonce += 1 }),
                    TvButtonAction("clear", "Clear search", TvActionRole.Cache, enabled = query.isNotEmpty(), onSelect = {
                        onQueryChange("")
                        uiState = TvSearchUiState.Idle
                        focusRestorer.record(TvSearchFocusPolicy.SURFACE_FIELD, TvSearchFocusPolicy.ITEM_QUERY)
                        runCatching { focusRequester.requestFocus() }
                    }),
                ),
                focusRestorer = focusRestorer,
                surfaceKey = TvSearchFocusPolicy.SURFACE_ACTIONS,
                autoFocus = preferredSurface == TvSearchFocusPolicy.SURFACE_ACTIONS,
            )
            when (val state = uiState) {
                TvSearchUiState.Idle -> TvStateCopy("Ready to search", "Enter at least two characters to search the current server.")
                TvSearchUiState.KeepTyping -> TvStateCopy("Keep typing", "Search begins after two characters.")
                TvSearchUiState.Unavailable -> TvStateCopy("Search unavailable", "The protected search dependency is not configured for this TV build.")
                is TvSearchUiState.Loading -> TvStateCopy("Searching “${state.query}”…", "Results remain scoped to this server and user.", loading = true)
                is TvSearchUiState.Loaded -> TvSearchOutcome(
                    outcome = state.outcome,
                    imageLoader = imageLoader,
                    scope = scope,
                    rows = rows,
                    resolutions = resolutions,
                    onOpenResult = onOpenResult,
                    onRetry = { retryNonce += 1 },
                    focusRestorer = focusRestorer,
                    preferredSurface = preferredSurface,
                    onOpenDiagnostics = onOpenDiagnostics,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
internal fun TvSearchOutcome(
    outcome: MediaSearchOutcome,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    rows: List<SearchResultRow>,
    resolutions: Map<ImageRequestKey, ImageResolution>,
    onOpenResult: (SearchDetailTarget) -> Unit,
    onRetry: () -> Unit,
    focusRestorer: TvFocusRestorer,
    preferredSurface: String,
    onOpenDiagnostics: () -> Unit,
    modifier: Modifier = Modifier,
) {
    when (outcome) {
        MediaSearchOutcome.Idle -> TvStateCopy("Ready to search", "Enter at least two characters to search the current server.")
        is MediaSearchOutcome.NoResults -> TvActionPanel(
            title = "No results for “${outcome.query}”",
            supportingText = "Try a shorter title or alternate spelling.",
            actions = listOf(TvActionPanelAction("retry", "Retry", TvActionRole.Retry, onSelect = onRetry)),
            focusRestorer = focusRestorer,
            surfaceKey = TvSearchFocusPolicy.SURFACE_RESULTS_RECOVERY,
            autoFocus = TvSearchFocusPolicy.shouldAutoFocusRecovery(preferredSurface),
        )
        is MediaSearchOutcome.Failure -> {
            val prefix = when (outcome.kind) {
                SearchFailureKind.NetworkOffline -> "Search is offline. "
                SearchFailureKind.Http -> "Search HTTP error. "
                SearchFailureKind.Server -> "Server search error. "
                SearchFailureKind.InvalidResponse -> "Search response changed. "
            }
            TvActionPanel(
                title = "Search failed",
                supportingText = prefix + outcome.message,
                actions = listOf(
                    TvActionPanelAction("retry", "Retry", TvActionRole.Retry, enabled = outcome.retryable, onSelect = onRetry),
                    TvActionPanelAction("diagnostics", "Diagnostics / Export diagnostics", TvActionRole.SettingsExit, onSelect = onOpenDiagnostics),
                ),
                focusRestorer = focusRestorer,
                surfaceKey = TvSearchFocusPolicy.SURFACE_RESULTS_RECOVERY,
                autoFocus = TvSearchFocusPolicy.shouldAutoFocusRecovery(preferredSurface),
            )
        }
        is MediaSearchOutcome.Results -> {
            if (outcome.staleCache) {
                Text(
                    text = "Cache is stale or retryable; resolved rows use the current scoped cache and misses can be repaired with Retry sync.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            if (rows.size > SEARCH_RESULT_DISPLAY_LIMIT) {
                Text(
                    text = "Showing ${SEARCH_RESULT_DISPLAY_LIMIT} of ${rows.size} search result(s). Narrow the query for more focused rows.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            val visibleRows = rows.take(SEARCH_RESULT_DISPLAY_LIMIT)
            val rowKeys = visibleRows.map { it.searchStableKey() }
            val rowRequesters = remember(rowKeys) { rowKeys.associateWith { FocusRequester() } }
            val restoredRowKey = rowKeys.firstOrNull()?.let { fallback ->
                focusRestorer.restoreItem(TvSearchFocusPolicy.SURFACE_RESULTS, rowKeys, fallback)
            }
            LaunchedEffect(preferredSurface, restoredRowKey, rowKeys) {
                if (preferredSurface == TvSearchFocusPolicy.SURFACE_RESULTS && restoredRowKey != null) {
                    runCatching { rowRequesters[restoredRowKey]?.requestFocus() }
                }
            }
            LazyColumn(
                modifier = modifier
                    .fillMaxWidth()
                    .testTag(FerrexQaTags.Tv.SearchResults),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
                contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Sm),
            ) {
                items(visibleRows, key = { it.searchStableKey() }) { row ->
                    val rowKey = row.searchStableKey()
                    when (row) {
                        is SearchResultRow.Resolved -> TvSearchResolvedRow(
                            row = row,
                            resolution = row.imageKey?.let { resolutions[it] },
                            imageLoader = imageLoader,
                            scope = scope,
                            focusRequester = rowRequesters[rowKey],
                            onFocused = { focusRestorer.record(TvSearchFocusPolicy.SURFACE_RESULTS, rowKey) },
                            onOpenResult = onOpenResult,
                        )
                        is SearchResultRow.CacheMiss -> TvSearchCacheMissRow(
                            row = row,
                            focusRestorer = focusRestorer,
                            autoFocus = preferredSurface == TvSearchFocusPolicy.cacheMissSurface(rowKey),
                            onRetry = onRetry,
                            onOpenDiagnostics = onOpenDiagnostics,
                        )
                    }
                }
            }
        }
    }
}

@Composable
internal fun TvSearchResolvedRow(
    row: SearchResultRow.Resolved,
    resolution: ImageResolution?,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRequester: FocusRequester?,
    onFocused: () -> Unit,
    onOpenResult: (SearchDetailTarget) -> Unit,
) {
    TvFocusableSurface(
        onClick = { onOpenResult(row.target) },
        semanticLabel = "Open ${row.title}",
        minHeight = FerrexDesignTokens.Tv.SearchResultMinHeight,
        testTag = FerrexQaTags.Tv.action(TvSearchFocusPolicy.SURFACE_RESULTS, row.searchStableKey()),
        modifier = Modifier.fillMaxWidth(),
        focusRequester = focusRequester,
        onFocused = onFocused,
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg), verticalAlignment = Alignment.CenterVertically) {
            Box(modifier = Modifier.width(FerrexDesignTokens.Tv.SearchThumbnailWidth)) {
                SearchResultImage(row, resolution, imageLoader, scope)
            }
            Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
                Text(row.title, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text("${row.subtitle} • image ${resolution?.label ?: "queued"}", style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Text("Open", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
internal fun TvSearchCacheMissRow(
    row: SearchResultRow.CacheMiss,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onRetry: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val rowKey = row.searchStableKey()
    val surfaceKey = TvSearchFocusPolicy.cacheMissSurface(rowKey)
    TvActionPanel(
        title = row.title,
        supportingText = row.message,
        actions = listOf(
            TvActionPanelAction(TvSearchFocusPolicy.cacheMissRetryAction(rowKey), "Retry sync / search", TvActionRole.Retry, enabled = row.retryable, onSelect = onRetry),
            TvActionPanelAction(TvSearchFocusPolicy.cacheMissDiagnosticsAction(rowKey), "Diagnostics / Export diagnostics", TvActionRole.SettingsExit, onSelect = onOpenDiagnostics),
        ),
        focusRestorer = focusRestorer,
        surfaceKey = surfaceKey,
        autoFocus = autoFocus,
        buttonMaxWidth = FerrexDesignTokens.Tv.PlayerActionMaxWidth,
    )
}

@Composable
internal fun SearchResultImage(
    row: SearchResultRow.Resolved,
    resolution: ImageResolution?,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
) {
    val imageKey = row.imageKey
    if (imageLoader == null || imageKey == null) {
        PosterPlaceholder(if (imageKey == null) "No image" else "Images unavailable")
        return
    }
    FerrexAsyncImage(
        resolution = resolution,
        imageLoader = imageLoader,
        contentDescription = row.title,
        category = imageKey.category,
        fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
            runtimeFallback(scope.canonicalServerUrl, imageKey, row.publicFallbackPath)
        } else {
            null
        },
    )
}

internal fun SearchDetailTarget.toMediaRouteArgs(): MediaRouteArgs? {
    val browseType = when (mediaType) {
        SearchMediaType.Movie -> BrowseMediaType.Movie
        SearchMediaType.Series -> BrowseMediaType.Series
        SearchMediaType.Episode -> BrowseMediaType.Episode
        SearchMediaType.Season -> return null
    }
    return MediaRouteArgs(
        mediaType = browseType,
        mediaId = mediaId,
        libraryId = libraryId,
        sourceSurface = BrowseSourceSurface.Search,
    )
}

internal fun SearchResultRow.searchStableKey(): String = when (this) {
    is SearchResultRow.Resolved -> TvSearchFocusPolicy.resolvedRowKey(sourceId.type.routeSegment, sourceId.id, libraryId)
    is SearchResultRow.CacheMiss -> TvSearchFocusPolicy.cacheMissRowKey(sourceId.type.routeSegment, sourceId.id)
}

internal sealed interface TvSearchUiState {
    data object Idle : TvSearchUiState
    data object KeepTyping : TvSearchUiState
    data object Unavailable : TvSearchUiState
    data class Loading(val query: String) : TvSearchUiState
    data class Loaded(val outcome: MediaSearchOutcome) : TvSearchUiState
}

internal const val SEARCH_IMAGE_LOOKUP_LIMIT = 48
internal const val SEARCH_RESULT_DISPLAY_LIMIT = 20
internal const val SEARCH_DEBOUNCE_MILLIS = 350L
