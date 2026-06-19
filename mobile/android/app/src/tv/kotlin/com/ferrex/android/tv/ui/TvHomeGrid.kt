package com.ferrex.android.tv.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.testTag
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

private enum class TvGridControlPanel(val surfaceKey: String) {
    MediaType(TvGridFocusPolicy.SURFACE_MEDIA_TYPE_PANEL),
    Library(TvGridFocusPolicy.SURFACE_LIBRARY_PANEL),
    MovieControls(TvGridFocusPolicy.SURFACE_MOVIE_CONTROLS_PANEL),
    StatusMore(TvGridFocusPolicy.SURFACE_STATUS_PANEL),
}

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
    var openPanel by remember { mutableStateOf<TvGridControlPanel?>(null) }
    BackHandler {
        if (openPanel != null) {
            openPanel = null
        } else {
            onBack()
        }
    }

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
    val preferredSurface = TvGridFocusPolicy.preferredSurface(
        lastTarget = lastGridTarget,
        hasCards = cards.isNotEmpty(),
        openPanelSurface = openPanel?.surfaceKey,
    )

    TvFullScreenSurface {
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

        Box(modifier = Modifier.fillMaxSize()) {
            Column(
                modifier = Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.DenseLibraryGrid.tvTopControlGridGap),
            ) {
                TvButtonRow(
                    actions = tvGridTopActions(
                        tab = tab,
                        selectedLibrary = selectedLibrary,
                        libraries = libraries,
                        cards = cards,
                        fullCachedCount = fullCachedCount,
                        movieSort = movieSort,
                        movieFilter = movieFilter,
                        onBack = onBack,
                        onOpenPanel = { openPanel = it },
                    ),
                    focusRestorer = focusRestorer,
                    surfaceKey = TvGridFocusPolicy.SURFACE_TOP_CONTROLS,
                    autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_TOP_CONTROLS,
                )
                if (cards.isEmpty()) {
                    TvGridEmptyState(
                        tab = tab,
                        selectedId = selectedId,
                        focusRestorer = focusRestorer,
                        autoFocus = preferredSurface == TvGridFocusPolicy.SURFACE_EMPTY_ACTIONS,
                        onSyncSelected = onSyncSelected,
                        onRetryAll = onRetryAll,
                        onClearSelected = onClearSelected,
                        onClearAll = onClearAll,
                        onChangeServer = onChangeServer,
                        onResetConnection = onResetConnection,
                        onOpenDiagnostics = onOpenDiagnostics,
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f),
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
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f),
                    )
                }
            }

            openPanel?.let { panel ->
                TvGridControlPanelOverlay(
                    panel = panel,
                    tab = tab,
                    libraries = libraries,
                    selectedId = selectedId,
                    cachedIds = cachedIds,
                    selectedLibrary = selectedLibrary,
                    cards = cards,
                    fullCachedCount = fullCachedCount,
                    movieSort = movieSort,
                    movieFilter = movieFilter,
                    movieIndexState = movieIndexState,
                    invalidIndexCount = indexedMovieCards.invalidIndexCount,
                    appendedMissingCount = indexedMovieCards.appendedMissingCount,
                    focusRestorer = focusRestorer,
                    autoFocus = preferredSurface == panel.surfaceKey,
                    onSelectedTab = { selectedTab ->
                        onSelectedTab(selectedTab)
                        openPanel = null
                    },
                    onSelectedLibrary = { libraryId ->
                        onSelectedLibrary(libraryId)
                        openPanel = null
                    },
                    onMovieSort = onMovieSort,
                    onMovieFilter = onMovieFilter,
                    onSyncSelected = onSyncSelected,
                    onRetryAll = onRetryAll,
                    onClearSelected = onClearSelected,
                    onClearAll = onClearAll,
                    onChangeServer = onChangeServer,
                    onResetConnection = onResetConnection,
                    onOpenDiagnostics = onOpenDiagnostics,
                    onClose = { openPanel = null },
                )
            }
        }
    }
}

