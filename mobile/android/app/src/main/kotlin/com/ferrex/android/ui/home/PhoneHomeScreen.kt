package com.ferrex.android.ui.home

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.ferrex.android.FerrexShellCopy
import com.ferrex.android.core.auth.AuthConnectionHealth
import com.ferrex.android.core.auth.AuthenticatedConnectionSurface
import com.ferrex.android.core.auth.AuthenticatedConnectionUi
import com.ferrex.android.core.auth.ConnectionRecoveryRefreshGate
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.auth.connectionRecoveryUi
import com.ferrex.android.core.browse.AuthenticatedHomeBackPolicy
import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.HomeLibraryTab
import com.ferrex.android.core.browse.LibraryBrowseModels
import com.ferrex.android.core.browse.LibraryIndexResult
import com.ferrex.android.core.browse.LibraryIndexTransport
import com.ferrex.android.core.browse.LibraryMediaCard
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.browse.MovieFilterMode
import com.ferrex.android.core.browse.MovieSortMode
import com.ferrex.android.core.browse.PhoneSystemBackAction
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.detail.DetailCache
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.image.TmdbImageFallbackPolicy
import com.ferrex.android.core.playback.PlaybackProgressReporter
import com.ferrex.android.core.playback.PlaybackResumeProgressProvider
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.PlaybackTicketTransport
import com.ferrex.android.core.library.CachedMovieLibrary
import com.ferrex.android.core.library.CachedSeriesLibrary
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.LibraryKind
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.SearchDetailTarget
import com.ferrex.android.core.search.SearchMediaType
import com.ferrex.android.core.watch.ContinueWatchingCard
import com.ferrex.android.core.watch.ContinueWatchingRepository
import com.ferrex.android.core.watch.ContinueWatchingState
import com.ferrex.android.core.watch.ContinueWatchingStatus
import com.ferrex.android.core.watch.WatchRepository
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.core.watch.WatchStateInvalidationBus
import com.ferrex.android.ui.components.FerrexActionButton
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexAsyncImage
import com.ferrex.android.ui.components.FerrexImageFallback
import com.ferrex.android.ui.components.FerrexPosterCard
import com.ferrex.android.ui.components.FerrexPosterPlaceholder
import com.ferrex.android.ui.components.FerrexSectionTitle
import com.ferrex.android.ui.components.FerrexStatusAction
import com.ferrex.android.ui.components.FerrexStatusCard
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.detail.PhoneDetailScreen
import com.ferrex.android.ui.player.PlayerScreen
import com.ferrex.android.ui.search.PhoneSearchPanel
import com.ferrex.android.ui.theme.FerrexDesignTokens
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient

