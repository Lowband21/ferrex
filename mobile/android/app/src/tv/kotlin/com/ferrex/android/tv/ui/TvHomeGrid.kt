package com.ferrex.android.tv.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import coil.ImageLoader
import com.ferrex.android.core.browse.HomeLibraryTab
import com.ferrex.android.core.browse.IndexedMovieCards
import com.ferrex.android.core.browse.LibraryBrowseModels
import com.ferrex.android.core.browse.LibraryMediaCard
import com.ferrex.android.core.browse.MovieFilterMode
import com.ferrex.android.core.browse.MovieSortMode
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.mediaart.MediaRailIdentityResolver
import com.ferrex.android.core.tvfocus.TvGridFocusPolicy
import com.ferrex.android.tv.ui.foundation.TvActionPanel
import com.ferrex.android.tv.ui.foundation.TvActionPanelAction
import com.ferrex.android.tv.ui.foundation.TvActionRole
import com.ferrex.android.tv.ui.foundation.TvFocusRestorer
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theme.FerrexDesignTokens

@Composable
internal fun TvLibraryGridScreen(
    tab: HomeLibraryTab,
    selectedMovieInfo: LibraryInfo?,
    selectedSeriesInfo: LibraryInfo?,
    movieLibraryInfos: List<LibraryInfo>,
    seriesLibraryInfos: List<LibraryInfo>,
    selectedMovieLibraryId: String?,
    selectedSeriesLibraryId: String?,
    cachedMovieLibraryIds: Set<String>,
    cachedSeriesLibraryIds: Set<String>,
    onSelectedMovieLibrary: (String) -> Unit,
    onSelectedSeriesLibrary: (String) -> Unit,
    onSelectedTab: (HomeLibraryTab) -> Unit,
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    onMovieSort: (MovieSortMode) -> Unit,
    onMovieFilter: (MovieFilterMode) -> Unit,
    movieIndexState: MovieIndexUiState,
    indexedMovieCards: IndexedMovieCards,
    selectedSeriesCards: List<LibraryMediaCard>,
    fullMovieCount: Int,
    fullSeriesCount: Int,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRestorer: TvFocusRestorer,
    onSelect: (LibraryMediaCard) -> Unit,
    onSyncSelected: () -> Unit,
    onRetryAll: () -> Unit,
    onClearSelected: () -> Unit,
    onClearAll: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
    onBack: () -> Unit,
) {
    BackHandler(onBack = onBack)
    val cards = when (tab) {
        HomeLibraryTab.Movies -> indexedMovieCards.cards
        HomeLibraryTab.Series -> selectedSeriesCards
    }
    val selectedLibrary = when (tab) {
        HomeLibraryTab.Movies -> selectedMovieInfo
        HomeLibraryTab.Series -> selectedSeriesInfo
    }
    val fullCachedCount = when (tab) {
        HomeLibraryTab.Movies -> fullMovieCount
        HomeLibraryTab.Series -> fullSeriesCount
    }
    val lastGridTarget = focusRestorer.state.lastTarget(TvGridFocusPolicy.SCREEN_GRID)
    val preferredSurface = TvGridFocusPolicy.preferredSurface(lastGridTarget, hasCards = cards.isNotEmpty())
    TvFullScreenSurface {
        Column(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
        ) {
            TvButtonRow(
                actions = listOf(TvButtonAction("back", "Back to Home", TvActionRole.Back, onSelect = onBack)),
                focusRestorer = focusRestorer,
                surfaceKey = TvGridFocusPolicy.SURFACE_HEADER,
                autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_HEADER,
            )
            Text(
                text = selectedLibrary?.name ?: "${tab.label} library",
                style = MaterialTheme.typography.displaySmall,
                color = MaterialTheme.colorScheme.primary,
                fontWeight = FontWeight.Bold,
            )
            Text(
                text = "Full ${tab.label.lowercase()} grid: ${cards.size} visible item(s), $fullCachedCount cached item(s). No first-10/12/30 cap is applied.",
                style = MaterialTheme.typography.titleMedium,
            )
            Text(
                text = "Image manifest prefetch is bounded to ${cards.size.coerceAtMost(GRID_IMAGE_LOOKUP_LIMIT)} visible grid item(s); every cached item remains in the virtualized grid with stable keys.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = "D-pad focus is contained at the poster-grid edges; Back and recovery rows remain reachable if items disappear.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            TvButtonRow(
                actions = HomeLibraryTab.entries.map { entry ->
                    TvButtonAction(
                        key = "tab-${entry.name.lowercase()}",
                        label = entry.label,
                        role = if (entry == tab) TvActionRole.Primary else TvActionRole.Cache,
                        onSelect = { onSelectedTab(entry) },
                    )
                },
                focusRestorer = focusRestorer,
                surfaceKey = TvGridFocusPolicy.SURFACE_TABS,
                autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_TABS,
            )
            val libraries = when (tab) {
                HomeLibraryTab.Movies -> movieLibraryInfos
                HomeLibraryTab.Series -> seriesLibraryInfos
            }
            val selectedId = when (tab) {
                HomeLibraryTab.Movies -> selectedMovieLibraryId
                HomeLibraryTab.Series -> selectedSeriesLibraryId
            }
            val cachedIds = when (tab) {
                HomeLibraryTab.Movies -> cachedMovieLibraryIds
                HomeLibraryTab.Series -> cachedSeriesLibraryIds
            }
            val onSelectedLibrary = when (tab) {
                HomeLibraryTab.Movies -> onSelectedMovieLibrary
                HomeLibraryTab.Series -> onSelectedSeriesLibrary
            }
            if (libraries.isNotEmpty()) {
                TvButtonRow(
                    title = "Library chooser",
                    actions = libraries.map { library ->
                        TvButtonAction(
                            key = library.id,
                            label = if (library.id in cachedIds) library.name else "${library.name} (not cached)",
                            role = if (library.id == selectedId) TvActionRole.Primary else TvActionRole.Cache,
                            onSelect = { onSelectedLibrary(library.id) },
                        )
                    },
                    focusRestorer = focusRestorer,
                    surfaceKey = TvGridFocusPolicy.SURFACE_LIBRARY_CHOOSER,
                    autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_LIBRARY_CHOOSER,
                )
            }
            when (tab) {
                HomeLibraryTab.Movies -> TvMovieGridControls(
                    movieSort = movieSort,
                    movieFilter = movieFilter,
                    onMovieSort = onMovieSort,
                    onMovieFilter = onMovieFilter,
                    movieIndexState = movieIndexState,
                    fullCachedCount = fullMovieCount,
                    invalidIndexCount = indexedMovieCards.invalidIndexCount,
                    appendedMissingCount = indexedMovieCards.appendedMissingCount,
                    focusRestorer = focusRestorer,
                    preferredSurface = preferredSurface,
                )
                HomeLibraryTab.Series -> TvStateCopy(
                    title = "Series controls disabled",
                    body = LibraryBrowseModels.unsupportedSeriesControlsCopy(),
                )
            }
            TvButtonRow(
                actions = listOf(
                    TvButtonAction("sync-selected", "Retry selected library", TvActionRole.Retry, onSelect = onSyncSelected),
                    TvButtonAction("retry-all", "Retry all libraries", TvActionRole.Retry, onSelect = onRetryAll),
                    TvButtonAction("clear-selected-cache", "Clear selected cache", TvActionRole.Cache, enabled = selectedId != null, onSelect = onClearSelected),
                    TvButtonAction("clear-all-cache", "Clear all cache", TvActionRole.Destructive, onSelect = onClearAll),
                    TvButtonAction("change-server", "Change server", TvActionRole.SettingsExit, onSelect = onChangeServer),
                    TvButtonAction("reset-connection", "Reset connection", TvActionRole.Destructive, onSelect = onResetConnection),
                    TvButtonAction("diagnostics", "Diagnostics / Export diagnostics", TvActionRole.SettingsExit, onSelect = onOpenDiagnostics),
                ),
                focusRestorer = focusRestorer,
                surfaceKey = TvGridFocusPolicy.SURFACE_RECOVERY_ACTIONS,
                autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_RECOVERY_ACTIONS,
            )
            if (cards.isEmpty()) {
                TvActionPanel(
                    title = "No cached ${tab.label.lowercase()} for this library",
                    supportingText = "Retry selected library to fetch complete cached payloads. Empty, stale, corrupt, and offline states stay recoverable here.",
                    actions = listOf(
                        TvActionPanelAction("sync-selected-empty", "Retry selected library", TvActionRole.Retry, onSelect = onSyncSelected),
                        TvActionPanelAction("change-server-empty", "Change server", TvActionRole.SettingsExit, onSelect = onChangeServer),
                        TvActionPanelAction("reset-connection-empty", "Reset connection", TvActionRole.Destructive, onSelect = onResetConnection),
                        TvActionPanelAction("diagnostics-empty", "Diagnostics / Export diagnostics", TvActionRole.SettingsExit, onSelect = onOpenDiagnostics),
                    ),
                    focusRestorer = focusRestorer,
                    surfaceKey = TvGridFocusPolicy.SURFACE_EMPTY_ACTIONS,
                    autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_EMPTY_ACTIONS,
                )
            } else {
                TvPosterGrid(
                    cards = cards,
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    scope = scope,
                    focusRestorer = focusRestorer,
                    autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_CARDS,
                    onSelect = onSelect,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
internal fun TvMovieGridControls(
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    onMovieSort: (MovieSortMode) -> Unit,
    onMovieFilter: (MovieFilterMode) -> Unit,
    movieIndexState: MovieIndexUiState,
    fullCachedCount: Int,
    invalidIndexCount: Int,
    appendedMissingCount: Int,
    focusRestorer: TvFocusRestorer,
    preferredSurface: String,
) {
    Text(
        text = "Movie sort uses /api/v1/libraries/{id}/indices/sorted with paging; filters use /indices/filter.",
        style = MaterialTheme.typography.bodyLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    TvButtonRow(
        title = "Sort movies",
        actions = MovieSortMode.entries.map { mode ->
            TvButtonAction(
                key = "sort-${mode.name}",
                label = mode.label,
                role = if (mode == movieSort) TvActionRole.Primary else TvActionRole.Cache,
                onSelect = { onMovieSort(mode) },
            )
        },
        focusRestorer = focusRestorer,
        surfaceKey = TvGridFocusPolicy.SURFACE_MOVIE_SORT,
        autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_MOVIE_SORT,
    )
    TvButtonRow(
        title = "Filter movies",
        actions = MovieFilterMode.entries.map { mode ->
            TvButtonAction(
                key = "filter-${mode.name}",
                label = mode.label,
                role = if (mode == movieFilter) TvActionRole.Primary else TvActionRole.Cache,
                onSelect = { onMovieFilter(mode) },
            )
        },
        focusRestorer = focusRestorer,
        surfaceKey = TvGridFocusPolicy.SURFACE_MOVIE_FILTER,
        autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_MOVIE_FILTER,
    )
    val copy = when (movieIndexState) {
        MovieIndexUiState.Idle -> "Movie index idle. Showing cached batch order."
        MovieIndexUiState.Loading -> "Loading movie indices…"
        is MovieIndexUiState.Applied -> if (movieIndexState.filterMode == MovieFilterMode.All) {
            "Endpoint sorted ${movieIndexState.indices.size} index value(s) for $fullCachedCount cached movie(s) with ${movieIndexState.sortMode.label}."
        } else {
            "Endpoint filter ${movieIndexState.filterMode.label} returned ${movieIndexState.indices.size} index value(s) for $fullCachedCount cached movie(s)."
        }
        is MovieIndexUiState.Error -> "Movie index request failed: ${movieIndexState.message}. Showing uncapped cached order."
        is MovieIndexUiState.Unsupported -> "Unsupported movie index request: ${movieIndexState.message}. Showing uncapped cached order."
        is MovieIndexUiState.Unavailable -> movieIndexState.message
    }
    Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md), verticalAlignment = Alignment.CenterVertically) {
        if (movieIndexState == MovieIndexUiState.Loading) {
            CircularProgressIndicator(
                modifier = Modifier.size(FerrexDesignTokens.Space.Xxl),
                strokeWidth = FerrexDesignTokens.Focus.TvRestingBorder,
            )
        }
        Text(copy, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.primary)
    }
    if (invalidIndexCount > 0 || appendedMissingCount > 0) {
        Text(
            text = "Index reconciliation: $invalidIndexCount invalid index value(s), $appendedMissingCount cached item(s) appended to avoid a silent cap.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.primary,
        )
    }
}

@Composable
internal fun TvPosterGrid(
    cards: List<LibraryMediaCard>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onSelect: (LibraryMediaCard) -> Unit,
    modifier: Modifier = Modifier,
) {
    val gridItems = remember(cards) {
        cards.zip(
            MediaRailIdentityResolver.assign(
                railKey = TvGridFocusPolicy.SURFACE_CARDS,
                stableIds = cards.map { it.stableKey },
            ),
        )
    }
    val keys = gridItems.map { it.second.renderKey }
    val requesters = remember(keys) { keys.associateWith { FocusRequester() } }
    val restoredKey = keys.firstOrNull()?.let { fallback ->
        focusRestorer.restoreItem(TvGridFocusPolicy.SURFACE_CARDS, keys, fallback)
    }
    LaunchedEffect(autoFocus, restoredKey, keys) {
        if (autoFocus && restoredKey != null) {
            runCatching { requesters[restoredKey]?.requestFocus() }
        }
    }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = FerrexDesignTokens.Poster.TvGridMin),
        modifier = modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Tv.surface(TvGridFocusPolicy.SURFACE_CARDS)),
        contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Md),
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xxl),
    ) {
        items(gridItems, key = { it.second.focusKey }) { (card, identity) ->
            val itemKey = identity.renderKey
            TvPosterCard(
                entry = card.toPosterEntry(),
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
                focusRequester = requesters[itemKey],
                semanticLabel = identity.semanticLabel(card.title),
                onFocused = { focusRestorer.record(TvGridFocusPolicy.SURFACE_CARDS, itemKey) },
                onSelect = { onSelect(card) },
                modifier = Modifier.fillMaxWidth(),
                testTag = FerrexQaTags.Tv.poster(TvGridFocusPolicy.SURFACE_CARDS, itemKey),
            )
        }
    }
}
