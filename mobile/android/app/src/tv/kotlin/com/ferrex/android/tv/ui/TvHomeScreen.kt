package com.ferrex.android.tv.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import coil.ImageLoader
import com.ferrex.android.FerrexShellCopy
import com.ferrex.android.core.auth.AuthConnectionHealth
import com.ferrex.android.core.auth.AuthenticatedConnectionSurface
import com.ferrex.android.core.auth.AuthenticatedConnectionUi
import com.ferrex.android.core.auth.ConnectionRecoveryRefreshGate
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.auth.connectionRecoveryUi
import com.ferrex.android.core.browse.AuthenticatedDetailBackDestination
import com.ferrex.android.core.browse.AuthenticatedHomeBackPolicy
import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.HomeLibraryTab
import com.ferrex.android.core.browse.IndexedMovieCards
import com.ferrex.android.core.browse.LibraryBrowseModels
import com.ferrex.android.core.browse.LibraryIndexResult
import com.ferrex.android.core.browse.LibraryIndexTransport
import com.ferrex.android.core.browse.LibraryMediaCard
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.browse.MovieFilterMode
import com.ferrex.android.core.browse.MovieSortMode
import com.ferrex.android.core.detail.DetailCache
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.detail.DetailRouteContracts
import com.ferrex.android.core.detail.EpisodesAvailability
import com.ferrex.android.core.detail.playbackTargetId
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.image.TmdbImageFallbackPolicy
import com.ferrex.android.core.mediaart.MediaRailIdentityResolver
import com.ferrex.android.core.mediaart.MediaRailItemIdentity
import com.ferrex.android.core.library.CachedMovieLibrary
import com.ferrex.android.core.library.CachedSeriesLibrary
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
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.PlaybackTicketTransport
import com.ferrex.android.core.search.MediaSearchOutcome
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.SearchDetailTarget
import com.ferrex.android.core.search.SearchFailureKind
import com.ferrex.android.core.search.SearchMediaType
import com.ferrex.android.core.search.SearchResultRow
import com.ferrex.android.core.tvfocus.TvHomeFocusPolicy
import com.ferrex.android.core.watch.ContinueWatchingCard
import com.ferrex.android.core.watch.ContinueWatchingRepository
import com.ferrex.android.core.watch.ContinueWatchingState
import com.ferrex.android.core.watch.ContinueWatchingStatus
import com.ferrex.android.core.watch.WatchRepository
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.core.watch.WatchStateInvalidationBus
import com.ferrex.android.tv.ui.foundation.TvActionPanel
import com.ferrex.android.tv.ui.foundation.TvActionPanelAction
import com.ferrex.android.tv.ui.foundation.TvActionRole
import com.ferrex.android.tv.ui.foundation.TvFocusableButton
import com.ferrex.android.tv.ui.foundation.TvFocusableStyle
import com.ferrex.android.tv.ui.foundation.TvFocusableSurface
import com.ferrex.android.tv.ui.foundation.TvFocusRestorer
import com.ferrex.android.tv.ui.foundation.TvScaffold
import com.ferrex.android.tv.ui.foundation.TvTitle
import com.ferrex.android.tv.ui.foundation.rememberTvFocusRestorer
import com.ferrex.android.ui.components.FerrexAsyncImage
import com.ferrex.android.ui.components.FerrexImageFallback
import com.ferrex.android.ui.components.FerrexStatusCard
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.statusTone
import com.ferrex.android.ui.player.PlayerChrome
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theme.FerrexDesignTokens
import com.ferrex.android.ui.player.PlayerScreen
import kotlinx.coroutines.delay
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
    val homeFocusRestorer = rememberTvFocusRestorer(TvHomeFocusPolicy.SCREEN_HOME)
    val gridFocusRestorer = rememberTvFocusRestorer("library-grid")

    var childScreen by remember { mutableStateOf<TvHomeChild?>(null) }
    var selectedTab by remember { mutableStateOf(HomeLibraryTab.Movies) }
    var selectedMovieLibraryId by remember { mutableStateOf<String?>(null) }
    var selectedSeriesLibraryId by remember { mutableStateOf<String?>(null) }
    var movieSort by remember { mutableStateOf(MovieSortMode.TitleAsc) }
    var movieFilter by remember { mutableStateOf(MovieFilterMode.All) }
    var movieIndexState by remember { mutableStateOf<MovieIndexUiState>(MovieIndexUiState.Idle) }
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
    val imageLoader = remember(imagePipeline, scope.directoryName) { imagePipeline?.imageLoader(scope) }

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
    var imageResolutions by remember(scope.directoryName) { mutableStateOf<Map<ImageRequestKey, ImageResolution>>(emptyMap()) }
    LaunchedEffect(imageRepository, scope, imageKeys) {
        imageResolutions = if (imageRepository != null && imageKeys.isNotEmpty()) {
            imageRepository.resolveImages(scope, imageKeys)
        } else {
            emptyMap()
        }
    }

    fun openDetail(route: MediaRouteArgs, returnTo: TvReturnTarget) {
        playbackNotice = null
        childScreen = TvHomeChild.Detail(route, returnTo)
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
                imageResolutions = imageRepository.retryPendingOrFailed(scope, imageKeys)
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
            chrome = PlayerChrome.Tv,
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
        return
    }

    when (val screen = childScreen) {
        TvHomeChild.Search -> TvSearchScreen(
            scope = scope,
            searchRepository = searchRepository,
            imageRepository = imageRepository,
            imagePipeline = imagePipeline,
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
            onSyncSelected = {
                coroutineScope.launch {
                    when (screen.tab) {
                        HomeLibraryTab.Movies -> selectedMovieInfo?.let {
                            libraryRepository?.syncMovieLibrary(scope, it, repositoryState?.libraries.orEmpty())
                        } ?: libraryRepository?.refreshLibraries(scope)
                        HomeLibraryTab.Series -> selectedSeriesInfo?.let {
                            libraryRepository?.syncSeriesLibrary(scope, it, repositoryState?.libraries.orEmpty())
                        } ?: libraryRepository?.refreshLibraries(scope)
                    }
                }
            },
            onRetryAll = { coroutineScope.launch { libraryRepository?.refreshLibraries(scope, repositoryState?.selectedLibraryId) } },
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
            imageResolutions = imageResolutions,
            imageLoader = imageLoader,
            scope = scope,
            playbackNotice = playbackNotice,
            connectionStatus = detailConnectionUi,
            onBack = { childScreen = screen.returnTo.toChild(state.connectionHealth) },
            onRetryConnection = onRetryConnection,
            onRetryCacheSync = { retryDetailCacheSync(screen.route) },
            onClearSelectedCache = { screen.route.libraryId?.let { libraryRepository?.clearSelectedCache(scope, it) } },
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
            onRetryWatch = { retryDetailWatch(detailResult) },
            onClearProgress = { mediaId -> runNetworkAction { watchRepository?.clearProgress(mediaId) } },
            onMarkMovieWatched = { mediaId, watched -> runNetworkAction { watchRepository?.markMovieWatched(mediaId, watched) } },
            onMarkEpisodeWatched = { mediaId, watched -> runNetworkAction { watchRepository?.markEpisodeWatched(mediaId, watched) } },
            onMarkSeriesWatched = { tmdbId, watched -> runNetworkAction { watchRepository?.markSeriesWatched(tmdbId, watched) } },
            onPlaybackContract = { launchPlayback(it) },
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
            onRetryLibraries = { coroutineScope.launch { libraryRepository?.refreshLibraries(scope, repositoryState?.selectedLibraryId) } },
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

@Composable
private fun TvHomeContent(
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
private fun ContinueWatchingSection(
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
private fun TvShelfSection(
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
private fun TvLibraryEntrySection(
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
private fun TvLibraryRecoveryPanel(
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

@Composable
private fun TvLibraryGridScreen(
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
    val lastGridTarget = focusRestorer.state.lastTarget("library-grid")
    val preferredSurface = lastGridTarget?.surface ?: "grid-cards"
    TvFullScreenSurface {
        Column(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
        ) {
            TvButtonRow(
                actions = listOf(TvButtonAction("back", "Back to Home", TvActionRole.Back, onSelect = onBack)),
                focusRestorer = focusRestorer,
                surfaceKey = "grid-header",
                autoFocus = preferredSurface == "grid-header",
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
                surfaceKey = "grid-tabs",
                autoFocus = preferredSurface == "grid-tabs",
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
                    surfaceKey = "grid-library-chooser",
                    autoFocus = preferredSurface == "grid-library-chooser",
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
                surfaceKey = "grid-recovery-actions",
                autoFocus = preferredSurface == "grid-recovery-actions",
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
                    surfaceKey = "grid-empty-actions",
                    autoFocus = preferredSurface == "grid-cards",
                )
            } else {
                TvPosterGrid(
                    cards = cards,
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    scope = scope,
                    focusRestorer = focusRestorer,
                    autoFocus = preferredSurface == "grid-cards",
                    onSelect = onSelect,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
private fun TvMovieGridControls(
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
        surfaceKey = "movie-sort-controls",
        autoFocus = preferredSurface == "movie-sort-controls",
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
        surfaceKey = "movie-filter-controls",
        autoFocus = preferredSurface == "movie-filter-controls",
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
private fun TvPosterGrid(
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
                railKey = "grid-cards",
                stableIds = cards.map { it.stableKey },
            ),
        )
    }
    val keys = gridItems.map { it.second.renderKey }
    val requesters = remember(keys) { keys.associateWith { FocusRequester() } }
    val restoredKey = keys.firstOrNull()?.let { fallback ->
        focusRestorer.restoreItem("grid-cards", keys, fallback)
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
            .testTag(FerrexQaTags.Tv.surface("grid-cards")),
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
                onFocused = { focusRestorer.record("grid-cards", itemKey) },
                onSelect = { onSelect(card) },
                modifier = Modifier.fillMaxWidth(),
                testTag = FerrexQaTags.Tv.poster("grid-cards", itemKey),
            )
        }
    }
}

@Composable
private fun TvSearchScreen(
    scope: ServerCacheScope,
    searchRepository: MediaSearchRepository?,
    imageRepository: ImageRepository?,
    imagePipeline: FerrexImagePipeline?,
    onOpenResult: (SearchDetailTarget) -> Unit,
    onBack: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    BackHandler(onBack = onBack)
    var query by remember(scope.directoryName) { mutableStateOf("") }
    var retryNonce by remember(scope.directoryName) { mutableStateOf(0) }
    var uiState by remember(scope.directoryName) { mutableStateOf<TvSearchUiState>(TvSearchUiState.Idle) }
    val focusRequester = remember { FocusRequester() }

    LaunchedEffect(Unit) { runCatching { focusRequester.requestFocus() } }
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
    var resolutions by remember(scope.directoryName, visibleKeys) { mutableStateOf<Map<ImageRequestKey, ImageResolution>>(emptyMap()) }
    LaunchedEffect(imageRepository, scope.directoryName, visibleKeys) {
        resolutions = if (imageRepository != null && visibleKeys.isNotEmpty()) {
            imageRepository.resolveImages(scope, visibleKeys)
        } else {
            emptyMap()
        }
    }

    TvFullScreenSurface {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .testTag(FerrexQaTags.Tv.Search),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
        ) {
            Text("Search", style = MaterialTheme.typography.displaySmall, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
            Text(
                text = "Search uses the protected JSON media query contract and resolves rows through the scoped library cache. Cache misses stay visible with retry.",
                style = MaterialTheme.typography.titleMedium,
            )
            OutlinedTextField(
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(FerrexQaTags.Tv.SearchField)
                    .focusRequester(focusRequester),
                value = query,
                onValueChange = { query = it },
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
                        query = ""
                        uiState = TvSearchUiState.Idle
                    }),
                ),
                surfaceKey = "search-actions",
                autoFocus = false,
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
                    onOpenDiagnostics = onOpenDiagnostics,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
private fun TvSearchOutcome(
    outcome: MediaSearchOutcome,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    rows: List<SearchResultRow>,
    resolutions: Map<ImageRequestKey, ImageResolution>,
    onOpenResult: (SearchDetailTarget) -> Unit,
    onRetry: () -> Unit,
    onOpenDiagnostics: () -> Unit,
    modifier: Modifier = Modifier,
) {
    when (outcome) {
        MediaSearchOutcome.Idle -> TvStateCopy("Ready to search", "Enter at least two characters to search the current server.")
        is MediaSearchOutcome.NoResults -> TvActionPanel(
            title = "No results for “${outcome.query}”",
            supportingText = "Try a shorter title or alternate spelling.",
            actions = listOf(TvActionPanelAction("retry", "Retry", TvActionRole.Retry, onSelect = onRetry)),
            autoFocus = false,
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
                autoFocus = false,
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
            LazyColumn(
                modifier = modifier
                    .fillMaxWidth()
                    .testTag(FerrexQaTags.Tv.SearchResults),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
                contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Sm),
            ) {
                items(rows.take(SEARCH_RESULT_DISPLAY_LIMIT), key = { it.searchStableKey() }) { row ->
                    when (row) {
                        is SearchResultRow.Resolved -> TvSearchResolvedRow(
                            row = row,
                            resolution = row.imageKey?.let { resolutions[it] },
                            imageLoader = imageLoader,
                            scope = scope,
                            onOpenResult = onOpenResult,
                        )
                        is SearchResultRow.CacheMiss -> TvSearchCacheMissRow(row = row, onRetry = onRetry, onOpenDiagnostics = onOpenDiagnostics)
                    }
                }
            }
        }
    }
}

@Composable
private fun TvSearchResolvedRow(
    row: SearchResultRow.Resolved,
    resolution: ImageResolution?,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    onOpenResult: (SearchDetailTarget) -> Unit,
) {
    TvFocusableSurface(
        onClick = { onOpenResult(row.target) },
        semanticLabel = "Open ${row.title}",
        minHeight = FerrexDesignTokens.Tv.SearchResultMinHeight,
        testTag = FerrexQaTags.Tv.action("search-results", row.searchStableKey()),
        modifier = Modifier.fillMaxWidth(),
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
private fun TvSearchCacheMissRow(
    row: SearchResultRow.CacheMiss,
    onRetry: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    TvActionPanel(
        title = row.title,
        supportingText = row.message,
        actions = listOf(
            TvActionPanelAction("retry-${row.searchStableKey()}", "Retry sync / search", TvActionRole.Retry, enabled = row.retryable, onSelect = onRetry),
            TvActionPanelAction("diagnostics-${row.searchStableKey()}", "Diagnostics / Export diagnostics", TvActionRole.SettingsExit, onSelect = onOpenDiagnostics),
        ),
        autoFocus = false,
        buttonMaxWidth = FerrexDesignTokens.Tv.PlayerActionMaxWidth,
    )
}

@Composable
private fun SearchResultImage(
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

@Composable
private fun TvMediaDetailScreen(
    detailResult: DetailLoadResult?,
    watchState: WatchRepositoryState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    playbackNotice: String?,
    connectionStatus: AuthenticatedConnectionUi,
    onBack: () -> Unit,
    onRetryConnection: () -> Unit,
    onRetryCacheSync: () -> Unit,
    onClearSelectedCache: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onRetryWatch: () -> Unit,
    onClearProgress: (String) -> Unit,
    onMarkMovieWatched: (String, Boolean) -> Unit,
    onMarkEpisodeWatched: (String, Boolean) -> Unit,
    onMarkSeriesWatched: (Long, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    BackHandler(onBack = onBack)
    TvScaffold(
        modifier = Modifier.testTag(FerrexQaTags.Tv.Detail),
        contentMaxWidth = FerrexDesignTokens.Tv.DetailMaxWidth,
        horizontalPadding = FerrexDesignTokens.Space.ScreenTvHorizontal,
        verticalPadding = FerrexDesignTokens.Tv.DetailVerticalPadding,
        verticalArrangement = Arrangement.Top,
        scrollable = true,
    ) {
        TvButtonRow(
            actions = buildList {
                add(TvButtonAction("back", "Back", TvActionRole.Back, onSelect = onBack))
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
            },
            surfaceKey = "detail-back",
            autoFocus = true,
        )
        if (connectionStatus.visible) {
            TvStateCopy(connectionStatus.title, connectionStatus.message)
        }
        playbackNotice?.let {
            Text(it, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary, textAlign = TextAlign.Center)
        }
        Spacer(Modifier.height(FerrexDesignTokens.Space.Lg))
        when (val result = detailResult) {
            is DetailLoadResult.Movie -> TvMovieDetail(
                result = result,
                watchState = watchState,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
                networkActionsEnabled = connectionStatus.networkActionsEnabled,
                networkActionMessage = connectionStatus.networkActionMessage,
                onRetryWatch = onRetryWatch,
                onClearProgress = onClearProgress,
                onMarkMovieWatched = onMarkMovieWatched,
                onPlaybackContract = onPlaybackContract,
            )
            is DetailLoadResult.Series -> TvSeriesDetail(
                result = result,
                watchState = watchState,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
                networkActionsEnabled = connectionStatus.networkActionsEnabled,
                networkActionMessage = connectionStatus.networkActionMessage,
                onRetryWatch = onRetryWatch,
                onMarkSeriesWatched = onMarkSeriesWatched,
                onPlaybackContract = onPlaybackContract,
            )
            is DetailLoadResult.Season -> TvSeasonDetail(
                result = result,
                watchState = watchState,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
                networkActionsEnabled = connectionStatus.networkActionsEnabled,
                networkActionMessage = connectionStatus.networkActionMessage,
                onRetryWatch = onRetryWatch,
                onMarkSeriesWatched = onMarkSeriesWatched,
                onPlaybackContract = onPlaybackContract,
            )
            is DetailLoadResult.Episode -> TvEpisodeDetail(
                result = result,
                watchState = watchState,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
                networkActionsEnabled = connectionStatus.networkActionsEnabled,
                networkActionMessage = connectionStatus.networkActionMessage,
                onClearProgress = onClearProgress,
                onMarkEpisodeWatched = onMarkEpisodeWatched,
                onPlaybackContract = onPlaybackContract,
            )
            is DetailLoadResult.Missing -> TvDetailRecovery(
                title = result.title,
                message = result.message,
                canClearSelected = result.selectedLibraryId != null,
                onRetryCacheSync = onRetryCacheSync,
                onClearSelectedCache = onClearSelectedCache,
                onChangeServer = onChangeServer,
                onResetConnection = onResetConnection,
                onOpenDiagnostics = onOpenDiagnostics,
            )
            null -> TvActionPanel(
                title = "Details loading",
                supportingText = "Library cache is resolving the selected media.",
                actions = listOf(TvActionPanelAction("retry-cache", "Retry cache sync", TvActionRole.Retry, onSelect = onRetryCacheSync)),
                autoFocus = false,
            )
        }
    }
}

@Composable
private fun TvMovieDetail(
    result: DetailLoadResult.Movie,
    watchState: WatchRepositoryState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
    onRetryWatch: () -> Unit,
    onClearProgress: (String) -> Unit,
    onMarkMovieWatched: (String, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    val movie = result.detail
    val progress = watchState.mediaProgress(movie.id)
    TvDetailArtwork(movie.images.backdrop ?: movie.images.poster, movie.title, imageResolutions, imageLoader, scope)
    TvDetailTitle(movie.title, listOfNotNull(movie.releaseDate?.take(4), movie.runtimeMinutes?.let { "$it min" }, movie.contentRating, movie.voteAverage?.let { "★ ${"%.1f".format(it)}" }).joinToString(" • "), movie.overview)
    TvStateCopy("Movie watch state", progress?.let { if (it.isCompleted) "Watched" else if (it.isStarted) "${(it.progressRatio * 100).toInt()}% watched" else "Unwatched" } ?: (watchState.lastError ?: "Watch state has not loaded yet."))
    TvNetworkActionStatus(networkActionsEnabled, networkActionMessage)
    TvActionPanel(
        title = "Playback and watch actions",
        actions = buildList {
            DetailRouteContracts.movieResume(movie, progress, result.route)?.let { contract ->
                add(TvActionPanelAction("resume", "Resume", TvActionRole.Primary, enabled = networkActionsEnabled, onSelect = { onPlaybackContract(contract) }))
            }
            DetailRouteContracts.movieStartOver(movie, result.route)?.let { contract ->
                add(TvActionPanelAction("start-over", "Start over", TvActionRole.Primary, enabled = networkActionsEnabled, onSelect = { onPlaybackContract(contract) }))
            }
            add(TvActionPanelAction("retry-watch", "Retry watch state", TvActionRole.Retry, enabled = networkActionsEnabled, onSelect = onRetryWatch))
            if (progress?.hasServerState == true) {
                add(TvActionPanelAction("clear-progress", "Clear progress", TvActionRole.Cache, enabled = progress.pendingMutation.not() && networkActionsEnabled, onSelect = { onClearProgress(movie.id) }))
            }
            add(
                TvActionPanelAction(
                    key = "toggle-watched",
                    label = if (progress?.isCompleted == true) "Mark unwatched" else "Mark watched",
                    role = TvActionRole.Cache,
                    enabled = progress?.pendingMutation != true && networkActionsEnabled,
                    onSelect = { onMarkMovieWatched(movie.id, progress?.isCompleted != true) },
                ),
            )
        },
        autoFocus = false,
    )
}

@Composable
private fun TvSeriesDetail(
    result: DetailLoadResult.Series,
    watchState: WatchRepositoryState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
    onRetryWatch: () -> Unit,
    onMarkSeriesWatched: (Long, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    val detail = result.detail
    val series = detail.series
    val seriesStatus = watchState.seriesStatus(series.tmdbId)
    TvDetailArtwork(series.images.backdrop ?: series.images.poster, series.title, imageResolutions, imageLoader, scope)
    TvDetailTitle(
        title = series.title,
        subtitle = listOfNotNull(series.firstAirDate?.take(4), series.availableSeasons?.let { "$it season(s)" }, series.availableEpisodes?.let { "$it episode(s)" }, series.contentRating).joinToString(" • "),
        body = series.overview,
    )
    when (val availability = detail.episodesAvailability) {
        is EpisodesAvailability.Available -> TvStateCopy("Episodes ready", "${availability.episodeCount} cached episode(s) parsed from the current series bundle.")
        is EpisodesAvailability.Unavailable -> TvStateCopy("Episodes unavailable", availability.message)
    }
    TvStateCopy(
        title = "Series watch state",
        body = seriesStatus?.let { "${it.watched} of ${it.totalEpisodes} watched; ${it.inProgress} in progress." }
            ?: (watchState.lastError ?: "Retry to load series watch state and next episode."),
    )
    TvNetworkActionStatus(networkActionsEnabled, networkActionMessage)
    TvActionPanel(
        title = "Playback and watch actions",
        actions = buildList {
            DetailRouteContracts.seriesNext(detail, seriesStatus?.nextEpisode ?: series.tmdbId?.let { watchState.nextEpisodes[it] }, result.route)?.let { contract ->
                add(TvActionPanelAction("play-next", "Play next", TvActionRole.Primary, enabled = networkActionsEnabled, onSelect = { onPlaybackContract(contract) }))
            }
            DetailRouteContracts.seriesStartOver(detail, result.route)?.let { contract ->
                add(TvActionPanelAction("start-over", "Start over", TvActionRole.Primary, enabled = networkActionsEnabled, onSelect = { onPlaybackContract(contract) }))
            }
            add(TvActionPanelAction("retry-watch", "Retry watch state", TvActionRole.Retry, enabled = networkActionsEnabled, onSelect = onRetryWatch))
            series.tmdbId?.let { tmdbId ->
                add(
                    TvActionPanelAction(
                        key = "toggle-series-watched",
                        label = if (seriesStatus?.isCompleted == true) "Mark series unwatched" else "Mark series watched",
                        role = TvActionRole.Cache,
                        enabled = seriesStatus?.pendingMutation != true && networkActionsEnabled,
                        onSelect = { onMarkSeriesWatched(tmdbId, seriesStatus?.isCompleted != true) },
                    ),
                )
            }
        },
        autoFocus = false,
    )
    if (detail.episodes.isNotEmpty()) {
        Text("Episodes", style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.primary)
        detail.episodes.take(12).forEach { episode ->
            Text(
                text = "S${episode.seasonNumber} E${episode.episodeNumber}: ${episode.title}",
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (detail.episodes.size > 12) {
            Text("Showing 12 of ${detail.episodes.size} episode labels on TV detail; playback actions remain available above.", style = MaterialTheme.typography.bodyLarge)
        }
    }
}

@Composable
private fun TvSeasonDetail(
    result: DetailLoadResult.Season,
    watchState: WatchRepositoryState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
    onRetryWatch: () -> Unit,
    onMarkSeriesWatched: (Long, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    val season = result.season
    val series = result.series
    val seriesStatus = watchState.seriesStatus(series?.tmdbId)
    val seasonStatus = seriesStatus?.seasons?.get(season.seasonNumber)
    val firstPlayableEpisode = result.episodes.firstOrNull { it.playbackTargetId != null }
    TvDetailArtwork(season.images.poster ?: series?.images?.poster ?: series?.images?.backdrop, season.title, imageResolutions, imageLoader, scope)
    TvDetailTitle(
        title = season.title,
        subtitle = listOfNotNull(series?.title, season.episodeCount?.let { "$it episode(s)" }, season.airDate?.take(4)).joinToString(" • "),
        body = season.overview,
    )
    TvStateCopy(
        title = "Season watch state",
        body = seasonStatus?.let { "${it.watched} of ${it.total} watched; ${it.inProgress} in progress." }
            ?: (watchState.lastError ?: "Retry to load series watch state for this season."),
    )
    TvNetworkActionStatus(networkActionsEnabled, networkActionMessage)
    TvActionPanel(
        title = "Playback and watch actions",
        actions = buildList {
            result.episodes.firstNotNullOfOrNull { episode ->
                DetailRouteContracts.episodeResume(episode, watchState.mediaProgress(episode.id), result.route)
            }?.let { contract ->
                add(TvActionPanelAction("resume-season", "Resume season", TvActionRole.Primary, enabled = networkActionsEnabled, onSelect = { onPlaybackContract(contract) }))
            }
            firstPlayableEpisode?.let { episode ->
                DetailRouteContracts.episodeStartOver(episode, result.route)?.let { contract ->
                    add(TvActionPanelAction("play-season", "Play season", TvActionRole.Primary, enabled = networkActionsEnabled, onSelect = { onPlaybackContract(contract) }))
                }
            }
            add(TvActionPanelAction("retry-watch", "Retry watch state", TvActionRole.Retry, enabled = networkActionsEnabled, onSelect = onRetryWatch))
            series?.tmdbId?.let { tmdbId ->
                add(
                    TvActionPanelAction(
                        key = "toggle-season-watched",
                        label = if (seasonStatus?.isCompleted == true) "Mark series unwatched" else "Mark series watched",
                        role = TvActionRole.Cache,
                        enabled = seriesStatus?.pendingMutation != true && networkActionsEnabled,
                        onSelect = { onMarkSeriesWatched(tmdbId, seasonStatus?.isCompleted != true) },
                    ),
                )
            }
        },
        autoFocus = false,
    )
    if (result.episodes.isNotEmpty()) {
        Text("Episodes", style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.primary)
        result.episodes.take(12).forEach { episode ->
            Text(
                text = "S${episode.seasonNumber} E${episode.episodeNumber}: ${episode.title}",
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun TvEpisodeDetail(
    result: DetailLoadResult.Episode,
    watchState: WatchRepositoryState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
    onClearProgress: (String) -> Unit,
    onMarkEpisodeWatched: (String, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    val episode = result.detail
    val progress = watchState.mediaProgress(episode.id)
    TvDetailArtwork(episode.images.still ?: episode.images.poster, episode.title, imageResolutions, imageLoader, scope)
    TvDetailTitle(
        title = episode.title,
        subtitle = listOfNotNull(result.parentSeries?.title, "S${episode.seasonNumber} E${episode.episodeNumber}", episode.runtimeMinutes?.let { "$it min" }).joinToString(" • "),
        body = episode.overview,
    )
    TvStateCopy("Episode watch state", progress?.let { if (it.isCompleted) "Watched" else if (it.isStarted) "${(it.progressRatio * 100).toInt()}% watched" else "Unwatched" } ?: (watchState.lastError ?: "Watch state has not loaded yet."))
    TvNetworkActionStatus(networkActionsEnabled, networkActionMessage)
    TvActionPanel(
        title = "Playback and watch actions",
        actions = buildList {
            DetailRouteContracts.episodeResume(episode, progress, result.route)?.let { contract ->
                add(TvActionPanelAction("resume", "Resume", TvActionRole.Primary, enabled = networkActionsEnabled, onSelect = { onPlaybackContract(contract) }))
            }
            DetailRouteContracts.episodeStartOver(episode, result.route)?.let { contract ->
                add(TvActionPanelAction("start-over", "Start over", TvActionRole.Primary, enabled = networkActionsEnabled, onSelect = { onPlaybackContract(contract) }))
            }
            if (progress?.hasServerState == true) {
                add(TvActionPanelAction("clear-progress", "Clear progress", TvActionRole.Cache, enabled = progress.pendingMutation.not() && networkActionsEnabled, onSelect = { onClearProgress(episode.id) }))
            }
            add(
                TvActionPanelAction(
                    key = "toggle-watched",
                    label = if (progress?.isCompleted == true) "Mark unwatched" else "Mark watched",
                    role = TvActionRole.Cache,
                    enabled = progress?.pendingMutation != true && networkActionsEnabled,
                    onSelect = { onMarkEpisodeWatched(episode.id, progress?.isCompleted != true) },
                ),
            )
        },
        autoFocus = false,
    )
}

@Composable
private fun TvNetworkActionStatus(
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
) {
    if (!networkActionsEnabled && networkActionMessage != null) {
        TvStateCopy("Playback and watch updates paused", networkActionMessage)
    }
}

@Composable
private fun TvDetailRecovery(
    title: String,
    message: String,
    canClearSelected: Boolean,
    onRetryCacheSync: () -> Unit,
    onClearSelectedCache: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    TvActionPanel(
        title = title,
        supportingText = message,
        actions = buildList {
            add(TvActionPanelAction("retry-cache", "Retry cache sync", TvActionRole.Retry, onSelect = onRetryCacheSync))
            if (canClearSelected) {
                add(TvActionPanelAction("clear-selected", "Clear selected cache", TvActionRole.Cache, onSelect = onClearSelectedCache))
            }
            add(TvActionPanelAction("change-server", "Change server", TvActionRole.SettingsExit, onSelect = onChangeServer))
            add(TvActionPanelAction("reset-connection", "Reset connection", TvActionRole.Destructive, onSelect = onResetConnection))
            add(TvActionPanelAction("diagnostics", "Diagnostics / Export diagnostics", TvActionRole.SettingsExit, onSelect = onOpenDiagnostics))
        },
        autoFocus = false,
    )
}

@Composable
private fun TvDetailArtwork(
    imageKey: ImageRequestKey?,
    title: String,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(
                min = FerrexDesignTokens.Tv.DetailArtworkMinHeight,
                max = FerrexDesignTokens.Tv.DetailArtworkMaxHeight,
            ),
        contentAlignment = Alignment.Center,
    ) {
        if (imageKey == null || imageLoader == null) {
            PosterPlaceholder(if (imageKey == null) "No image" else "Images unavailable")
        } else {
            val resolution = imageResolutions[imageKey]
            FerrexAsyncImage(
                resolution = resolution,
                imageLoader = imageLoader,
                contentDescription = title,
                category = imageKey.category,
                fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
                    runtimeFallback(scope.canonicalServerUrl, imageKey, null)
                } else {
                    null
                },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun TvDetailTitle(title: String, subtitle: String, body: String?) {
    Text(title, style = MaterialTheme.typography.displaySmall, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center)
    if (subtitle.isNotBlank()) Text(subtitle, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
    body?.let { Text(it, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center) }
}

@Composable
private fun TvPosterRow(
    title: String?,
    supportingText: String,
    entries: List<TvPosterEntry>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRestorer: TvFocusRestorer,
    surfaceKey: String,
    autoFocus: Boolean,
    onSelect: (TvPosterEntry) -> Unit,
) {
    if (entries.isEmpty()) return
    title?.let { TvSectionHeader(it) }
    Text(supportingText, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
    val railItems = remember(surfaceKey, entries) {
        entries.zip(
            MediaRailIdentityResolver.assign(
                railKey = surfaceKey,
                stableIds = entries.map { it.stableKey },
            ),
        ).map { (entry, identity) -> TvRailPosterItem(entry, identity) }
    }
    val keys = railItems.map { it.identity.renderKey }
    val requesters = remember(keys) { keys.associateWith { FocusRequester() } }
    val restoredKey = keys.firstOrNull()?.let { fallback ->
        focusRestorer.restoreItem(surfaceKey, keys, fallback)
    }
    LaunchedEffect(autoFocus, restoredKey, keys) {
        if (autoFocus && restoredKey != null) {
            runCatching { requesters[restoredKey]?.requestFocus() }
        }
    }
    LazyRow(
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
        contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Lg),
        modifier = Modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Tv.surface(surfaceKey))
            .focusGroup(),
    ) {
        items(railItems, key = { it.identity.focusKey }) { railItem ->
            val entry = railItem.entry
            val itemKey = railItem.identity.renderKey
            TvPosterCard(
                entry = entry,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
                focusRequester = requesters[itemKey],
                semanticLabel = railItem.identity.semanticLabel(entry.title),
                onFocused = { focusRestorer.record(surfaceKey, itemKey) },
                onSelect = { onSelect(entry) },
                modifier = Modifier.width(FerrexDesignTokens.Poster.TvWidth),
                testTag = FerrexQaTags.Tv.poster(surfaceKey, itemKey),
            )
        }
    }
}

@Composable
private fun TvPosterCard(
    entry: TvPosterEntry,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRequester: FocusRequester?,
    semanticLabel: String = entry.title,
    onFocused: () -> Unit,
    onSelect: () -> Unit,
    modifier: Modifier = Modifier,
    testTag: String? = null,
) {
    TvFocusableSurface(
        onClick = onSelect,
        semanticLabel = semanticLabel,
        modifier = modifier,
        focusRequester = focusRequester,
        minHeight = FerrexDesignTokens.Poster.TvCardMinHeight,
        testTag = testTag,
        onFocused = onFocused,
    ) {
        Column(modifier = Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            Poster(
                imageKey = entry.imageKey,
                title = entry.title,
                fallbackPath = entry.publicFallbackPath,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
            )
            Text(entry.title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(entry.subtitle, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            entry.tertiary?.let { Text(it, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary) }
        }
    }
}

@Composable
private fun Poster(
    imageKey: ImageRequestKey?,
    title: String,
    fallbackPath: String?,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
) {
    if (imageKey == null || imageLoader == null) {
        PosterPlaceholder(if (imageKey == null) "No poster" else "Images unavailable")
        return
    }
    val resolution = imageResolutions[imageKey]
    FerrexAsyncImage(
        resolution = resolution,
        imageLoader = imageLoader,
        contentDescription = title,
        category = imageKey.category,
        fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
            runtimeFallback(scope.canonicalServerUrl, imageKey, fallbackPath)
        } else {
            null
        },
    )
}

@Composable
private fun PosterPlaceholder(label: String) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(FerrexDesignTokens.Poster.AspectRatio)
            .background(FerrexDesignTokens.Palette.PosterFallback, FerrexDesignTokens.Shapes.PosterImage),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
    }
}

@Composable
private fun TvButtonRow(
    actions: List<TvButtonAction>,
    modifier: Modifier = Modifier,
    title: String? = null,
    supportingText: String? = null,
    focusRestorer: TvFocusRestorer? = null,
    surfaceKey: String = "actions",
    autoFocus: Boolean = false,
) {
    if (actions.isEmpty()) return
    val keys = actions.map { it.key }
    val requesters = remember(keys) { actions.associate { it.key to FocusRequester() } }
    val enabledKeys = actions.filter { it.enabled }.map { it.key }
    val restoredKey = enabledKeys.firstOrNull()?.let { fallback ->
        focusRestorer?.restoreItem(surfaceKey, enabledKeys, fallback) ?: fallback
    }
    LaunchedEffect(autoFocus, restoredKey, keys) {
        if (autoFocus && restoredKey != null) {
            runCatching { requesters[restoredKey]?.requestFocus() }
        }
    }
    Column(
        modifier = modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Tv.surface(surfaceKey)),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
    ) {
        title?.let { TvSectionHeader(it) }
        supportingText?.let { Text(it, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            actions.forEach { action ->
                TvFocusableButton(
                    label = action.label,
                    onClick = action.onSelect,
                    enabled = action.enabled,
                    style = action.role.toFocusableStyle(),
                    tone = action.role.sharedActionRole.statusTone(),
                    focusRequester = requesters[action.key],
                    testTag = FerrexQaTags.Tv.action(surfaceKey, action.key),
                    onFocused = { focusRestorer?.record(surfaceKey, action.key) },
                    modifier = Modifier.widthIn(
                        min = FerrexDesignTokens.Tv.ActionMinWidth,
                        max = FerrexDesignTokens.Tv.ActionMaxWidth,
                    ),
                )
            }
        }
    }
}

@Composable
private fun TvStateCopy(title: String, body: String, loading: Boolean = false) {
    FerrexStatusCard(
        title = title,
        body = body,
        loading = loading,
        tone = if (title.contains("failed", ignoreCase = true) || title.contains("unavailable", ignoreCase = true)) {
            FerrexStatusTone.Error
        } else {
            FerrexStatusTone.Secondary
        },
    )
}

@Composable
private fun TvSectionHeader(title: String) {
    Text(title, style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
}

@Composable
private fun TvFullScreenSurface(content: @Composable BoxScope.() -> Unit) {
    Surface(
        modifier = Modifier.fillMaxSize().background(FerrexDesignTokens.Palette.SlateCanvas),
        color = MaterialTheme.colorScheme.background,
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(FerrexDesignTokens.privateCinemaGradient())
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(
                    horizontal = FerrexDesignTokens.Space.ScreenTvHorizontal,
                    vertical = FerrexDesignTokens.Tv.FullScreenVerticalPadding,
                ),
            content = content,
        )
    }
}

private fun TvReturnTarget.toChild(connectionHealth: AuthConnectionHealth): TvHomeChild? = when (
    AuthenticatedHomeBackPolicy.detailBackDestination(connectionHealth, toDetailBackDestination())
) {
    AuthenticatedDetailBackDestination.Home -> null
    AuthenticatedDetailBackDestination.Search -> TvHomeChild.Search
    AuthenticatedDetailBackDestination.MovieGrid -> TvHomeChild.Grid(HomeLibraryTab.Movies)
    AuthenticatedDetailBackDestination.SeriesGrid -> TvHomeChild.Grid(HomeLibraryTab.Series)
}

private fun TvReturnTarget.toDetailBackDestination(): AuthenticatedDetailBackDestination = when (this) {
    TvReturnTarget.Home -> AuthenticatedDetailBackDestination.Home
    TvReturnTarget.Search -> AuthenticatedDetailBackDestination.Search
    is TvReturnTarget.Grid -> when (tab) {
        HomeLibraryTab.Movies -> AuthenticatedDetailBackDestination.MovieGrid
        HomeLibraryTab.Series -> AuthenticatedDetailBackDestination.SeriesGrid
    }
}

private fun TvActionRole.toFocusableStyle(): TvFocusableStyle = when (this) {
    TvActionRole.Primary,
    TvActionRole.Retry -> TvFocusableStyle.Primary
    TvActionRole.Destructive -> TvFocusableStyle.Destructive
    TvActionRole.Back,
    TvActionRole.Cache,
    TvActionRole.Recovery,
    TvActionRole.SettingsExit -> TvFocusableStyle.Secondary
}

private fun ContinueWatchingCard.toPosterEntry(): TvPosterEntry = TvPosterEntry(
    stableKey = stableKey,
    title = title,
    subtitle = subtitle,
    tertiary = progressLabel,
    imageKey = imageKey,
    publicFallbackPath = null,
    route = route,
)

private fun LibraryMediaCard.toPosterEntry(): TvPosterEntry = TvPosterEntry(
    stableKey = stableKey,
    title = title,
    subtitle = subtitle,
    tertiary = libraryName,
    imageKey = imageKey,
    publicFallbackPath = publicFallbackPath,
    route = route,
)

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

private fun SearchResultRow.searchStableKey(): String = when (this) {
    is SearchResultRow.Resolved -> "resolved:${sourceId.type.routeSegment}:${sourceId.id}:${libraryId}"
    is SearchResultRow.CacheMiss -> "miss:${sourceId.type.routeSegment}:${sourceId.id}"
}

private fun shelfSurfaceKey(shelf: com.ferrex.android.core.browse.HomeShelf): String =
    "shelf-${shelf.title.lowercase().replace(Regex("[^a-z0-9]+"), "-").trim('-')}"

private fun runtimeFallback(
    serverUrl: String,
    key: ImageRequestKey,
    publicFallbackPath: String?,
): FerrexImageFallback? {
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

private sealed interface TvHomeChild {
    data object Search : TvHomeChild
    data class Grid(val tab: HomeLibraryTab) : TvHomeChild
    data class Detail(val route: MediaRouteArgs, val returnTo: TvReturnTarget) : TvHomeChild
}

private sealed interface TvReturnTarget {
    data object Home : TvReturnTarget
    data object Search : TvReturnTarget
    data class Grid(val tab: HomeLibraryTab) : TvReturnTarget
}

private data class TvPosterEntry(
    val stableKey: String,
    val title: String,
    val subtitle: String,
    val tertiary: String?,
    val imageKey: ImageRequestKey?,
    val publicFallbackPath: String?,
    val route: MediaRouteArgs?,
)

private data class TvRailPosterItem(
    val entry: TvPosterEntry,
    val identity: MediaRailItemIdentity,
)

private data class TvButtonAction(
    val key: String,
    val label: String,
    val role: TvActionRole,
    val enabled: Boolean = true,
    val onSelect: () -> Unit,
)

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

private sealed interface TvSearchUiState {
    data object Idle : TvSearchUiState
    data object KeepTyping : TvSearchUiState
    data object Unavailable : TvSearchUiState
    data class Loading(val query: String) : TvSearchUiState
    data class Loaded(val outcome: MediaSearchOutcome) : TvSearchUiState
}

private const val GRID_IMAGE_LOOKUP_LIMIT = 96
private const val SEARCH_IMAGE_LOOKUP_LIMIT = 48
private const val SEARCH_RESULT_DISPLAY_LIMIT = 20
private const val SEARCH_DEBOUNCE_MILLIS = 350L
private const val PRODUCT_COPY_ALLOWS_PUBLIC_CDN_IMAGES = false