@Composable
fun PhoneHomeScreen(
    state: SessionState.Authenticated,
    libraryRepository: LibraryRepository? = null,
    libraryIndexTransport: LibraryIndexTransport? = null,
    imageRepository: ImageRepository? = null,
    imagePipeline: FerrexImagePipeline? = null,
    searchRepository: MediaSearchRepository? = null,
    continueWatchingRepository: ContinueWatchingRepository? = null,
    watchRepository: WatchRepository? = null,
    watchStateInvalidationBus: WatchStateInvalidationBus? = null,
    playbackTicketTransport: PlaybackTicketTransport? = null,
    playbackStreamUrlFactory: PlaybackStreamUrlFactory? = null,
    playbackProgressReporter: PlaybackProgressReporter? = null,
    playbackResumeProgressProvider: PlaybackResumeProgressProvider? = null,
    streamingHttpClient: OkHttpClient? = null,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onRetryConnection: () -> Unit,
    onPlaybackSessionInvalidated: () -> Unit = {},
    onOpenDiagnostics: () -> Unit = {},
) {
    val scope = remember(state.serverUrl, state.user.id) { ServerCacheScope.from(state.serverUrl, state.user.id) }
    val emptyRepositoryState = remember { mutableStateOf<LibraryRepositoryState?>(null) }
    val repositoryState by libraryRepository?.state?.collectAsState() ?: emptyRepositoryState
    val emptyContinueState = remember { mutableStateOf(ContinueWatchingState()) }
    val continueState by continueWatchingRepository?.state?.collectAsState() ?: emptyContinueState
    val emptyWatchState = remember { mutableStateOf(WatchRepositoryState()) }
    val watchState by watchRepository?.state?.collectAsState() ?: emptyWatchState
    val coroutineScope = rememberCoroutineScope()
    var selectedTab by remember { mutableStateOf(HomeLibraryTab.Movies) }
    var selectedMovieLibraryId by remember { mutableStateOf<String?>(null) }
    var selectedSeriesLibraryId by remember { mutableStateOf<String?>(null) }
    var movieSort by remember { mutableStateOf(MovieSortMode.TitleAsc) }
    var movieFilter by remember { mutableStateOf(MovieFilterMode.All) }
    var movieIndexState by remember { mutableStateOf<MovieIndexUiState>(MovieIndexUiState.Idle) }
    var selectedDetailRoute by remember { mutableStateOf<MediaRouteArgs?>(null) }
    var activePlaybackContract by remember { mutableStateOf<PlaybackRouteContract?>(null) }
    var playbackNotice by remember { mutableStateOf<String?>(null) }
    val homeConnectionUi = state.connectionRecoveryUi(AuthenticatedConnectionSurface.Home)
    val detailConnectionUi = state.connectionRecoveryUi(AuthenticatedConnectionSurface.Detail)
    val recoveryRefreshGate = remember(scope.directoryName) { ConnectionRecoveryRefreshGate(state.connectionHealth) }

    LaunchedEffect(libraryRepository, scope) {
        libraryRepository?.refreshLibraries(scope)
    }
    LaunchedEffect(continueWatchingRepository, state.serverUrl, state.user.id) {
        continueWatchingRepository?.refresh()
    }
    LaunchedEffect(watchStateInvalidationBus, continueWatchingRepository) {
        watchStateInvalidationBus?.events?.collect {
            continueWatchingRepository?.refresh()
        }
    }
    LaunchedEffect(selectedDetailRoute) {
        playbackNotice = null
    }

    val movieLibraries = repositoryState?.movieLibraries.orEmpty()
    val seriesLibraries = repositoryState?.seriesLibraries.orEmpty()
    val movieLibraryInfos = repositoryState?.libraries.orEmpty().filter { it.kind == LibraryKind.Movies }
    val seriesLibraryInfos = repositoryState?.libraries.orEmpty().filter { it.kind == LibraryKind.Series }

    LaunchedEffect(movieLibraries, movieLibraryInfos) {
        val selectedStillExists = movieLibraries.any { it.library.id == selectedMovieLibraryId } ||
            movieLibraryInfos.any { it.id == selectedMovieLibraryId }
        if (!selectedStillExists) selectedMovieLibraryId = movieLibraries.firstOrNull()?.library?.id ?: movieLibraryInfos.firstOrNull()?.id
    }
    LaunchedEffect(seriesLibraries, seriesLibraryInfos) {
        val selectedStillExists = seriesLibraries.any { it.library.id == selectedSeriesLibraryId } ||
            seriesLibraryInfos.any { it.id == selectedSeriesLibraryId }
        if (!selectedStillExists) selectedSeriesLibraryId = seriesLibraries.firstOrNull()?.library?.id ?: seriesLibraryInfos.firstOrNull()?.id
    }

    val selectedMovieLibrary = movieLibraries.firstOrNull { it.library.id == selectedMovieLibraryId }
    val selectedSeriesLibrary = seriesLibraries.firstOrNull { it.library.id == selectedSeriesLibraryId }
    val selectedMovieInfo = selectedMovieLibrary?.library ?: movieLibraryInfos.firstOrNull { it.id == selectedMovieLibraryId }
    val selectedSeriesInfo = selectedSeriesLibrary?.library ?: seriesLibraryInfos.firstOrNull { it.id == selectedSeriesLibraryId }

    LaunchedEffect(selectedTab, selectedMovieLibrary?.library?.id, selectedMovieLibrary?.accessor, movieSort, movieFilter, libraryIndexTransport, state.connectionHealth) {
        if (selectedTab != HomeLibraryTab.Movies || selectedMovieLibrary == null) {
            movieIndexState = MovieIndexUiState.Idle
            return@LaunchedEffect
        }
        if (state.connectionHealth != AuthConnectionHealth.Online) {
            movieIndexState = MovieIndexUiState.Unavailable("Movie sorting and filters are paused until Ferrex reconnects; showing uncapped cached order.")
            return@LaunchedEffect
        }
        if (libraryIndexTransport == null) {
            movieIndexState = MovieIndexUiState.Unavailable("Movie index endpoints are unavailable in this build; showing cached batch order without silently capping results.")
            return@LaunchedEffect
        }
        movieIndexState = MovieIndexUiState.Loading
        movieIndexState = when (val result = libraryIndexTransport.fetchFilteredMovieIndices(selectedMovieLibrary.library.id, movieSort, movieFilter)) {
            is LibraryIndexResult.Success -> MovieIndexUiState.Applied(
                indices = result.value,
                filterMode = movieFilter,
                sortMode = movieSort,
            )
            is LibraryIndexResult.Unsupported -> MovieIndexUiState.Unsupported(result.message)
            is LibraryIndexResult.Failure -> MovieIndexUiState.Error(result.message)
        }
    }

    val selectedMovieCards = remember(selectedMovieLibrary) {
        selectedMovieLibrary?.let(LibraryBrowseModels::movieGridCards).orEmpty()
    }
    val indexedMovieCards = remember(selectedMovieCards, movieIndexState) {
        when (val indexState = movieIndexState) {
            is MovieIndexUiState.Applied -> LibraryBrowseModels.applyMovieIndices(
                cards = selectedMovieCards,
                indices = indexState.indices,
                appendMissing = indexState.filterMode == MovieFilterMode.All,
            )
            else -> LibraryBrowseModels.applyMovieIndices(
                cards = selectedMovieCards,
                indices = selectedMovieCards.indices.toList(),
                appendMissing = false,
            )
        }
    }
    val selectedSeriesCards = remember(selectedSeriesLibrary) {
        selectedSeriesLibrary?.let(LibraryBrowseModels::seriesGridCards).orEmpty()
    }
    val shelves = remember(movieLibraries, seriesLibraries) {
        LibraryBrowseModels.homeShelves(movieLibraries, seriesLibraries)
    }
    val imageLoader = remember(imagePipeline, scope) { imagePipeline?.imageLoader(scope) }
    val detailResult = remember(repositoryState, selectedDetailRoute) {
        selectedDetailRoute?.let { DetailCache.resolve(repositoryState, it) }
    }
    LaunchedEffect(detailResult, watchRepository) {
        when (val detail = detailResult) {
            is DetailLoadResult.Movie -> watchRepository?.refreshMediaProgress(detail.detail.id)
            is DetailLoadResult.Series -> detail.detail.series.tmdbId?.let { watchRepository?.refreshSeries(it) }
            is DetailLoadResult.Episode -> watchRepository?.refreshMediaProgress(detail.detail.id)
            is DetailLoadResult.Missing,
            null -> Unit
        }
    }
    LaunchedEffect(watchStateInvalidationBus, watchRepository, detailResult) {
        watchStateInvalidationBus?.events?.collect {
            when (val detail = detailResult) {
                is DetailLoadResult.Movie -> watchRepository?.refreshMediaProgress(detail.detail.id)
                is DetailLoadResult.Series -> detail.detail.series.tmdbId?.let { watchRepository?.refreshSeries(it) }
                is DetailLoadResult.Episode -> watchRepository?.refreshMediaProgress(detail.detail.id)
                is DetailLoadResult.Missing,
                null -> Unit
            }
        }
    }
    val imageKeys = remember(continueState, shelves, indexedMovieCards, selectedSeriesCards, selectedTab, detailResult) {
        buildList {
            continueState.cards.mapNotNullTo(this) { it.imageKey }
            shelves.flatMap { it.items }.mapNotNullTo(this) { it.imageKey }
            val gridCards = if (selectedTab == HomeLibraryTab.Movies) indexedMovieCards.cards else selectedSeriesCards
            gridCards.take(GRID_IMAGE_LOOKUP_LIMIT).mapNotNullTo(this) { it.imageKey }
            DetailCache.imageKeys(detailResult).forEach(::add)
        }.distinctBy { it.cacheKey }.take(GRID_IMAGE_LOOKUP_LIMIT).toSet()
    }
    var imageResolutions by remember(scope.directoryName) { mutableStateOf<Map<ImageRequestKey, ImageResolution>>(emptyMap()) }
    LaunchedEffect(imageRepository, scope, imageKeys) {
        imageResolutions = if (imageRepository != null && imageKeys.isNotEmpty()) {
            imageRepository.resolveImages(scope, imageKeys)
        } else {
            emptyMap()
        }
    }

    fun retryDetailCacheSync(route: MediaRouteArgs?) {
        coroutineScope.launch {
            val libraryId = route?.libraryId
            val library = repositoryState?.libraries.orEmpty().firstOrNull { it.id == libraryId }
            when (route?.mediaType) {
                com.ferrex.android.core.browse.BrowseMediaType.Movie -> if (library != null) {
                    libraryRepository?.syncMovieLibrary(scope, library, repositoryState?.libraries.orEmpty())
                } else {
                    libraryRepository?.refreshLibraries(scope, libraryId)
                }
                com.ferrex.android.core.browse.BrowseMediaType.Series,
                com.ferrex.android.core.browse.BrowseMediaType.Episode -> if (library != null) {
                    libraryRepository?.syncSeriesLibrary(scope, library, repositoryState?.libraries.orEmpty())
                } else {
                    libraryRepository?.refreshLibraries(scope, libraryId)
                }
                com.ferrex.android.core.browse.BrowseMediaType.Unknown,
                null -> libraryRepository?.refreshLibraries(scope, libraryId)
            }
        }
    }

    fun retryDetailWatch(detail: DetailLoadResult?) {
        coroutineScope.launch {
            when (detail) {
                is DetailLoadResult.Movie -> watchRepository?.refreshMediaProgress(detail.detail.id)
                is DetailLoadResult.Series -> detail.detail.series.tmdbId?.let { watchRepository?.refreshSeries(it) }
                is DetailLoadResult.Episode -> watchRepository?.refreshMediaProgress(detail.detail.id)
                is DetailLoadResult.Missing,
                null -> Unit
            }
        }
    }

    fun runNetworkAction(action: suspend () -> Unit) {
        if (!detailConnectionUi.networkActionsEnabled) {
            playbackNotice = detailConnectionUi.networkActionMessage
            return
        }
        coroutineScope.launch { action() }
    }

    suspend fun refreshVisibleWatchState(detail: DetailLoadResult?) {
        when (detail) {
            is DetailLoadResult.Movie -> watchRepository?.refreshMediaProgress(detail.detail.id)
            is DetailLoadResult.Series -> detail.detail.series.tmdbId?.let { watchRepository?.refreshSeries(it) }
            is DetailLoadResult.Episode -> watchRepository?.refreshMediaProgress(detail.detail.id)
            is DetailLoadResult.Missing,
            null -> Unit
        }
    }

    LaunchedEffect(state.connectionHealth, scope.directoryName) {
        if (recoveryRefreshGate.consumeOnlineRecoveryRefresh(state.connectionHealth)) {
            playbackNotice = null
            val selectedLibraryId = when (selectedTab) {
                HomeLibraryTab.Movies -> selectedMovieInfo?.id
                HomeLibraryTab.Series -> selectedSeriesInfo?.id
            }
            libraryRepository?.refreshLibraries(scope, selectedLibraryId)
            if (imageRepository != null && imageKeys.isNotEmpty()) {
                imageResolutions = imageRepository.retryPendingOrFailed(scope, imageKeys)
            }
            continueWatchingRepository?.refresh()
            refreshVisibleWatchState(detailResult)
        }
    }

    fun launchPlayback(contract: PlaybackRouteContract) {
        if (!detailConnectionUi.networkActionsEnabled) {
            playbackNotice = detailConnectionUi.networkActionMessage
            return
        }
        if (playbackTicketTransport == null || playbackStreamUrlFactory == null || streamingHttpClient == null) {
            playbackNotice = "Playback is unavailable because the ticketed Media3 substrate is not configured."
            return
        }
        playbackNotice = null
        activePlaybackContract = contract
    }

    fun refreshPlaybackProgress(contract: PlaybackRouteContract) {
        watchStateInvalidationBus?.notifyWatchStateChanged("playback progress:${contract.logicalMediaId}")
        coroutineScope.launch { watchRepository?.refreshMediaProgress(contract.logicalMediaId) }
    }

    val phoneBackAction = AuthenticatedHomeBackPolicy.phoneSystemBackAction(
        hasActivePlayback = activePlaybackContract != null,
        hasSelectedDetail = selectedDetailRoute != null,
    )
    BackHandler(enabled = phoneBackAction != PhoneSystemBackAction.ExitApp) {
        when (phoneBackAction) {
            PhoneSystemBackAction.ClosePlayback -> activePlaybackContract = null
            PhoneSystemBackAction.CloseDetail -> selectedDetailRoute = null
            PhoneSystemBackAction.ExitApp -> Unit
        }
    }

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        val playbackContract = activePlaybackContract
        if (
            playbackContract != null &&
            playbackTicketTransport != null &&
            playbackStreamUrlFactory != null &&
            streamingHttpClient != null
        ) {
            PlayerScreen(
                route = playbackContract,
                ticketTransport = playbackTicketTransport,
                streamUrlFactory = playbackStreamUrlFactory,
                progressReporter = playbackProgressReporter,
                resumeProgressProvider = playbackResumeProgressProvider,
                streamingHttpClient = streamingHttpClient,
                onBack = { activePlaybackContract = null },
                onSessionInvalidated = {
                    activePlaybackContract = null
                    onPlaybackSessionInvalidated()
                },
                onProgressCommitted = { refreshPlaybackProgress(playbackContract) },
                onChangeServer = onChangeServer,
                onSignOut = onSignOut,
                onOpenDiagnostics = onOpenDiagnostics,
            )
        } else if (detailResult != null && selectedDetailRoute != null) {
            PhoneDetailScreen(
                detailResult = detailResult,
                watchState = watchState,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoader != null,
                imageLoader = imageLoader,
                scope = scope,
                preparedPlaybackContract = null,
                connectionStatus = detailConnectionUi,
                actionNotice = playbackNotice,
                onBack = { selectedDetailRoute = null },
                onRetryConnection = onRetryConnection,
                onRetryCacheSync = { retryDetailCacheSync(selectedDetailRoute) },
                onClearSelectedCache = { selectedDetailRoute?.libraryId?.let { libraryRepository?.clearSelectedCache(scope, it) } },
                onChangeServer = onChangeServer,
                onResetConnection = onResetConnection,
                onRetryWatch = { retryDetailWatch(detailResult) },
                onRetryEpisodes = { retryDetailCacheSync(selectedDetailRoute) },
                onClearProgress = { mediaId -> runNetworkAction { watchRepository?.clearProgress(mediaId) } },
                onMarkMovieWatched = { mediaId, watched -> runNetworkAction { watchRepository?.markMovieWatched(mediaId, watched) } },
                onMarkEpisodeWatched = { mediaId, watched -> runNetworkAction { watchRepository?.markEpisodeWatched(mediaId, watched) } },
                onMarkSeriesWatched = { tmdbId, watched -> runNetworkAction { watchRepository?.markSeriesWatched(tmdbId, watched) } },
                onPlaybackContract = { launchPlayback(it) },
                onOpenDiagnostics = onOpenDiagnostics,
            )
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = FerrexDesignTokens.Space.ScreenPhoneHorizontal, vertical = FerrexDesignTokens.Space.ScreenPhoneVertical),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xxl),
            ) {
            item {
                HomeHeader(
                    state = state,
                    connectionStatus = homeConnectionUi,
                    playbackNotice = playbackNotice,
                    onRetryConnection = onRetryConnection,
                    onSignOut = onSignOut,
                    onOpenDiagnostics = onOpenDiagnostics,
                )
            }
            item {
                ContinueWatchingSection(
                    continueState = continueState,
                    imageResolutions = imageResolutions,
                    imageLoaderAvailable = imageLoader != null,
                    imageLoader = imageLoader,
                    scope = scope,
                    onRetry = { coroutineScope.launch { continueWatchingRepository?.refresh() } },
                    onSelect = { selectedDetailRoute = it.route },
                )
            }
            item {
                PhoneSearchPanel(
                    scope = scope,
                    searchRepository = searchRepository,
                    imageRepository = imageRepository,
                    imagePipeline = imagePipeline,
                    onOpenResult = { target -> target.toMediaRouteArgs()?.let { selectedDetailRoute = it } },
                    onOpenDiagnostics = onOpenDiagnostics,
                )
            }
            if (shelves.isNotEmpty()) {
                items(shelves, key = { it.title }) { shelf ->
                    HomeShelfSection(
                        shelf = shelf,
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoader != null,
                        imageLoader = imageLoader,
                        scope = scope,
                        onSelect = { selectedDetailRoute = it.route },
                    )
                }
            } else {
                item {
                    StateCard(
                        title = "Local shelves are waiting for cached datasets",
                        body = "Home shelves are built only from cached complete movie batches and series bundles; no backend discovery shelves are shown here.",
                    )
                }
            }
            item {
                LibraryBrowseSection(
                    selectedTab = selectedTab,
                    onSelectedTab = { selectedTab = it },
                    movieLibraries = movieLibraries,
                    seriesLibraries = seriesLibraries,
                    movieLibraryInfos = movieLibraryInfos,
                    seriesLibraryInfos = seriesLibraryInfos,
                    selectedMovieLibraryId = selectedMovieLibraryId,
                    selectedSeriesLibraryId = selectedSeriesLibraryId,
                    onSelectedMovieLibrary = { selectedMovieLibraryId = it },
                    onSelectedSeriesLibrary = { selectedSeriesLibraryId = it },
                    selectedMovieInfo = selectedMovieInfo,
                    selectedSeriesInfo = selectedSeriesInfo,
                    movieSort = movieSort,
                    movieFilter = movieFilter,
                    onMovieSort = { movieSort = it },
                    onMovieFilter = { movieFilter = it },
                    movieIndexState = movieIndexState,
                    indexedMovieCards = indexedMovieCards,
                    selectedSeriesCards = selectedSeriesCards,
                    imageResolutions = imageResolutions,
                    imageLoaderAvailable = imageLoader != null,
                    imageLoader = imageLoader,
                    scope = scope,
                    onSelect = { selectedDetailRoute = it.route },
                    onSyncSelected = {
                        coroutineScope.launch {
                            when (selectedTab) {
                                HomeLibraryTab.Movies -> selectedMovieInfo?.let {
                                    libraryRepository?.syncMovieLibrary(scope, it, repositoryState?.libraries.orEmpty())
                                } ?: libraryRepository?.refreshLibraries(scope)
                                HomeLibraryTab.Series -> selectedSeriesInfo?.let {
                                    libraryRepository?.syncSeriesLibrary(scope, it, repositoryState?.libraries.orEmpty())
                                } ?: libraryRepository?.refreshLibraries(scope)
                            }
                        }
                    },
                )
            }
            item {
                LibraryRecoveryPanel(
                    freshness = repositoryState?.freshness ?: LibraryFreshness.Empty,
                    selectedLibraryId = when (selectedTab) {
                        HomeLibraryTab.Movies -> selectedMovieInfo?.id
                        HomeLibraryTab.Series -> selectedSeriesInfo?.id
                    },
                    onRetry = { coroutineScope.launch { libraryRepository?.refreshLibraries(scope, repositoryState?.selectedLibraryId) } },
                    onClearSelected = {
                        val libraryId = when (selectedTab) {
                            HomeLibraryTab.Movies -> selectedMovieInfo?.id
                            HomeLibraryTab.Series -> selectedSeriesInfo?.id
                        } ?: return@LibraryRecoveryPanel
                        libraryRepository?.clearSelectedCache(scope, libraryId)
                    },
                    onChangeServer = onChangeServer,
                    onResetConnection = onResetConnection,
                    onOpenDiagnostics = onOpenDiagnostics,
                )
            }
        }
        }
    }
}