private fun tvGridTopActions(
    tab: HomeLibraryTab,
    selectedLibrary: LibraryInfo?,
    libraries: List<LibraryInfo>,
    cards: List<LibraryMediaCard>,
    fullCachedCount: Int,
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    onBack: () -> Unit,
    onOpenPanel: (TvGridControlPanel) -> Unit,
): List<TvButtonAction> = buildList {
    add(TvButtonAction("back", "Back", TvActionRole.Back, onSelect = onBack))
    add(
        TvButtonAction(
            key = "media-type",
            label = "Media: ${tab.label}",
            role = TvActionRole.Primary,
            onSelect = { onOpenPanel(TvGridControlPanel.MediaType) },
        ),
    )
    add(
        TvButtonAction(
            key = "library",
            label = selectedLibrary?.name?.let { "Library: $it" } ?: "Choose library",
            role = if (selectedLibrary != null) TvActionRole.Primary else TvActionRole.Cache,
            enabled = libraries.isNotEmpty(),
            onSelect = { onOpenPanel(TvGridControlPanel.Library) },
        ),
    )
    if (tab == HomeLibraryTab.Movies) {
        add(
            TvButtonAction(
                key = "sort-filter",
                label = "Sort/filter: ${movieSort.label} • ${movieFilter.label}",
                role = TvActionRole.Cache,
                onSelect = { onOpenPanel(TvGridControlPanel.MovieControls) },
            ),
        )
    }
    add(
        TvButtonAction(
            key = "status-more",
            label = "${cards.size}/$fullCachedCount visible • Status / More",
            role = if (cards.isEmpty()) TvActionRole.Retry else TvActionRole.Cache,
            onSelect = { onOpenPanel(TvGridControlPanel.StatusMore) },
        ),
    )
}

@Composable
private fun TvGridEmptyState(
    tab: HomeLibraryTab,
    selectedId: String?,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onSyncSelected: () -> Unit,
    onRetryAll: () -> Unit,
    onClearSelected: () -> Unit,
    onClearAll: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
    modifier: Modifier = Modifier,
) {
    TvActionPanel(
        modifier = modifier,
        title = "No cached ${tab.label.lowercase()} for this library",
        supportingText = "Retry selected or all libraries to fetch complete cached payloads. Empty, stale, corrupt, and offline states stay recoverable without clearing app data.",
        actions = gridRecoveryActions(
            selectedId = selectedId,
            onSyncSelected = onSyncSelected,
            onRetryAll = onRetryAll,
            onClearSelected = onClearSelected,
            onClearAll = onClearAll,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
            onOpenDiagnostics = onOpenDiagnostics,
        ),
        focusRestorer = focusRestorer,
        surfaceKey = TvGridFocusPolicy.SURFACE_EMPTY_ACTIONS,
        autoFocus = autoFocus,
        buttonMaxWidth = FerrexDesignTokens.DenseLibraryGrid.tvControlPanelMaxWidth,
    )
}

