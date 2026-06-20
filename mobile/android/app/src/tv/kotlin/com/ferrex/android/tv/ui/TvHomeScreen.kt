package com.ferrex.android.tv.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import com.ferrex.android.core.auth.AuthConnectionHealth
import com.ferrex.android.core.auth.AuthenticatedConnectionSurface
import com.ferrex.android.core.auth.ConnectionRecoveryRefreshGate
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.auth.connectionRecoveryUi
import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.HomeLibraryTab
import com.ferrex.android.core.browse.LibraryBrowseModels
import com.ferrex.android.core.browse.LibraryIndexResult
import com.ferrex.android.core.browse.LibraryIndexTransport
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.browse.MovieFilterMode
import com.ferrex.android.core.browse.MovieSortMode
import com.ferrex.android.core.detail.DetailCache
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.LibraryKind
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.playback.PlaybackLaunchDecision
import com.ferrex.android.core.playback.PlaybackLaunchPolicy
import com.ferrex.android.core.playback.PlaybackProgressReporter
import com.ferrex.android.core.playback.PlaybackResumeProgressProvider
import com.ferrex.android.core.playback.PlaybackRoutePersistence
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.PlaybackTicketTransport
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.tvfocus.TvGridFocusPolicy
import com.ferrex.android.core.tvfocus.TvHomeFocusPolicy
import com.ferrex.android.core.tvfocus.TvSearchFocusPolicy
import com.ferrex.android.core.watch.ContinueWatchingRepository
import com.ferrex.android.core.watch.ContinueWatchingState
import com.ferrex.android.core.watch.WatchRepository
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.core.watch.WatchStateInvalidationBus
import com.ferrex.android.navigation.PlaybackRouteContractSaver
import com.ferrex.android.tv.ui.foundation.rememberTvFocusRestorer
import com.ferrex.android.ui.components.rememberScopedImageLoader
import com.ferrex.android.ui.components.rememberVisibleImageResolutionState
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient

@Composable
fun TvHomeScreen(
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
    val routePersistenceScope = remember(state.serverUrl, state.user.id) {
        PlaybackRoutePersistence.scopeKey(state.serverUrl, state.user.id)
    }
    val homeFocusRestorer = rememberTvFocusRestorer(TvHomeFocusPolicy.SCREEN_HOME)
    val gridFocusRestorer = rememberTvFocusRestorer(TvGridFocusPolicy.SCREEN_GRID)
    val searchFocusRestorer = rememberTvFocusRestorer(TvSearchFocusPolicy.SCREEN_SEARCH)

    var childScreen by remember { mutableStateOf<TvHomeChild?>(null) }
    var searchQuery by remember(scope.directoryName) { mutableStateOf("") }
    var selectedTab by remember { mutableStateOf(HomeLibraryTab.Movies) }
    var selectedMovieLibraryId by remember { mutableStateOf<String?>(null) }
    var selectedSeriesLibraryId by remember { mutableStateOf<String?>(null) }
    var movieSort by remember { mutableStateOf(MovieSortMode.TitleAsc) }
    var movieFilter by remember { mutableStateOf(MovieFilterMode.All) }
    var movieIndexState by remember { mutableStateOf<MovieIndexUiState>(MovieIndexUiState.Idle) }
    var activePlaybackContract by rememberSaveable(routePersistenceScope, stateSaver = PlaybackRouteContractSaver) {
        mutableStateOf<PlaybackRouteContract?>(null)
    }
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

    val movieLibraries = repositoryState?.movieLibraries.orEmpty()
    val seriesLibraries = repositoryState?.seriesLibraries.orEmpty()
    val movieLibraryInfos = repositoryState?.libraries.orEmpty().filter { it.kind == LibraryKind.Movies }
    val seriesLibraryInfos = repositoryState?.libraries.orEmpty().filter { it.kind == LibraryKind.Series }

    LaunchedEffect(movieLibraries, movieLibraryInfos) {
        val selectedStillExists = movieLibraries.any { it.library.id == selectedMovieLibraryId } ||
            movieLibraryInfos.any { it.id == selectedMovieLibraryId }
        if (!selectedStillExists) {
            selectedMovieLibraryId = movieLibraries.firstOrNull()?.library?.id ?: movieLibraryInfos.firstOrNull()?.id
        }
    }
    LaunchedEffect(seriesLibraries, seriesLibraryInfos) {
        val selectedStillExists = seriesLibraries.any { it.library.id == selectedSeriesLibraryId } ||
            seriesLibraryInfos.any { it.id == selectedSeriesLibraryId }
        if (!selectedStillExists) {
            selectedSeriesLibraryId = seriesLibraries.firstOrNull()?.library?.id ?: seriesLibraryInfos.firstOrNull()?.id
        }
    }

    val selectedMovieLibrary = movieLibraries.firstOrNull { it.library.id == selectedMovieLibraryId }
    val selectedSeriesLibrary = seriesLibraries.firstOrNull { it.library.id == selectedSeriesLibraryId }
    val selectedMovieInfo = selectedMovieLibrary?.library ?: movieLibraryInfos.firstOrNull { it.id == selectedMovieLibraryId }
    val selectedSeriesInfo = selectedSeriesLibrary?.library ?: seriesLibraryInfos.firstOrNull { it.id == selectedSeriesLibraryId }
    val selectedMovieFreshness = repositoryState?.takeIf { it.selectedLibraryId == selectedMovieInfo?.id }?.freshness ?: LibraryFreshness.Empty
    val selectedSeriesFreshness = repositoryState?.takeIf { it.selectedLibraryId == selectedSeriesInfo?.id }?.freshness ?: LibraryFreshness.Empty

    LaunchedEffect(libraryRepository, scope.directoryName, childScreen, selectedSeriesInfo?.id) {
        if ((childScreen as? TvHomeChild.Grid)?.tab == HomeLibraryTab.Series) {
            selectedSeriesInfo?.let { library ->
                libraryRepository?.syncSeriesLibrary(scope, library, repositoryState?.libraries.orEmpty())
            }
        }
    }

    LaunchedEffect(selectedTab, selectedMovieLibrary?.library?.id, selectedMovieLibrary?.accessor, movieSort, movieFilter, libraryIndexTransport, state.connectionHealth) {
        if (selectedTab != HomeLibraryTab.Movies || selectedMovieLibrary == null) {
            movieIndexState = MovieIndexUiState.Idle
            return@LaunchedEffect
        }
        if (state.connectionHealth != AuthConnectionHealth.Online) {
            movieIndexState = MovieIndexUiState.Unavailable(
                "Movie sorting and filters are paused until Ferrex reconnects; showing uncapped cached order.",
            )
            return@LaunchedEffect
        }
        if (libraryIndexTransport == null) {
            movieIndexState = MovieIndexUiState.Unavailable(
                "Movie index endpoints are unavailable in this build; showing uncapped cached batch order.",
            )
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
    val imageLoader = rememberScopedImageLoader(imagePipeline, scope)

    val detailScreen = childScreen as? TvHomeChild.Detail
    val detailResult = remember(repositoryState, detailScreen?.route) {
        detailScreen?.route?.let { DetailCache.resolve(repositoryState, it) }
    }
    LaunchedEffect(detailResult, watchRepository) {
        when (val detail = detailResult) {
            is DetailLoadResult.Movie -> watchRepository?.refreshMediaProgress(detail.detail.id)
            is DetailLoadResult.Series -> detail.detail.series.tmdbId?.let { watchRepository?.refreshSeries(it) }
            is DetailLoadResult.Season -> detail.series?.tmdbId?.let { watchRepository?.refreshSeries(it) }
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
                is DetailLoadResult.Season -> detail.series?.tmdbId?.let { watchRepository?.refreshSeries(it) }
                is DetailLoadResult.Episode -> watchRepository?.refreshMediaProgress(detail.detail.id)
                is DetailLoadResult.Missing,
                null -> Unit
            }
        }
    }

    val gridCardsForImages = when ((childScreen as? TvHomeChild.Grid)?.tab ?: selectedTab) {
        HomeLibraryTab.Movies -> indexedMovieCards.cards
        HomeLibraryTab.Series -> selectedSeriesCards
    }
    val browseImageKeys = remember(continueState, shelves, gridCardsForImages, childScreen) {
        buildList {
            continueState.cards.mapNotNullTo(this) { it.imageKey }
            shelves.flatMap { it.items }.mapNotNullTo(this) { it.imageKey }
            if (childScreen is TvHomeChild.Grid) {
                gridCardsForImages.take(GRID_IMAGE_LOOKUP_LIMIT).mapNotNullTo(this) { it.imageKey }
            }
        }.distinctBy { it.cacheKey }.take(GRID_IMAGE_LOOKUP_LIMIT)
    }
    val imageKeys = remember(browseImageKeys, detailResult) {
        (DetailCache.imageKeys(detailResult) + browseImageKeys).distinctBy { it.cacheKey }.toSet()
    }
    val visibleImageState = rememberVisibleImageResolutionState(
        scope = scope,
        imageRepository = imageRepository,
        visibleKeys = imageKeys,
    )
    val imageResolutions = visibleImageState.resolutions

    fun openDetail(route: MediaRouteArgs, returnTo: TvReturnTarget) {
        playbackNotice = null
        childScreen = TvHomeChild.Detail(route, returnTo)
    }

    suspend fun syncLibrary(library: LibraryInfo) {
        when (library.kind) {
            LibraryKind.Movies -> libraryRepository?.syncMovieLibrary(scope, library, repositoryState?.libraries.orEmpty())
            LibraryKind.Series -> libraryRepository?.syncSeriesLibrary(scope, library, repositoryState?.libraries.orEmpty())
            LibraryKind.Unknown -> Unit
        }
    }

    fun syncSelectedLibrary(tab: HomeLibraryTab) {
        coroutineScope.launch {
            when (tab) {
                HomeLibraryTab.Movies -> selectedMovieInfo?.let { syncLibrary(it) } ?: libraryRepository?.refreshLibraries(scope)
                HomeLibraryTab.Series -> selectedSeriesInfo?.let { syncLibrary(it) } ?: libraryRepository?.refreshLibraries(scope)
            }
        }
    }

    fun retryAllLibrariesForTab(tab: HomeLibraryTab) {
        coroutineScope.launch {
            val plan = LibraryBrowseModels.retryAllTargetPlan(tab, repositoryState?.libraries.orEmpty())
            if (plan.libraries.isEmpty()) {
                val selectedLibraryId = when (tab) {
                    HomeLibraryTab.Movies -> selectedMovieInfo?.id
                    HomeLibraryTab.Series -> selectedSeriesInfo?.id
                }
                selectedLibraryId?.let { libraryRepository?.refreshLibraries(scope, it) }
            } else {
                plan.libraries.forEach { syncLibrary(it) }
            }
        }
    }

    fun retryDetailCacheSync(route: MediaRouteArgs?) {
        coroutineScope.launch {
            val libraryId = route?.libraryId
            val library = repositoryState?.libraries.orEmpty().firstOrNull { it.id == libraryId }
            when (route?.mediaType) {
                BrowseMediaType.Movie -> if (library != null) {
                    libraryRepository?.syncMovieLibrary(scope, library, repositoryState?.libraries.orEmpty())
                } else {
                    libraryRepository?.refreshLibraries(scope, libraryId)
                }
                BrowseMediaType.Series,
                BrowseMediaType.Season,
                BrowseMediaType.Episode -> if (library != null) {
                    libraryRepository?.syncSeriesLibrary(scope, library, repositoryState?.libraries.orEmpty())
                } else {
                    libraryRepository?.refreshLibraries(scope, libraryId)
                }
                BrowseMediaType.Unknown,
                null -> libraryRepository?.refreshLibraries(scope, libraryId)
            }
        }
    }

    fun retryDetailWatch(detail: DetailLoadResult?) {
        coroutineScope.launch {
            when (detail) {
                is DetailLoadResult.Movie -> watchRepository?.refreshMediaProgress(detail.detail.id)
                is DetailLoadResult.Series -> detail.detail.series.tmdbId?.let { watchRepository?.refreshSeries(it) }
                is DetailLoadResult.Season -> detail.series?.tmdbId?.let { watchRepository?.refreshSeries(it) }
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
            is DetailLoadResult.Season -> detail.series?.tmdbId?.let { watchRepository?.refreshSeries(it) }
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
                visibleImageState.retryVisibleNow()
            }
            continueWatchingRepository?.refresh()
            refreshVisibleWatchState(detailResult)
        }
    }

    fun launchPlayback(contract: PlaybackRouteContract) {
        when (
            val decision = PlaybackLaunchPolicy.tv(
                route = contract,
                networkActionsEnabled = detailConnectionUi.networkActionsEnabled,
                networkActionMessage = detailConnectionUi.networkActionMessage,
                ticketTransportReady = playbackTicketTransport != null,
                streamUrlFactoryReady = playbackStreamUrlFactory != null,
                streamingHttpClientReady = streamingHttpClient != null,
            )
        ) {
            is PlaybackLaunchDecision.Launch -> {
                playbackNotice = null
                activePlaybackContract = decision.route
            }
            is PlaybackLaunchDecision.Blocked -> playbackNotice = decision.message
        }
    }

    fun refreshPlaybackProgress(contract: PlaybackRouteContract) {
        watchStateInvalidationBus?.notifyWatchStateChanged("playback progress:${contract.logicalMediaId}")
        coroutineScope.launch { watchRepository?.refreshMediaProgress(contract.logicalMediaId) }
    }

    if (
        TvHomePlaybackHost(
            playbackContract = activePlaybackContract,
            playbackTicketTransport = playbackTicketTransport,
            playbackStreamUrlFactory = playbackStreamUrlFactory,
            playbackProgressReporter = playbackProgressReporter,
            playbackResumeProgressProvider = playbackResumeProgressProvider,
            streamingHttpClient = streamingHttpClient,
            onBack = { activePlaybackContract = null },
            onSessionInvalidated = {
                activePlaybackContract = null
                onPlaybackSessionInvalidated()
            },
            onProgressCommitted = { refreshPlaybackProgress(it) },
            onChangeServer = {
                activePlaybackContract = null
                onChangeServer()
            },
            onSignOut = {
                activePlaybackContract = null
                onSignOut()
            },
            onOpenDiagnostics = onOpenDiagnostics,
        )
    ) {
        return
    }

    when (val screen = childScreen) {
        TvHomeChild.Search -> TvSearchScreen(
            scope = scope,
            searchRepository = searchRepository,
            imageRepository = imageRepository,
            imagePipeline = imagePipeline,
            query = searchQuery,
            onQueryChange = { searchQuery = it },
            focusRestorer = searchFocusRestorer,
            onOpenResult = { target ->
                target.toMediaRouteArgs()?.let { openDetail(it, TvReturnTarget.Search) }
            },
            onBack = { childScreen = null },
            onOpenDiagnostics = onOpenDiagnostics,
        )
        is TvHomeChild.Grid -> TvLibraryGridScreen(
            tab = screen.tab,
            selectedMovieInfo = selectedMovieInfo,
            selectedSeriesInfo = selectedSeriesInfo,
            movieLibraryInfos = movieLibraryInfos,
            seriesLibraryInfos = seriesLibraryInfos,
            selectedMovieLibraryId = selectedMovieLibraryId,
            selectedSeriesLibraryId = selectedSeriesLibraryId,
            cachedMovieLibraryIds = movieLibraries.map { it.library.id }.toSet(),
            cachedSeriesLibraryIds = seriesLibraries.map { it.library.id }.toSet(),
            onSelectedMovieLibrary = { selectedMovieLibraryId = it },
            onSelectedSeriesLibrary = { selectedSeriesLibraryId = it },
            onSelectedTab = {
                selectedTab = it
                childScreen = TvHomeChild.Grid(it)
            },
            movieSort = movieSort,
            movieFilter = movieFilter,
            onMovieSort = { movieSort = it },
            onMovieFilter = { movieFilter = it },
            movieIndexState = movieIndexState,
            indexedMovieCards = indexedMovieCards,
            selectedSeriesCards = selectedSeriesCards,
            fullMovieCount = selectedMovieLibrary?.accessor?.movieCount ?: 0,
            fullSeriesCount = selectedSeriesLibrary?.accessor?.seriesReferenceCount ?: 0,
            imageResolutions = imageResolutions,
            imageLoader = imageLoader,
            scope = scope,
            focusRestorer = gridFocusRestorer,
            onSelect = { openDetail(it.route, TvReturnTarget.Grid(screen.tab)) },
            libraryFreshness = when (screen.tab) {
                HomeLibraryTab.Movies -> selectedMovieFreshness
                HomeLibraryTab.Series -> selectedSeriesFreshness
            },
            retryAllLabel = LibraryBrowseModels.retryAllTargetPlan(screen.tab, repositoryState?.libraries.orEmpty()).label,
            onSyncSelected = { syncSelectedLibrary(screen.tab) },
            onRetryAll = { retryAllLibrariesForTab(screen.tab) },
            onClearSelected = {
                val libraryId = when (screen.tab) {
                    HomeLibraryTab.Movies -> selectedMovieInfo?.id
                    HomeLibraryTab.Series -> selectedSeriesInfo?.id
                } ?: return@TvLibraryGridScreen
                libraryRepository?.clearSelectedCache(scope, libraryId)
            },
            onClearAll = { libraryRepository?.clearAllCache(scope) },
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
            onOpenDiagnostics = onOpenDiagnostics,
            onBack = { childScreen = null },
        )
        is TvHomeChild.Detail -> TvMediaDetailScreen(
            detailResult = detailResult,
            watchState = watchState,
            libraryFreshness = repositoryState?.takeIf { it.selectedLibraryId == screen.route.libraryId }?.freshness ?: LibraryFreshness.Empty,
            imageResolutions = imageResolutions,
            imageLoader = imageLoader,
            scope = scope,
            playbackNotice = playbackNotice,
            connectionStatus = detailConnectionUi,
            onBack = { childScreen = screen.returnTo.toChild(state.connectionHealth) },
            onRetryConnection = onRetryConnection,
            onRetryCacheSync = { retryDetailCacheSync(screen.route) },
            onClearSelectedCache = { screen.route.libraryId?.let { libraryRepository?.clearSelectedCache(scope, it) } },
            onClearAllCache = { libraryRepository?.clearAllCache(scope) },
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
            onRetryWatch = { retryDetailWatch(detailResult) },
            onClearProgress = { mediaId -> runNetworkAction { watchRepository?.clearProgress(mediaId) } },
            onMarkMovieWatched = { mediaId, watched -> runNetworkAction { watchRepository?.markMovieWatched(mediaId, watched) } },
            onMarkEpisodeWatched = { mediaId, watched -> runNetworkAction { watchRepository?.markEpisodeWatched(mediaId, watched) } },
            onMarkSeriesWatched = { tmdbId, watched -> runNetworkAction { watchRepository?.markSeriesWatched(tmdbId, watched) } },
            onPlaybackContract = { launchPlayback(it) },
            onOpenDetail = { openDetail(it, screen.returnTo) },
            onOpenDiagnostics = onOpenDiagnostics,
        )
        null -> TvHomeContent(
            state = state,
            repositoryState = repositoryState,
            continueState = continueState,
            movieLibraryInfos = movieLibraryInfos,
            seriesLibraryInfos = seriesLibraryInfos,
            movieLibraries = movieLibraries,
            seriesLibraries = seriesLibraries,
            selectedTab = selectedTab,
            onSelectedTab = { selectedTab = it },
            selectedMovieLibraryId = selectedMovieLibraryId,
            selectedSeriesLibraryId = selectedSeriesLibraryId,
            selectedMovieInfo = selectedMovieInfo,
            selectedSeriesInfo = selectedSeriesInfo,
            onSelectedMovieLibrary = { selectedMovieLibraryId = it },
            onSelectedSeriesLibrary = { selectedSeriesLibraryId = it },
            shelves = shelves,
            imageResolutions = imageResolutions,
            imageLoader = imageLoader,
            scope = scope,
            focusRestorer = homeFocusRestorer,
            searchAvailable = searchRepository != null,
            playbackNotice = playbackNotice,
            connectionStatus = homeConnectionUi,
            onRetryContinueWatching = { coroutineScope.launch { continueWatchingRepository?.refresh() } },
            onRetryConnection = onRetryConnection,
            onOpenSearch = { childScreen = TvHomeChild.Search },
            onOpenGrid = {
                selectedTab = it
                childScreen = TvHomeChild.Grid(it)
            },
            onOpenDetail = { openDetail(it, TvReturnTarget.Home) },
            onRetryLibraries = { syncSelectedLibrary(selectedTab) },
            onSyncSelected = { syncSelectedLibrary(selectedTab) },
            onClearSelected = {
                val libraryId = when (selectedTab) {
                    HomeLibraryTab.Movies -> selectedMovieInfo?.id
                    HomeLibraryTab.Series -> selectedSeriesInfo?.id
                } ?: return@TvHomeContent
                libraryRepository?.clearSelectedCache(scope, libraryId)
            },
            onClearAll = { libraryRepository?.clearAllCache(scope) },
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
            onOpenDiagnostics = onOpenDiagnostics,
        )
    }
}