@Composable
private fun HomeHeader(
    state: SessionState.Authenticated,
    connectionStatus: AuthenticatedConnectionUi,
    playbackNotice: String?,
    onRetryConnection: () -> Unit,
    onSignOut: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(
            text = FerrexShellCopy.MOBILE_TITLE,
            style = MaterialTheme.typography.headlineLarge,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            text = "Signed in as ${state.user.displayName ?: state.user.username}",
            style = MaterialTheme.typography.titleMedium,
        )
        Text(text = "Server: ${state.serverUrl}", style = MaterialTheme.typography.bodyMedium)
        if (connectionStatus.visible) {
            Text(
                text = connectionStatus.message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.primary,
            )
            OutlinedButton(
                onClick = onRetryConnection,
                enabled = connectionStatus.retryEnabled,
            ) {
                Text(connectionStatus.retryLabel)
            }
        }
        Text(text = FerrexShellCopy.MOBILE_BODY, style = MaterialTheme.typography.bodyLarge)
        if (state.requiresPinSetup) {
            Text(
                text = "PIN setup is required by this server before PIN sign-in can be used. Use password sign-in or configure PIN support on the server; this app will not show a fake PIN setup flow.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.primary,
            )
        }
        playbackNotice?.let {
            Text(
                text = it,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.primary,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onOpenDiagnostics, modifier = Modifier.weight(1f)) { Text("Settings & Diagnostics") }
            TextButton(onClick = onSignOut, modifier = Modifier.weight(1f)) { Text("Sign out") }
        }
    }
}

