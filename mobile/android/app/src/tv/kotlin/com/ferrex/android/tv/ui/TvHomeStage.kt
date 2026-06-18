package com.ferrex.android.tv.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextAlign
import coil.ImageLoader
import com.ferrex.android.FerrexShellCopy
import com.ferrex.android.core.auth.AuthenticatedConnectionUi
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.browse.HomeLibraryTab
import com.ferrex.android.core.browse.LibraryBrowseModels
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.CachedMovieLibrary
import com.ferrex.android.core.library.CachedSeriesLibrary
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.tvfocus.TvHomeFocusPolicy
import com.ferrex.android.core.watch.ContinueWatchingState
import com.ferrex.android.core.watch.ContinueWatchingStatus
import com.ferrex.android.tv.ui.foundation.TvActionPanel
import com.ferrex.android.tv.ui.foundation.TvActionPanelAction
import com.ferrex.android.tv.ui.foundation.TvActionRole
import com.ferrex.android.tv.ui.foundation.TvFocusRestorer
import com.ferrex.android.tv.ui.foundation.TvScaffold
import com.ferrex.android.tv.ui.foundation.TvTitle
import com.ferrex.android.ui.components.FerrexStatusCard
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theme.FerrexDesignTokens

@Composable
internal fun TvHomeContent(
    state: SessionState.Authenticated,
    repositoryState: LibraryRepositoryState?,
    continueState: ContinueWatchingState,
    movieLibraryInfos: List<LibraryInfo>,
    seriesLibraryInfos: List<LibraryInfo>,
    movieLibraries: List<CachedMovieLibrary>,
    seriesLibraries: List<CachedSeriesLibrary>,
    selectedTab: HomeLibraryTab,
    onSelectedTab: (HomeLibraryTab) -> Unit,
    selectedMovieLibraryId: String?,
    selectedSeriesLibraryId: String?,
    selectedMovieInfo: LibraryInfo?,
    selectedSeriesInfo: LibraryInfo?,
    onSelectedMovieLibrary: (String) -> Unit,
    onSelectedSeriesLibrary: (String) -> Unit,
    shelves: List<com.ferrex.android.core.browse.HomeShelf>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRestorer: TvFocusRestorer,
    searchAvailable: Boolean,
    playbackNotice: String?,
    connectionStatus: AuthenticatedConnectionUi,
    onRetryContinueWatching: () -> Unit,
    onRetryConnection: () -> Unit,
    onOpenSearch: () -> Unit,
    onOpenGrid: (HomeLibraryTab) -> Unit,
    onOpenDetail: (MediaRouteArgs) -> Unit,
    onRetryLibraries: () -> Unit,
    onSyncSelected: () -> Unit,
    onClearSelected: () -> Unit,
    onClearAll: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val continueEntries = remember(continueState.cards) { continueState.cards.map { it.toPosterEntry() } }
    val cachedIds = when (selectedTab) {
        HomeLibraryTab.Movies -> movieLibraries.map { it.library.id }.toSet()
        HomeLibraryTab.Series -> seriesLibraries.map { it.library.id }.toSet()
    }
    val selectedLibraryId = when (selectedTab) {
        HomeLibraryTab.Movies -> selectedMovieInfo?.id
        HomeLibraryTab.Series -> selectedSeriesInfo?.id
    }
    val libraryActionKeys = buildList {
        if (movieLibraryInfos.isNotEmpty() || movieLibraries.isNotEmpty()) add("browse-movies")
        if (seriesLibraryInfos.isNotEmpty() || seriesLibraries.isNotEmpty()) add("browse-series")
        if (selectedLibraryId != null) add("sync-selected")
    }
    val recoveryActionKeys = buildList {
        add("retry-cache-sync")
        if (selectedLibraryId != null) add("clear-selected-cache")
        add("clear-all-cache")
        add("change-server")
        add("reset-connection")
    }
    val homeActionKeys = buildList {
        if (connectionStatus.visible) add("retry-connection")
        if (searchAvailable) add(TvHomeFocusPolicy.ITEM_SEARCH)
        add(TvHomeFocusPolicy.ITEM_DIAGNOSTICS)
    }
    val initialTarget = TvHomeFocusPolicy.initialHomeTarget(
        continueWatchingKeys = continueEntries.map { it.stableKey },
        searchAvailable = searchAvailable,
        libraryActionKeys = libraryActionKeys,
        recoveryActionKeys = recoveryActionKeys,
        homeActionKeys = homeActionKeys,
    )
    val availableSurfaces = buildSet {
        if (continueEntries.isNotEmpty()) add(TvHomeFocusPolicy.SURFACE_CONTINUE_WATCHING)
        if (homeActionKeys.isNotEmpty()) add(TvHomeFocusPolicy.SURFACE_HOME_ACTIONS)
        if (movieLibraryInfos.isNotEmpty() || seriesLibraryInfos.isNotEmpty()) add("library-tabs")
        if (selectedTab == HomeLibraryTab.Movies && (movieLibraryInfos.isNotEmpty() || movieLibraries.isNotEmpty())) add("library-chooser")
        if (selectedTab == HomeLibraryTab.Series && (seriesLibraryInfos.isNotEmpty() || seriesLibraries.isNotEmpty())) add("library-chooser")
        if (libraryActionKeys.isNotEmpty()) add(TvHomeFocusPolicy.SURFACE_LIBRARY_ACTIONS)
        shelves.forEach { add(shelfSurfaceKey(it)) }
        add(TvHomeFocusPolicy.SURFACE_RECOVERY_ACTIONS)
    }
    val lastHomeTarget = focusRestorer.state.lastTarget(TvHomeFocusPolicy.SCREEN_HOME)
    val preferredSurface = lastHomeTarget?.surface?.takeIf { it in availableSurfaces } ?: initialTarget.surface

    TvScaffold(
        modifier = Modifier.testTag(FerrexQaTags.Tv.Home),
        contentMaxWidth = FerrexDesignTokens.Tv.HomeMaxWidth,
        horizontalPadding = FerrexDesignTokens.Space.ScreenTvHorizontal,
        verticalPadding = FerrexDesignTokens.Space.ScreenTvVertical,
        verticalArrangement = Arrangement.Top,
        scrollable = true,
    ) {
        TvTitle(FerrexShellCopy.TV_TITLE, FerrexShellCopy.TV_SUBTITLE)
        Text("Signed in as ${state.user.displayName ?: state.user.username}", style = MaterialTheme.typography.headlineSmall)
        Text("Server: ${state.serverUrl}", style = MaterialTheme.typography.titleMedium)
        if (connectionStatus.visible) {
            Text(
                connectionStatus.message,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary,
                textAlign = TextAlign.Center,
            )
        }
        Text(FerrexShellCopy.TV_BODY, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
        if (state.requiresPinSetup) {
            Text(
                text = "PIN setup is required by this server before PIN sign-in can be used. Use password sign-in or configure PIN support on the server.",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary,
                textAlign = TextAlign.Center,
            )
        }
        playbackNotice?.let {
            Text(it, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary, textAlign = TextAlign.Center)
        }
        Spacer(Modifier.height(FerrexDesignTokens.Space.Xxxl))
        TvButtonRow(
            title = "Home actions",
            actions = buildList {
                if (connectionStatus.visible) {
                    add(
                        TvButtonAction(
                            key = "retry-connection",
                            label = connectionStatus.retryLabel,
                            role = TvActionRole.Retry,
                            enabled = connectionStatus.retryEnabled,
                            onSelect = onRetryConnection,
                        ),
                    )
                }
                if (searchAvailable) {
                    add(
                        TvButtonAction(
                            key = TvHomeFocusPolicy.ITEM_SEARCH,
                            label = "Search cached media",
                            role = TvActionRole.Primary,
                            onSelect = onOpenSearch,
                        ),
                    )
                }
                add(
                    TvButtonAction(
                        key = TvHomeFocusPolicy.ITEM_DIAGNOSTICS,
                        label = "Settings & Diagnostics",
                        role = TvActionRole.SettingsExit,
                        onSelect = onOpenDiagnostics,
                    ),
                )
            },
            focusRestorer = focusRestorer,
            surfaceKey = TvHomeFocusPolicy.SURFACE_HOME_ACTIONS,
            autoFocus = preferredSurface == TvHomeFocusPolicy.SURFACE_HOME_ACTIONS,
        )
        ContinueWatchingSection(
            continueState = continueState,
            entries = continueEntries,
            imageResolutions = imageResolutions,
            imageLoader = imageLoader,
            scope = scope,
            focusRestorer = focusRestorer,
            autoFocus = preferredSurface == TvHomeFocusPolicy.SURFACE_CONTINUE_WATCHING,
            onRetry = onRetryContinueWatching,
            onSelect = { it.route?.let(onOpenDetail) },
        )
        if (shelves.isEmpty()) {
            TvStateCopy(
                title = "Local shelves are waiting for cached datasets",
                body = "Home shelves are built from cached complete movie batches and series bundles. Browse all remains available once a library cache exists.",
            )
        } else {
            shelves.forEach { shelf ->
                TvShelfSection(
                    shelf = shelf,
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    scope = scope,
                    focusRestorer = focusRestorer,
                    autoFocus = preferredSurface == shelfSurfaceKey(shelf),
                    onSelect = { it.route?.let(onOpenDetail) },
                )
            }
        }
        TvLibraryEntrySection(
            selectedTab = selectedTab,
            onSelectedTab = onSelectedTab,
            movieLibraryInfos = movieLibraryInfos.ifEmpty { movieLibraries.map { it.library } },
            seriesLibraryInfos = seriesLibraryInfos.ifEmpty { seriesLibraries.map { it.library } },
            selectedMovieLibraryId = selectedMovieLibraryId,
            selectedSeriesLibraryId = selectedSeriesLibraryId,
            cachedIds = cachedIds,
            selectedMovieInfo = selectedMovieInfo,
            selectedSeriesInfo = selectedSeriesInfo,
            movieCount = movieLibraries.firstOrNull { it.library.id == selectedMovieLibraryId }?.accessor?.movieCount,
            seriesCount = seriesLibraries.firstOrNull { it.library.id == selectedSeriesLibraryId }?.accessor?.seriesReferenceCount,
            onSelectedMovieLibrary = onSelectedMovieLibrary,
            onSelectedSeriesLibrary = onSelectedSeriesLibrary,
            focusRestorer = focusRestorer,
            chooserAutoFocus = preferredSurface == "library-chooser",
            tabAutoFocus = preferredSurface == "library-tabs",
            actionsAutoFocus = preferredSurface == TvHomeFocusPolicy.SURFACE_LIBRARY_ACTIONS,
            onOpenGrid = onOpenGrid,
            onSyncSelected = onSyncSelected,
        )
        TvLibraryRecoveryPanel(
            freshness = repositoryState?.freshness ?: LibraryFreshness.Empty,
            selectedLibraryId = selectedLibraryId,
            focusRestorer = focusRestorer,
            autoFocus = preferredSurface == TvHomeFocusPolicy.SURFACE_RECOVERY_ACTIONS,
            onRetry = onRetryLibraries,
            onClearSelected = onClearSelected,
            onClearAll = onClearAll,
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
            onOpenDiagnostics = onOpenDiagnostics,
        )
    }
}

@Composable
internal fun ContinueWatchingSection(
    continueState: ContinueWatchingState,
    entries: List<TvPosterEntry>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onRetry: () -> Unit,
    onSelect: (TvPosterEntry) -> Unit,
) {
    TvSectionHeader("Continue Watching")
    when (val status = continueState.status) {
        ContinueWatchingStatus.Idle,
        ContinueWatchingStatus.Loading -> TvStateCopy(
            title = "Loading Continue Watching",
            body = "The /api/v1/watch/continue shelf loads independently and never blocks library browsing.",
            loading = status == ContinueWatchingStatus.Loading,
        )
        ContinueWatchingStatus.Empty -> TvActionPanel(
            title = "Nothing in progress",
            supportingText = "Start playback on a movie or episode and it will appear here.",
            actions = listOf(
                TvActionPanelAction("retry-continue", "Retry", TvActionRole.Retry, onSelect = onRetry),
            ),
            focusRestorer = focusRestorer,
            surfaceKey = TvHomeFocusPolicy.SURFACE_CONTINUE_WATCHING,
            autoFocus = autoFocus,
        )
        is ContinueWatchingStatus.ErrorRetryable -> TvActionPanel(
            title = "Continue Watching unavailable",
            supportingText = status.message,
            actions = listOf(
                TvActionPanelAction("retry-continue", "Retry", TvActionRole.Retry, onSelect = onRetry),
            ),
            focusRestorer = focusRestorer,
            surfaceKey = TvHomeFocusPolicy.SURFACE_CONTINUE_WATCHING,
            autoFocus = autoFocus,
        )
        is ContinueWatchingStatus.StaleOffline -> FerrexStatusCard(
            title = "Stale/offline Continue Watching",
            body = "Showing ${status.itemCount} stale/offline item(s): ${status.message}",
            tone = FerrexStatusTone.StaleOffline,
        )
        is ContinueWatchingStatus.Fresh -> Text(
            text = "${status.itemCount} current item(s) from /api/v1/watch/continue.",
            style = MaterialTheme.typography.titleMedium,
        )
    }
    if (entries.isNotEmpty()) {
        TvPosterRow(
            title = null,
            supportingText = "Resume targets open cached details and ticketed playback actions.",
            entries = entries,
            imageResolutions = imageResolutions,
            imageLoader = imageLoader,
            scope = scope,
            focusRestorer = focusRestorer,
            surfaceKey = TvHomeFocusPolicy.SURFACE_CONTINUE_WATCHING,
            autoFocus = autoFocus,
            onSelect = onSelect,
        )
    }
    Spacer(Modifier.height(FerrexDesignTokens.Space.Xxl))
}

@Composable
internal fun TvShelfSection(
    shelf: com.ferrex.android.core.browse.HomeShelf,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onSelect: (TvPosterEntry) -> Unit,
) {
    val entries = remember(shelf.items) { shelf.items.map { it.toPosterEntry() } }
    TvPosterRow(
        title = shelf.title,
        supportingText = shelf.subtitle + " " + shelf.limitCopy,
        entries = entries,
        imageResolutions = imageResolutions,
        imageLoader = imageLoader,
        scope = scope,
        focusRestorer = focusRestorer,
        surfaceKey = shelfSurfaceKey(shelf),
        autoFocus = autoFocus,
        onSelect = onSelect,
    )
    Spacer(Modifier.height(FerrexDesignTokens.Space.Xxl))
}

@Composable
internal fun TvLibraryEntrySection(
    selectedTab: HomeLibraryTab,
    onSelectedTab: (HomeLibraryTab) -> Unit,
    movieLibraryInfos: List<LibraryInfo>,
    seriesLibraryInfos: List<LibraryInfo>,
    selectedMovieLibraryId: String?,
    selectedSeriesLibraryId: String?,
    cachedIds: Set<String>,
    selectedMovieInfo: LibraryInfo?,
    selectedSeriesInfo: LibraryInfo?,
    movieCount: Int?,
    seriesCount: Int?,
    onSelectedMovieLibrary: (String) -> Unit,
    onSelectedSeriesLibrary: (String) -> Unit,
    focusRestorer: TvFocusRestorer,
    chooserAutoFocus: Boolean,
    tabAutoFocus: Boolean,
    actionsAutoFocus: Boolean,
    onOpenGrid: (HomeLibraryTab) -> Unit,
    onSyncSelected: () -> Unit,
) {
    TvSectionHeader("Library")
    TvButtonRow(
        supportingText = "Choose a media type, pick a library, then open a full virtualized grid with no first-page cap.",
        actions = HomeLibraryTab.entries.map { tab ->
            TvButtonAction(
                key = "tab-${tab.name.lowercase()}",
                label = tab.label,
                role = if (tab == selectedTab) TvActionRole.Primary else TvActionRole.Cache,
                onSelect = { onSelectedTab(tab) },
            )
        },
        focusRestorer = focusRestorer,
        surfaceKey = "library-tabs",
        autoFocus = tabAutoFocus,
    )
    val libraries = when (selectedTab) {
        HomeLibraryTab.Movies -> movieLibraryInfos
        HomeLibraryTab.Series -> seriesLibraryInfos
    }
    val selectedId = when (selectedTab) {
        HomeLibraryTab.Movies -> selectedMovieLibraryId
        HomeLibraryTab.Series -> selectedSeriesLibraryId
    }
    val onSelected = when (selectedTab) {
        HomeLibraryTab.Movies -> onSelectedMovieLibrary
        HomeLibraryTab.Series -> onSelectedSeriesLibrary
    }
    if (libraries.isEmpty()) {
        TvStateCopy(
            title = "No ${selectedTab.label.lowercase()} libraries reported",
            body = "Retry cache sync or change server if this server should expose ${selectedTab.label.lowercase()}.",
        )
    } else {
        TvButtonRow(
            title = "Library chooser",
            actions = libraries.map { library ->
                val cached = library.id in cachedIds
                TvButtonAction(
                    key = library.id,
                    label = if (cached) library.name else "${library.name} (not cached)",
                    role = if (library.id == selectedId) TvActionRole.Primary else TvActionRole.Cache,
                    onSelect = { onSelected(library.id) },
                )
            },
            focusRestorer = focusRestorer,
            surfaceKey = "library-chooser",
            autoFocus = chooserAutoFocus,
        )
    }
    val countCopy = when (selectedTab) {
        HomeLibraryTab.Movies -> selectedMovieInfo?.let {
            "Full movie grid for ${it.name}: ${movieCount ?: 0} cached movie(s)."
        } ?: "No movie library selected."
        HomeLibraryTab.Series -> selectedSeriesInfo?.let {
            "Full series grid for ${it.name}: ${seriesCount ?: 0} cached series."
        } ?: "No series library selected."
    }
    Text(countCopy, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
    TvButtonRow(
        actions = buildList {
            if (movieLibraryInfos.isNotEmpty()) {
                add(TvButtonAction("browse-movies", "Browse all movies", TvActionRole.Primary, onSelect = { onOpenGrid(HomeLibraryTab.Movies) }))
            }
            if (seriesLibraryInfos.isNotEmpty()) {
                add(TvButtonAction("browse-series", "Browse all series", TvActionRole.Primary, onSelect = { onOpenGrid(HomeLibraryTab.Series) }))
            }
            if (selectedId != null) {
                add(TvButtonAction("sync-selected", "Retry selected library", TvActionRole.Retry, onSelect = onSyncSelected))
            }
        },
        focusRestorer = focusRestorer,
        surfaceKey = TvHomeFocusPolicy.SURFACE_LIBRARY_ACTIONS,
        autoFocus = actionsAutoFocus,
    )
    Spacer(Modifier.height(FerrexDesignTokens.Space.Xxl))
}

@Composable
internal fun TvLibraryRecoveryPanel(
    freshness: LibraryFreshness,
    selectedLibraryId: String?,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onRetry: () -> Unit,
    onClearSelected: () -> Unit,
    onClearAll: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val status = LibraryBrowseModels.libraryStatusCopy(freshness)
    val actions = LibraryBrowseModels.recoveryActionVisibility(selectedLibraryId)
    TvActionPanel(
        title = status.title,
        supportingText = status.detail,
        actions = buildList {
            if (actions.retry) {
                add(TvActionPanelAction("retry-cache-sync", "Retry", TvActionRole.Retry, onSelect = onRetry))
            }
            if (actions.clearSelectedCache) {
                add(TvActionPanelAction("clear-selected-cache", "Clear selected cache", TvActionRole.Cache, onSelect = onClearSelected))
            }
            add(TvActionPanelAction("clear-all-cache", "Clear all cache", TvActionRole.Destructive, onSelect = onClearAll))
            add(TvActionPanelAction("sign-out", "Sign out", TvActionRole.Recovery, onSelect = onSignOut))
            if (actions.changeServer) {
                add(TvActionPanelAction("change-server", "Change server", TvActionRole.SettingsExit, onSelect = onChangeServer))
            }
            if (actions.resetConnection) {
                add(TvActionPanelAction("reset-connection", "Reset connection", TvActionRole.Destructive, onSelect = onResetConnection))
            }
            add(TvActionPanelAction("diagnostics", "Diagnostics / Export diagnostics", TvActionRole.SettingsExit, onSelect = onOpenDiagnostics))
        },
        focusRestorer = focusRestorer,
        surfaceKey = TvHomeFocusPolicy.SURFACE_RECOVERY_ACTIONS,
        autoFocus = autoFocus,
        buttonMaxWidth = FerrexDesignTokens.Tv.RecoveryActionMaxWidth,
    )
}

internal fun shelfSurfaceKey(shelf: com.ferrex.android.core.browse.HomeShelf): String =
    "shelf-${shelf.title.lowercase().replace(Regex("[^a-z0-9]+"), "-").trim('-')}"