@Composable
private fun TvGridControlPanelOverlay(
    panel: TvGridControlPanel,
    tab: HomeLibraryTab,
    libraries: List<LibraryInfo>,
    selectedId: String?,
    cachedIds: Set<String>,
    selectedLibrary: LibraryInfo?,
    cards: List<LibraryMediaCard>,
    fullCachedCount: Int,
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    movieIndexState: MovieIndexUiState,
    invalidIndexCount: Int,
    appendedMissingCount: Int,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onSelectedTab: (HomeLibraryTab) -> Unit,
    onSelectedLibrary: (String) -> Unit,
    onMovieSort: (MovieSortMode) -> Unit,
    onMovieFilter: (MovieFilterMode) -> Unit,
    onSyncSelected: () -> Unit,
    onRetryAll: () -> Unit,
    onClearSelected: () -> Unit,
    onClearAll: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
    onClose: () -> Unit,
) {
    val closeAction = TvActionPanelAction("close", "Close panel", TvActionRole.Back, onSelect = onClose)
    val title: String
    val supportingText: String
    val actions: List<TvActionPanelAction>
    when (panel) {
        TvGridControlPanel.MediaType -> {
            title = "Choose media type"
            supportingText = "Switch between movie and series libraries without leaving the full-library grid."
            actions = HomeLibraryTab.entries.map { entry ->
                TvActionPanelAction(
                    key = "tab-${entry.name.lowercase()}",
                    label = entry.label,
                    role = if (entry == tab) TvActionRole.Primary else TvActionRole.Cache,
                    onSelect = { onSelectedTab(entry) },
                )
            } + closeAction
        }
        TvGridControlPanel.Library -> {
            title = "Choose ${tab.label.lowercase()} library"
            supportingText = "Cached libraries open immediately; uncached libraries remain selectable so Retry selected can fetch them."
            actions = libraries.map { library ->
                val cached = library.id in cachedIds
                TvActionPanelAction(
                    key = library.id,
                    label = if (cached) library.name else "${library.name} (not cached)",
                    role = if (library.id == selectedId) TvActionRole.Primary else TvActionRole.Cache,
                    onSelect = { onSelectedLibrary(library.id) },
                )
            } + closeAction
        }
        TvGridControlPanel.MovieControls -> {
            title = "Sort and filter movies"
            supportingText = movieControlsSupportingText(
                movieIndexState = movieIndexState,
                fullCachedCount = fullCachedCount,
                invalidIndexCount = invalidIndexCount,
                appendedMissingCount = appendedMissingCount,
            )
            actions = movieControlActions(
                movieSort = movieSort,
                movieFilter = movieFilter,
                onMovieSort = onMovieSort,
                onMovieFilter = onMovieFilter,
            ) + closeAction
        }
        TvGridControlPanel.StatusMore -> {
            title = "Grid status and recovery"
            supportingText = gridStatusSupportingText(
                tab = tab,
                selectedLibrary = selectedLibrary,
                visibleCount = cards.size,
                fullCachedCount = fullCachedCount,
                movieIndexState = movieIndexState,
                invalidIndexCount = invalidIndexCount,
                appendedMissingCount = appendedMissingCount,
            )
            actions = gridRecoveryActions(
                selectedId = selectedId,
                onSyncSelected = onSyncSelected,
                onRetryAll = onRetryAll,
                onClearSelected = onClearSelected,
                onClearAll = onClearAll,
                onChangeServer = onChangeServer,
                onResetConnection = onResetConnection,
                onOpenDiagnostics = onOpenDiagnostics,
            ) + closeAction
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(FerrexDesignTokens.Palette.SlateBlack.copy(alpha = 0.72f))
            .padding(FerrexDesignTokens.Space.Xxl),
        contentAlignment = Alignment.CenterEnd,
    ) {
        Box(
            modifier = Modifier
                .fillMaxHeight()
                .widthIn(max = FerrexDesignTokens.DenseLibraryGrid.tvControlPanelMaxWidth)
                .fillMaxWidth()
                .verticalScroll(rememberScrollState()),
            contentAlignment = Alignment.Center,
        ) {
            TvActionPanel(
                title = title,
                supportingText = supportingText,
                actions = actions,
                focusRestorer = focusRestorer,
                surfaceKey = panel.surfaceKey,
                autoFocus = autoFocus,
                buttonMaxWidth = FerrexDesignTokens.DenseLibraryGrid.tvControlPanelMaxWidth,
            )
        }
    }
}

private fun movieControlActions(
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    onMovieSort: (MovieSortMode) -> Unit,
    onMovieFilter: (MovieFilterMode) -> Unit,
): List<TvActionPanelAction> = buildList {
    MovieSortMode.entries.forEach { mode ->
        add(
            TvActionPanelAction(
                key = "sort-${mode.name.lowercase()}",
                label = "Sort: ${mode.label}",
                role = if (mode == movieSort) TvActionRole.Primary else TvActionRole.Cache,
                onSelect = { onMovieSort(mode) },
            ),
        )
    }
    MovieFilterMode.entries.forEach { mode ->
        add(
            TvActionPanelAction(
                key = "filter-${mode.name.lowercase()}",
                label = "Filter: ${mode.label}",
                role = if (mode == movieFilter) TvActionRole.Primary else TvActionRole.Cache,
                onSelect = { onMovieFilter(mode) },
            ),
        )
    }
}

private fun gridRecoveryActions(
    selectedId: String?,
    onSyncSelected: () -> Unit,
    onRetryAll: () -> Unit,
    onClearSelected: () -> Unit,
    onClearAll: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
): List<TvActionPanelAction> = listOf(
    TvActionPanelAction("sync-selected", "Retry selected library", TvActionRole.Retry, onSelect = onSyncSelected),
    TvActionPanelAction("retry-all", "Retry all libraries", TvActionRole.Retry, onSelect = onRetryAll),
    TvActionPanelAction(
        key = "clear-selected-cache",
        label = "Clear selected cache",
        role = TvActionRole.Cache,
        enabled = selectedId != null,
        onSelect = onClearSelected,
    ),
    TvActionPanelAction("clear-all-cache", "Clear all cache", TvActionRole.Destructive, onSelect = onClearAll),
    TvActionPanelAction("change-server", "Change server", TvActionRole.SettingsExit, onSelect = onChangeServer),
    TvActionPanelAction("reset-connection", "Reset connection", TvActionRole.Destructive, onSelect = onResetConnection),
    TvActionPanelAction("diagnostics", "Diagnostics / Export diagnostics", TvActionRole.SettingsExit, onSelect = onOpenDiagnostics),
)

private fun gridStatusSupportingText(
    tab: HomeLibraryTab,
    selectedLibrary: LibraryInfo?,
    visibleCount: Int,
    fullCachedCount: Int,
    movieIndexState: MovieIndexUiState,
    invalidIndexCount: Int,
    appendedMissingCount: Int,
): String = buildList {
    add("${selectedLibrary?.name ?: "Selected ${tab.label.lowercase()} library"}: $visibleCount visible • $fullCachedCount cached • no first-10/12/30 cap.")
    add("Dense poster cards fill the route below the compact control row; home rails keep their separate TV poster sizing.")
    add("Prefetch is bounded to ${visibleCount.coerceAtMost(GRID_IMAGE_LOOKUP_LIMIT)} visible key(s), while stable grid keys keep every cached item virtualized.")
    if (tab == HomeLibraryTab.Movies) {
        add(movieIndexStatusCopy(movieIndexState, fullCachedCount))
        movieIndexReconciliationCopy(invalidIndexCount, appendedMissingCount)?.let(::add)
    } else {
        add(LibraryBrowseModels.unsupportedSeriesControlsCopy())
    }
}.joinToString(separator = "\n")

private fun movieControlsSupportingText(
    movieIndexState: MovieIndexUiState,
    fullCachedCount: Int,
    invalidIndexCount: Int,
    appendedMissingCount: Int,
): String = buildList {
    add("Sort/filter use paged movie index endpoints; uncapped cached order remains available on failure.")
    add(movieIndexStatusCopy(movieIndexState, fullCachedCount))
    movieIndexReconciliationCopy(invalidIndexCount, appendedMissingCount)?.let(::add)
}.joinToString(separator = "\n")

private fun movieIndexStatusCopy(movieIndexState: MovieIndexUiState, fullCachedCount: Int): String = when (movieIndexState) {
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

private fun movieIndexReconciliationCopy(invalidIndexCount: Int, appendedMissingCount: Int): String? =
    if (invalidIndexCount > 0 || appendedMissingCount > 0) {
        "Index reconciliation: $invalidIndexCount invalid index value(s), $appendedMissingCount cached item(s) appended to avoid a silent cap."
    } else {
        null
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
    val gridSpec = FerrexDesignTokens.DenseLibraryGrid.tv
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = gridSpec.minCellWidth),
        modifier = modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Tv.surface(TvGridFocusPolicy.SURFACE_CARDS)),
        contentPadding = PaddingValues(
            horizontal = gridSpec.contentPaddingHorizontal,
            vertical = gridSpec.contentPaddingVertical,
        ),
        horizontalArrangement = Arrangement.spacedBy(gridSpec.horizontalSpacing),
        verticalArrangement = Arrangement.spacedBy(gridSpec.verticalSpacing),
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
                density = TvPosterCardDensity.DenseGrid,
                testTag = FerrexQaTags.Tv.poster(TvGridFocusPolicy.SURFACE_CARDS, itemKey),
            )
        }
    }
}