@Composable
private fun ContinueWatchingSection(
    continueState: ContinueWatchingState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    onRetry: () -> Unit,
    onSelect: (ContinueWatchingCard) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionTitle("Continue Watching")
        when (val status = continueState.status) {
            ContinueWatchingStatus.Idle,
            ContinueWatchingStatus.Loading -> StateCard(
                title = "Loading Continue Watching",
                body = "The /api/v1/watch/continue shelf loads independently and never blocks library browsing.",
                loading = status == ContinueWatchingStatus.Loading,
            )
            ContinueWatchingStatus.Empty -> StateCard(
                title = "Nothing in progress",
                body = "Start playback on a movie or episode and it will appear here.",
                action = "Retry" to onRetry,
            )
            is ContinueWatchingStatus.ErrorRetryable -> StateCard(
                title = "Continue Watching unavailable",
                body = status.message,
                action = "Retry" to onRetry,
            )
            is ContinueWatchingStatus.StaleOffline -> StateCard(
                title = "Stale/offline Continue Watching",
                body = "Showing ${status.itemCount} previous item(s): ${status.message}",
                tone = FerrexStatusTone.StaleOffline,
                action = "Retry" to onRetry,
            )
            is ContinueWatchingStatus.Fresh -> Text(
                text = "${status.itemCount} current item(s) from /api/v1/watch/continue.",
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        if (continueState.cards.isNotEmpty()) {
            LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                items(continueState.cards, key = { it.stableKey }) { card ->
                    ContinueWatchingCardView(
                        card = card,
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoaderAvailable,
                        imageLoader = imageLoader,
                        scope = scope,
                        onClick = { onSelect(card) },
                    )
                }
            }
        }
    }
}

@Composable
private fun HomeShelfSection(
    shelf: com.ferrex.android.core.browse.HomeShelf,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    onSelect: (LibraryMediaCard) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionTitle(shelf.title)
        Text(shelf.subtitle, style = MaterialTheme.typography.bodyMedium)
        Text(shelf.limitCopy, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            items(shelf.items, key = { it.stableKey }) { card ->
                MediaCardView(
                    card = card,
                    imageResolutions = imageResolutions,
                    imageLoaderAvailable = imageLoaderAvailable,
                    imageLoader = imageLoader,
                    scope = scope,
                    compact = true,
                    onClick = { onSelect(card) },
                )
            }
        }
    }
}

@Composable
private fun LibraryBrowseSection(
    selectedTab: HomeLibraryTab,
    onSelectedTab: (HomeLibraryTab) -> Unit,
    movieLibraries: List<CachedMovieLibrary>,
    seriesLibraries: List<CachedSeriesLibrary>,
    movieLibraryInfos: List<LibraryInfo>,
    seriesLibraryInfos: List<LibraryInfo>,
    selectedMovieLibraryId: String?,
    selectedSeriesLibraryId: String?,
    onSelectedMovieLibrary: (String) -> Unit,
    onSelectedSeriesLibrary: (String) -> Unit,
    selectedMovieInfo: LibraryInfo?,
    selectedSeriesInfo: LibraryInfo?,
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    onMovieSort: (MovieSortMode) -> Unit,
    onMovieFilter: (MovieFilterMode) -> Unit,
    movieIndexState: MovieIndexUiState,
    indexedMovieCards: com.ferrex.android.core.browse.IndexedMovieCards,
    selectedSeriesCards: List<LibraryMediaCard>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    onSelect: (LibraryMediaCard) -> Unit,
    onSyncSelected: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        SectionTitle("Library")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            HomeLibraryTab.entries.forEach { tab ->
                if (tab == selectedTab) {
                    Button(onClick = { onSelectedTab(tab) }) { Text(tab.label) }
                } else {
                    OutlinedButton(onClick = { onSelectedTab(tab) }) { Text(tab.label) }
                }
            }
        }
        when (selectedTab) {
            HomeLibraryTab.Movies -> MovieLibrarySection(
                movieLibraries = movieLibraries,
                movieLibraryInfos = movieLibraryInfos,
                selectedMovieLibraryId = selectedMovieLibraryId,
                onSelectedMovieLibrary = onSelectedMovieLibrary,
                selectedMovieInfo = selectedMovieInfo,
                movieSort = movieSort,
                movieFilter = movieFilter,
                onMovieSort = onMovieSort,
                onMovieFilter = onMovieFilter,
                movieIndexState = movieIndexState,
                indexedMovieCards = indexedMovieCards,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
                onSelect = onSelect,
                onSyncSelected = onSyncSelected,
            )
            HomeLibraryTab.Series -> SeriesLibrarySection(
                seriesLibraries = seriesLibraries,
                seriesLibraryInfos = seriesLibraryInfos,
                selectedSeriesLibraryId = selectedSeriesLibraryId,
                onSelectedSeriesLibrary = onSelectedSeriesLibrary,
                selectedSeriesInfo = selectedSeriesInfo,
                selectedSeriesCards = selectedSeriesCards,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
                onSelect = onSelect,
                onSyncSelected = onSyncSelected,
            )
        }
    }
}

@Composable
private fun MovieLibrarySection(
    movieLibraries: List<CachedMovieLibrary>,
    movieLibraryInfos: List<LibraryInfo>,
    selectedMovieLibraryId: String?,
    onSelectedMovieLibrary: (String) -> Unit,
    selectedMovieInfo: LibraryInfo?,
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    onMovieSort: (MovieSortMode) -> Unit,
    onMovieFilter: (MovieFilterMode) -> Unit,
    movieIndexState: MovieIndexUiState,
    indexedMovieCards: com.ferrex.android.core.browse.IndexedMovieCards,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    onSelect: (LibraryMediaCard) -> Unit,
    onSyncSelected: () -> Unit,
) {
    val cachedIds = movieLibraries.map { it.library.id }.toSet()
    LibraryChooser(
        libraries = movieLibraryInfos.ifEmpty { movieLibraries.map { it.library } },
        selectedLibraryId = selectedMovieLibraryId,
        cachedIds = cachedIds,
        onSelectedLibrary = onSelectedMovieLibrary,
    )
    MovieControls(
        movieSort = movieSort,
        movieFilter = movieFilter,
        onMovieSort = onMovieSort,
        onMovieFilter = onMovieFilter,
    )
    MovieIndexStatus(
        movieIndexState = movieIndexState,
        totalCards = indexedMovieCards.cards.size,
        fullCachedCount = movieLibraries.firstOrNull { it.library.id == selectedMovieLibraryId }?.accessor?.movieCount ?: 0,
        invalidIndexCount = indexedMovieCards.invalidIndexCount,
        appendedMissingCount = indexedMovieCards.appendedMissingCount,
    )
    Text(
        text = selectedMovieInfo?.let { "Full movie grid for ${it.name}: ${indexedMovieCards.cards.size} visible item(s)." }
            ?: "No movie library selected.",
        style = MaterialTheme.typography.bodyMedium,
    )
    if (indexedMovieCards.cards.isEmpty()) {
        EmptyBrowseState(
            title = "No cached movies for this library",
            body = "Retry sync to fetch every movie batch. The full grid will show all cached movie rows, not a first-batch preview.",
            onSyncSelected = onSyncSelected,
        )
    } else {
        MediaGrid(
            cards = indexedMovieCards.cards,
            imageResolutions = imageResolutions,
            imageLoaderAvailable = imageLoaderAvailable,
            imageLoader = imageLoader,
            scope = scope,
            onSelect = onSelect,
        )
    }
}

@Composable
private fun SeriesLibrarySection(
    seriesLibraries: List<CachedSeriesLibrary>,
    seriesLibraryInfos: List<LibraryInfo>,
    selectedSeriesLibraryId: String?,
    onSelectedSeriesLibrary: (String) -> Unit,
    selectedSeriesInfo: LibraryInfo?,
    selectedSeriesCards: List<LibraryMediaCard>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    onSelect: (LibraryMediaCard) -> Unit,
    onSyncSelected: () -> Unit,
) {
    val cachedIds = seriesLibraries.map { it.library.id }.toSet()
    LibraryChooser(
        libraries = seriesLibraryInfos.ifEmpty { seriesLibraries.map { it.library } },
        selectedLibraryId = selectedSeriesLibraryId,
        cachedIds = cachedIds,
        onSelectedLibrary = onSelectedSeriesLibrary,
    )
    StateCard(
        title = "Series controls disabled",
        body = LibraryBrowseModels.unsupportedSeriesControlsCopy(),
    )
    Text(
        text = selectedSeriesInfo?.let { "Full series grid for ${it.name}: ${selectedSeriesCards.size} cached series across all bundles." }
            ?: "No series library selected.",
        style = MaterialTheme.typography.bodyMedium,
    )
    if (selectedSeriesCards.isEmpty()) {
        EmptyBrowseState(
            title = "No cached series for this library",
            body = "Retry sync to fetch complete series bundles. Series sorting/filtering is disabled until server index endpoints support series.",
            onSyncSelected = onSyncSelected,
        )
    } else {
        MediaGrid(
            cards = selectedSeriesCards,
            imageResolutions = imageResolutions,
            imageLoaderAvailable = imageLoaderAvailable,
            imageLoader = imageLoader,
            scope = scope,
            onSelect = onSelect,
        )
    }
}

@Composable
private fun LibraryChooser(
    libraries: List<LibraryInfo>,
    selectedLibraryId: String?,
    cachedIds: Set<String>,
    onSelectedLibrary: (String) -> Unit,
) {
    if (libraries.isEmpty()) {
        Text("No libraries reported by this server yet.", style = MaterialTheme.typography.bodyMedium)
        return
    }
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        libraries.forEach { library ->
            val label = if (library.id in cachedIds) library.name else "${library.name} (not cached)"
            if (library.id == selectedLibraryId) {
                Button(onClick = { onSelectedLibrary(library.id) }) { Text(label) }
            } else {
                OutlinedButton(onClick = { onSelectedLibrary(library.id) }) { Text(label) }
            }
        }
    }
}

@Composable
private fun MovieControls(
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    onMovieSort: (MovieSortMode) -> Unit,
    onMovieFilter: (MovieFilterMode) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Movie sort uses /api/v1/libraries/{id}/indices/sorted with paging; filters use /indices/filter.", style = MaterialTheme.typography.bodySmall)
        Row(
            modifier = Modifier.horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            MovieSortMode.entries.forEach { mode ->
                if (mode == movieSort) Button(onClick = { onMovieSort(mode) }) { Text(mode.label) } else OutlinedButton(onClick = { onMovieSort(mode) }) { Text(mode.label) }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MovieFilterMode.entries.forEach { mode ->
                if (mode == movieFilter) Button(onClick = { onMovieFilter(mode) }) { Text(mode.label) } else OutlinedButton(onClick = { onMovieFilter(mode) }) { Text(mode.label) }
            }
        }
    }
}

@Composable
private fun MovieIndexStatus(
    movieIndexState: MovieIndexUiState,
    totalCards: Int,
    fullCachedCount: Int,
    invalidIndexCount: Int,
    appendedMissingCount: Int,
) {
    val copy = when (movieIndexState) {
        MovieIndexUiState.Idle -> "Movie index idle. Showing cached batch order."
        MovieIndexUiState.Loading -> "Loading movie indices…"
        is MovieIndexUiState.Applied -> if (movieIndexState.filterMode == MovieFilterMode.All) {
            "Endpoint sorted $totalCards of $fullCachedCount cached movie(s) with ${movieIndexState.sortMode.label}."
        } else {
            "Endpoint filter ${movieIndexState.filterMode.label} returned $totalCards of $fullCachedCount cached movie(s)."
        }
        is MovieIndexUiState.Error -> "Movie index request failed: ${movieIndexState.message}. Showing uncapped cached order."
        is MovieIndexUiState.Unsupported -> "Unsupported movie index request: ${movieIndexState.message}. Showing uncapped cached order."
        is MovieIndexUiState.Unavailable -> movieIndexState.message
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        if (movieIndexState == MovieIndexUiState.Loading) CircularProgressIndicator(modifier = Modifier.size(18.dp))
        Text(copy, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    if (invalidIndexCount > 0 || appendedMissingCount > 0) {
        Text(
            text = "Index reconciliation: $invalidIndexCount invalid index value(s), $appendedMissingCount cached item(s) appended to avoid a silent cap.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.primary,
        )
    }
}

@Composable
private fun MediaGrid(
    cards: List<LibraryMediaCard>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    onSelect: (LibraryMediaCard) -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = FerrexDesignTokens.Poster.PhoneGridMin),
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 220.dp, max = 680.dp),
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
    ) {
        items(cards, key = { it.stableKey }) { card ->
            MediaCardView(
                card = card,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
                compact = false,
                onClick = { onSelect(card) },
            )
        }
    }
}

@Composable
private fun ContinueWatchingCardView(
    card: ContinueWatchingCard,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    onClick: () -> Unit,
) {
    FerrexPosterCard(
        modifier = Modifier.width(FerrexDesignTokens.Poster.PhoneWidth),
        onClick = onClick,
    ) {
        Column(modifier = Modifier.padding(FerrexDesignTokens.Space.Md), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
            Poster(
                imageKey = card.imageKey,
                title = card.title,
                fallbackPath = null,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
            )
            Text(card.title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(card.subtitle, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(card.progressLabel, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
private fun MediaCardView(
    card: LibraryMediaCard,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    compact: Boolean,
    onClick: () -> Unit,
) {
    FerrexPosterCard(
        modifier = Modifier.width(if (compact) FerrexDesignTokens.Poster.PhoneCompactWidth else FerrexDesignTokens.Poster.PhoneWidth),
        onClick = onClick,
    ) {
        Column(modifier = Modifier.padding(FerrexDesignTokens.Space.Sm), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
            Poster(
                imageKey = card.imageKey,
                title = card.title,
                fallbackPath = card.publicFallbackPath,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
            )
            Text(card.title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(card.subtitle, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(card.libraryName, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun Poster(
    imageKey: ImageRequestKey?,
    title: String,
    fallbackPath: String?,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
) {
    if (imageKey == null || !imageLoaderAvailable || imageLoader == null) {
        FerrexPosterPlaceholder(if (imageKey == null) "No poster" else "Images unavailable")
        return
    }
    val resolution = imageResolutions[imageKey]
    val fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
        PosterOnlyIidFallback.url(scope.canonicalServerUrl, imageKey)?.let { FerrexImageFallback(it, "Poster IID fallback") }
            ?: TmdbImageFallbackPolicy.publicCdnUrl(
                publicPath = fallbackPath,
                category = imageKey.category,
                productCopyAllowsPublicCdn = false,
            )?.let { FerrexImageFallback(it, "TMDB fallback") }
    } else {
        null
    }
    FerrexAsyncImage(
        resolution = resolution,
        imageLoader = imageLoader,
        contentDescription = title,
        category = imageKey.category,
        fallback = fallback,
    )
}

@Composable
private fun EmptyBrowseState(
    title: String,
    body: String,
    onSyncSelected: () -> Unit,
) {
    StateCard(title = title, body = body, action = "Retry selected library" to onSyncSelected)
}

@Composable
private fun LibraryRecoveryPanel(
    freshness: LibraryFreshness,
    selectedLibraryId: String?,
    onRetry: () -> Unit,
    onClearSelected: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val status = LibraryBrowseModels.libraryStatusCopy(freshness)
    val actions = LibraryBrowseModels.recoveryActionVisibility(selectedLibraryId)
    StateCard(title = status.title, body = status.detail, tone = freshness.statusTone())
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        if (actions.retry) {
            FerrexActionButton(
                label = "Retry",
                role = FerrexActionRole.Retry,
                onClick = onRetry,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        if (actions.clearSelectedCache) {
            FerrexActionButton(
                label = "Clear selected cache",
                role = FerrexActionRole.Cache,
                onClick = onClearSelected,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            if (actions.changeServer) {
                FerrexActionButton(
                    label = "Change server",
                    role = FerrexActionRole.Secondary,
                    onClick = onChangeServer,
                    modifier = Modifier.weight(1f),
                )
            }
            if (actions.resetConnection) {
                FerrexActionButton(
                    label = "Reset connection",
                    role = FerrexActionRole.DestructiveReset,
                    onClick = onResetConnection,
                    modifier = Modifier.weight(1f),
                )
            }
        }
        FerrexActionButton(
            label = "Diagnostics / Export diagnostics",
            role = FerrexActionRole.Secondary,
            onClick = onOpenDiagnostics,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun StateCard(
    title: String,
    body: String,
    loading: Boolean = false,
    action: Pair<String, () -> Unit>? = null,
    tone: FerrexStatusTone = FerrexStatusTone.Secondary,
    actionRole: FerrexActionRole = FerrexActionRole.Retry,
) {
    FerrexStatusCard(
        title = title,
        body = body,
        loading = loading,
        tone = tone,
        action = action?.let { (label, callback) ->
            FerrexStatusAction(label = label, role = actionRole, onClick = callback)
        },
    )
}

@Composable
private fun SectionTitle(title: String) {
    FerrexSectionTitle(title)
}

private fun LibraryFreshness.statusTone(): FerrexStatusTone = when (this) {
    LibraryFreshness.Empty,
    LibraryFreshness.Syncing,
    is LibraryFreshness.Fresh -> FerrexStatusTone.Secondary
    is LibraryFreshness.StaleOffline -> FerrexStatusTone.StaleOffline
    is LibraryFreshness.CorruptRebuilding -> FerrexStatusTone.Cache
    is LibraryFreshness.ErrorRetryable -> FerrexStatusTone.Error
}

private sealed interface MovieIndexUiState {
    data object Idle : MovieIndexUiState
    data object Loading : MovieIndexUiState
    data class Applied(
        val indices: List<Int>,
        val filterMode: MovieFilterMode,
        val sortMode: MovieSortMode,
    ) : MovieIndexUiState
    data class Unsupported(val message: String) : MovieIndexUiState
    data class Error(val message: String) : MovieIndexUiState
    data class Unavailable(val message: String) : MovieIndexUiState
}

private const val GRID_IMAGE_LOOKUP_LIMIT = 80


private fun SearchDetailTarget.toMediaRouteArgs(): MediaRouteArgs? {
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
