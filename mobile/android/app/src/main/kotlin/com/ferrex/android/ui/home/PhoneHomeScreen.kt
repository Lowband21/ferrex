package com.ferrex.android.ui.home

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
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
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import com.ferrex.android.core.browse.PhoneExplicitBackAction
import com.ferrex.android.core.browse.PhoneShellDestination
import com.ferrex.android.core.browse.PhoneSystemBackAction
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.detail.DetailCache
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.image.TmdbImageFallbackPolicy
import com.ferrex.android.core.mediaart.MediaRailIdentityResolver
import com.ferrex.android.core.playback.PlaybackLaunchDecision
import com.ferrex.android.core.playback.PlaybackLaunchPolicy
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
import com.ferrex.android.core.theaterplate.TheaterPlateAnalyzer
import com.ferrex.android.core.theaterplate.TheaterPlateColor
import com.ferrex.android.core.theaterplate.TheaterPlateImageSource
import com.ferrex.android.core.theaterplate.TheaterPlateImageSourceKind
import com.ferrex.android.core.theaterplate.TheaterPlateSourceContext
import com.ferrex.android.core.theaterplate.TheaterPlateViewport
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
import com.ferrex.android.ui.components.TheaterPlateDensityRole
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.detail.PhoneDetailScreen
import com.ferrex.android.ui.player.PlayerScreen
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.search.PhoneSearchPanel
import com.ferrex.android.ui.theaterplate.FerrexStageDensityFamily
import com.ferrex.android.ui.theaterplate.FerrexStageSurface
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceTone
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceVariant
import com.ferrex.android.ui.theaterplate.TheaterPlateBackdropAdaptation
import com.ferrex.android.ui.theaterplate.TheaterPlateStage
import com.ferrex.android.ui.theaterplate.tokens
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
    var selectedDestination by remember { mutableStateOf(PhoneShellDestination.Home) }
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

    LaunchedEffect(selectedDestination, selectedTab, selectedMovieLibrary?.library?.id, selectedMovieLibrary?.accessor, movieSort, movieFilter, libraryIndexTransport, state.connectionHealth) {
        if (selectedDestination != PhoneShellDestination.Libraries || selectedTab != HomeLibraryTab.Movies || selectedMovieLibrary == null) {
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
    val imageKeys = remember(continueState, shelves, indexedMovieCards, selectedSeriesCards, selectedTab, selectedDestination, detailResult) {
        buildList {
            continueState.cards.mapNotNullTo(this) { it.imageKey }
            shelves.flatMap { it.items }.mapNotNullTo(this) { it.imageKey }
            if (selectedDestination == PhoneShellDestination.Libraries) {
                val gridCards = if (selectedTab == HomeLibraryTab.Movies) indexedMovieCards.cards else selectedSeriesCards
                gridCards.take(GRID_IMAGE_LOOKUP_LIMIT).mapNotNullTo(this) { it.imageKey }
            }
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
                com.ferrex.android.core.browse.BrowseMediaType.Season,
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
            val decision = PlaybackLaunchPolicy.phone(
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

    val selectedBrowseLibraryId = when (selectedTab) {
        HomeLibraryTab.Movies -> selectedMovieInfo?.id
        HomeLibraryTab.Series -> selectedSeriesInfo?.id
    }

    fun syncSelectedLibrary() {
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
    }

    fun retryLibrary() {
        coroutineScope.launch {
            libraryRepository?.refreshLibraries(scope, selectedBrowseLibraryId ?: repositoryState?.selectedLibraryId)
        }
    }

    fun clearSelectedLibraryCache() {
        selectedBrowseLibraryId?.let { libraryRepository?.clearSelectedCache(scope, it) }
    }

    fun openSearchResult(target: SearchDetailTarget) {
        target.toMediaRouteArgs()?.let { selectedDetailRoute = it }
    }

    fun handleExplicitBack() {
        when (AuthenticatedHomeBackPolicy.phoneExplicitBackAction(activePlaybackContract != null, selectedDetailRoute != null)) {
            PhoneExplicitBackAction.CloseDiagnostics -> Unit
            PhoneExplicitBackAction.ClosePlayback -> activePlaybackContract = null
            PhoneExplicitBackAction.CloseDetail -> selectedDetailRoute = null
            PhoneExplicitBackAction.StayOnSurface -> Unit
        }
    }

    val phoneBackAction = AuthenticatedHomeBackPolicy.phoneSystemBackAction(
        hasActivePlayback = activePlaybackContract != null,
        hasSelectedDetail = selectedDetailRoute != null,
        currentDestination = selectedDestination,
    )
    BackHandler(enabled = phoneBackAction != PhoneSystemBackAction.ExitApp) {
        when (phoneBackAction) {
            PhoneSystemBackAction.CloseDiagnostics -> Unit
            PhoneSystemBackAction.ClosePlayback -> activePlaybackContract = null
            PhoneSystemBackAction.CloseDetail -> selectedDetailRoute = null
            PhoneSystemBackAction.ReturnHome -> selectedDestination = PhoneShellDestination.Home
            PhoneSystemBackAction.ExitApp -> Unit
        }
    }

    Surface(
        modifier = Modifier
            .fillMaxSize()
            .testTag(FerrexQaTags.Phone.Shell),
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
                onBack = { handleExplicitBack() },
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
                onBack = { handleExplicitBack() },
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
                libraryFreshness = repositoryState?.freshness,
                onOpenDetail = { selectedDetailRoute = it },
            )
        } else {
            AuthenticatedPhoneShell(
                selectedDestination = selectedDestination,
                onDestinationSelected = { selectedDestination = it },
            ) { contentPadding ->
                when (selectedDestination) {
                    PhoneShellDestination.Home -> HomeDestinationContent(
                        contentPadding = contentPadding,
                        state = state,
                        connectionStatus = homeConnectionUi,
                        playbackNotice = playbackNotice,
                        freshness = repositoryState?.freshness ?: LibraryFreshness.Empty,
                        continueState = continueState,
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoader != null,
                        imageLoader = imageLoader,
                        scope = scope,
                        shelves = shelves,
                        onRetryContinue = { coroutineScope.launch { continueWatchingRepository?.refresh() } },
                        onSelectContinue = { selectedDetailRoute = it.route },
                        onSelectShelf = { selectedDetailRoute = it.route },
                        onOpenLibraries = { selectedDestination = PhoneShellDestination.Libraries },
                        onOpenSearch = { selectedDestination = PhoneShellDestination.Search },
                        onOpenAccountServer = { selectedDestination = PhoneShellDestination.AccountServer },
                        onRetryConnection = onRetryConnection,
                        onOpenDiagnostics = onOpenDiagnostics,
                    )
                    PhoneShellDestination.Libraries -> LibraryDestinationContent(
                        contentPadding = contentPadding,
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
                        freshness = repositoryState?.freshness ?: LibraryFreshness.Empty,
                        selectedLibraryId = selectedBrowseLibraryId,
                        onSelect = { selectedDetailRoute = it.route },
                        onSyncSelected = { syncSelectedLibrary() },
                        onRetry = { retryLibrary() },
                        onClearSelected = { clearSelectedLibraryCache() },
                        onChangeServer = onChangeServer,
                        onResetConnection = onResetConnection,
                        onOpenDiagnostics = onOpenDiagnostics,
                    )
                    PhoneShellDestination.Search -> SearchDestinationContent(
                        contentPadding = contentPadding,
                        scope = scope,
                        searchRepository = searchRepository,
                        imageRepository = imageRepository,
                        imagePipeline = imagePipeline,
                        connectionStatus = homeConnectionUi,
                        onOpenResult = { openSearchResult(it) },
                        onRetryConnection = onRetryConnection,
                        onOpenAccountServer = { selectedDestination = PhoneShellDestination.AccountServer },
                        onOpenDiagnostics = onOpenDiagnostics,
                    )
                    PhoneShellDestination.AccountServer -> AccountServerDestinationContent(
                        contentPadding = contentPadding,
                        state = state,
                        connectionStatus = homeConnectionUi,
                        freshness = repositoryState?.freshness ?: LibraryFreshness.Empty,
                        selectedLibraryId = selectedBrowseLibraryId,
                        onRetryConnection = onRetryConnection,
                        onRetryLibrary = { retryLibrary() },
                        onClearSelected = { clearSelectedLibraryCache() },
                        onSignOut = onSignOut,
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
private fun AuthenticatedPhoneShell(
    selectedDestination: PhoneShellDestination,
    onDestinationSelected: (PhoneShellDestination) -> Unit,
    content: @Composable (PaddingValues) -> Unit,
) {
    Scaffold(
        modifier = Modifier
            .fillMaxSize()
            .testTag(FerrexQaTags.Phone.Shell),
        bottomBar = {
            NavigationBar(modifier = Modifier.testTag(FerrexQaTags.Phone.ShellNav)) {
                PhoneShellDestination.entries.forEach { destination ->
                    NavigationBarItem(
                        selected = selectedDestination == destination,
                        onClick = { onDestinationSelected(destination) },
                        icon = { Text(destination.navIcon(), style = MaterialTheme.typography.labelMedium) },
                        label = { Text(destination.label) },
                        modifier = Modifier.testTag(FerrexQaTags.Phone.navItem(destination.name)),
                    )
                }
            }
        },
        content = content,
    )
}

@Composable
private fun HomeDestinationContent(
    contentPadding: PaddingValues,
    state: SessionState.Authenticated,
    connectionStatus: AuthenticatedConnectionUi,
    playbackNotice: String?,
    freshness: LibraryFreshness,
    continueState: ContinueWatchingState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    shelves: List<com.ferrex.android.core.browse.HomeShelf>,
    onRetryContinue: () -> Unit,
    onSelectContinue: (ContinueWatchingCard) -> Unit,
    onSelectShelf: (LibraryMediaCard) -> Unit,
    onOpenLibraries: () -> Unit,
    onOpenSearch: () -> Unit,
    onOpenAccountServer: () -> Unit,
    onRetryConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val analyzer = remember { TheaterPlateAnalyzer() }
    BoxWithConstraints(
        modifier = Modifier
            .fillMaxSize()
            .padding(contentPadding),
    ) {
        val viewport = remember(maxWidth, maxHeight) {
            TheaterPlateViewport.fromLogicalSize(maxWidth.value, maxHeight.value)
        }
        val density = remember(viewport) { FerrexStageDensityFamily.forViewport(viewport) }
        val stageContext = remember(viewport, connectionStatus.health, freshness.label) {
            homeStageSourceContext(viewport, connectionStatus, freshness)
        }
        val stageAnalysis = remember(analyzer, stageContext) { analyzer.analyzeMissingBackdrop(stageContext) }
        val adaptation = remember(connectionStatus.health, freshness.label) {
            if (homeStageHasStaleOrOfflineState(connectionStatus, freshness)) {
                TheaterPlateBackdropAdaptation.StaleOffline
            } else {
                TheaterPlateBackdropAdaptation.MissingBackdrop
            }
        }

        TheaterPlateStage(
            analysis = stageAnalysis,
            adaptation = adaptation,
            density = density,
            modifier = Modifier
                .fillMaxSize()
                .testTag(FerrexQaTags.Phone.Home),
            contentDescription = "Phone Home Theater Plate stage with generated fallback backdrop analysis and no-wipe recovery paths",
        ) {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(density.tokens().contentGap),
            ) {
                item {
                    HomeHeader(
                        state = state,
                        connectionStatus = connectionStatus,
                        playbackNotice = playbackNotice,
                        density = density,
                    )
                }
                item {
                    HomeStatusBands(
                        connectionStatus = connectionStatus,
                        freshness = freshness,
                        density = density,
                        onRetryConnection = onRetryConnection,
                    )
                }
                item {
                    ContinueWatchingSection(
                        continueState = continueState,
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoaderAvailable,
                        imageLoader = imageLoader,
                        scope = scope,
                        density = density,
                        onRetry = onRetryContinue,
                        onSelect = onSelectContinue,
                    )
                }
                if (shelves.isNotEmpty()) {
                    items(shelves, key = { it.title }) { shelf ->
                        HomeShelfSection(
                            shelf = shelf,
                            imageResolutions = imageResolutions,
                            imageLoaderAvailable = imageLoaderAvailable,
                            imageLoader = imageLoader,
                            scope = scope,
                            density = density,
                            onSelect = onSelectShelf,
                        )
                    }
                } else {
                    item {
                        HomeStageStatusBand(
                            title = "Local shelves are waiting for cached datasets",
                            body = "Home shelves are built only from cached complete movie batches and series bundles; no backend discovery shelves are shown here.",
                            density = density,
                            variant = FerrexStageSurfaceVariant.EmptyState,
                            tone = FerrexStatusTone.StaleOffline,
                        )
                    }
                }
                item {
                    HomeEntrySection(
                        density = density,
                        onOpenLibraries = onOpenLibraries,
                        onOpenSearch = onOpenSearch,
                    )
                }
                item {
                    HomeUtilityPanel(
                        state = state,
                        connectionStatus = connectionStatus,
                        freshness = freshness,
                        density = density,
                        onRetryConnection = onRetryConnection,
                        onOpenAccountServer = onOpenAccountServer,
                        onOpenDiagnostics = onOpenDiagnostics,
                    )
                }
            }
        }
    }
}

@Composable
private fun HomeStatusBands(
    connectionStatus: AuthenticatedConnectionUi,
    freshness: LibraryFreshness,
    density: FerrexStageDensityFamily,
    onRetryConnection: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
        if (connectionStatus.visible) {
            HomeStageStatusBand(
                title = connectionStatus.title,
                body = connectionStatus.message,
                density = density,
                tone = if (connectionStatus.retryEnabled) FerrexStatusTone.Retry else FerrexStatusTone.StaleOffline,
                action = if (connectionStatus.retryEnabled) {
                    HomeStageAction(
                        label = connectionStatus.retryLabel,
                        role = FerrexActionRole.Retry,
                        onClick = onRetryConnection,
                    )
                } else {
                    null
                },
            )
        } else {
            HomeStageStatusBand(
                title = "Online",
                body = "Home remains cache-aware; library and search entry points stay available even when cache refreshes later need recovery.",
                density = density,
                variant = FerrexStageSurfaceVariant.FactRibbon,
                tone = FerrexStatusTone.Secondary,
            )
        }

        val cacheStatus = LibraryBrowseModels.libraryStatusCopy(freshness)
        HomeStageStatusBand(
            title = "Library cache • ${cacheStatus.title}",
            body = cacheStatus.detail,
            density = density,
            variant = if (homeStageHasCacheRecoveryState(freshness)) {
                FerrexStageSurfaceVariant.StatusSlab
            } else {
                FerrexStageSurfaceVariant.FactRibbon
            },
            tone = freshness.statusTone(),
        )
    }
}

private data class HomeStageAction(
    val label: String,
    val role: FerrexActionRole,
    val onClick: () -> Unit,
)

@Composable
private fun HomeStageStatusBand(
    title: String,
    body: String,
    density: FerrexStageDensityFamily,
    modifier: Modifier = Modifier,
    variant: FerrexStageSurfaceVariant = FerrexStageSurfaceVariant.StatusSlab,
    tone: FerrexStatusTone = FerrexStatusTone.Secondary,
    loading: Boolean = false,
    action: HomeStageAction? = null,
    contentDescription: String = title,
) {
    val typographyDensity = density.toTheaterPlateDensityRole()
    FerrexStageSurface(
        variant = variant,
        density = density,
        tone = tone.toStageSurfaceTone(),
        modifier = modifier.fillMaxWidth(),
        contentDescription = contentDescription,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
            ) {
                if (loading) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(FerrexDesignTokens.Space.Xxl),
                        strokeWidth = FerrexDesignTokens.Focus.TvRestingBorder,
                    )
                }
                TheaterPlateText(
                    text = title,
                    role = TheaterPlateTypographyRole.StatusTitle,
                    densityRole = typographyDensity,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            TheaterPlateText(
                text = body,
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = typographyDensity,
                maxLines = TheaterPlateTypographyRole.StatusCopy.defaultMaxLinesForHome(typographyDensity),
            )
            action?.let {
                FerrexActionButton(
                    label = it.label,
                    role = it.role,
                    onClick = it.onClick,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

@Composable
private fun HomeStageSectionTitle(
    title: String,
    density: FerrexStageDensityFamily,
) {
    TheaterPlateText(
        text = title,
        role = TheaterPlateTypographyRole.SectionTitle,
        densityRole = density.toTheaterPlateDensityRole(),
        maxLines = 2,
    )
}

@Composable
private fun HomeStageActionSurface(
    title: String,
    subtitle: String,
    density: FerrexStageDensityFamily,
    tone: FerrexStageSurfaceTone,
    contentDescription: String,
    onClick: () -> Unit,
) {
    val typographyDensity = density.toTheaterPlateDensityRole()
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ControlShelf,
        density = density,
        tone = tone,
        modifier = Modifier.fillMaxWidth(),
        onClick = onClick,
        contentDescription = contentDescription,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
            TheaterPlateText(
                text = title,
                role = TheaterPlateTypographyRole.ActionLabel,
                densityRole = typographyDensity,
            )
            TheaterPlateText(
                text = subtitle,
                role = TheaterPlateTypographyRole.ActionSubtitle,
                densityRole = typographyDensity,
                maxLines = 2,
            )
        }
    }
}

@Composable
private fun HomeStageActionButtons(
    density: FerrexStageDensityFamily,
    onOpenAccountServer: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    if (density == FerrexStageDensityFamily.Compact) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            FerrexActionButton(
                label = "Account & Server",
                role = FerrexActionRole.Secondary,
                onClick = onOpenAccountServer,
                modifier = Modifier.fillMaxWidth(),
                contentDescription = "Account/Server entry point",
            )
            FerrexActionButton(
                label = "Diagnostics",
                role = FerrexActionRole.Secondary,
                onClick = onOpenDiagnostics,
                modifier = Modifier.fillMaxWidth(),
                contentDescription = "Diagnostics entry point",
            )
        }
    } else {
        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            FerrexActionButton(
                label = "Account & Server",
                role = FerrexActionRole.Secondary,
                onClick = onOpenAccountServer,
                modifier = Modifier.weight(1f),
                contentDescription = "Account/Server entry point",
            )
            FerrexActionButton(
                label = "Diagnostics",
                role = FerrexActionRole.Secondary,
                onClick = onOpenDiagnostics,
                modifier = Modifier.weight(1f),
                contentDescription = "Diagnostics entry point",
            )
        }
    }
}

@Composable
private fun LibraryDestinationContent(
    contentPadding: PaddingValues,
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
    freshness: LibraryFreshness,
    selectedLibraryId: String?,
    onSelect: (LibraryMediaCard) -> Unit,
    onSyncSelected: () -> Unit,
    onRetry: () -> Unit,
    onClearSelected: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .testTag(FerrexQaTags.Phone.Libraries)
            .padding(contentPadding)
            .padding(horizontal = FerrexDesignTokens.Space.ScreenPhoneHorizontal, vertical = FerrexDesignTokens.Space.ScreenPhoneVertical),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xxl),
    ) {
        item {
            DestinationHeader(
                title = "Libraries",
                body = "Browse full cached movie and series libraries here instead of burying the complete grids in Home.",
            )
        }
        item {
            LibraryBrowseSection(
                selectedTab = selectedTab,
                onSelectedTab = onSelectedTab,
                movieLibraries = movieLibraries,
                seriesLibraries = seriesLibraries,
                movieLibraryInfos = movieLibraryInfos,
                seriesLibraryInfos = seriesLibraryInfos,
                selectedMovieLibraryId = selectedMovieLibraryId,
                selectedSeriesLibraryId = selectedSeriesLibraryId,
                onSelectedMovieLibrary = onSelectedMovieLibrary,
                onSelectedSeriesLibrary = onSelectedSeriesLibrary,
                selectedMovieInfo = selectedMovieInfo,
                selectedSeriesInfo = selectedSeriesInfo,
                movieSort = movieSort,
                movieFilter = movieFilter,
                onMovieSort = onMovieSort,
                onMovieFilter = onMovieFilter,
                movieIndexState = movieIndexState,
                indexedMovieCards = indexedMovieCards,
                selectedSeriesCards = selectedSeriesCards,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
                onSelect = onSelect,
                onSyncSelected = onSyncSelected,
            )
        }
        item {
            LibraryRecoveryPanel(
                freshness = freshness,
                selectedLibraryId = selectedLibraryId,
                onRetry = onRetry,
                onClearSelected = onClearSelected,
                onChangeServer = onChangeServer,
                onResetConnection = onResetConnection,
                onOpenDiagnostics = onOpenDiagnostics,
            )
        }
    }
}

@Composable
private fun SearchDestinationContent(
    contentPadding: PaddingValues,
    scope: ServerCacheScope,
    searchRepository: MediaSearchRepository?,
    imageRepository: ImageRepository?,
    imagePipeline: FerrexImagePipeline?,
    connectionStatus: AuthenticatedConnectionUi,
    onOpenResult: (SearchDetailTarget) -> Unit,
    onRetryConnection: () -> Unit,
    onOpenAccountServer: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .testTag(FerrexQaTags.Phone.Search)
            .padding(contentPadding)
            .padding(horizontal = FerrexDesignTokens.Space.ScreenPhoneHorizontal, vertical = FerrexDesignTokens.Space.ScreenPhoneVertical),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xxl),
    ) {
        item {
            DestinationHeader(
                title = "Search",
                body = "Search is a dedicated surface that uses the real media query contract and scoped cache; cache misses stay visible with retry.",
            )
        }
        if (connectionStatus.visible) {
            item {
                ConnectionRecoveryCard(
                    connectionStatus = connectionStatus,
                    onRetryConnection = onRetryConnection,
                )
            }
        }
        item {
            PhoneSearchPanel(
                scope = scope,
                searchRepository = searchRepository,
                imageRepository = imageRepository,
                imagePipeline = imagePipeline,
                onOpenResult = onOpenResult,
                onOpenDiagnostics = onOpenDiagnostics,
            )
        }
        item {
            StateCard(
                title = "Need account or server recovery?",
                body = "Use Account for sign out, change server, reset connection, diagnostics, and cache recovery without wiping app data.",
                action = "Open Account" to onOpenAccountServer,
                actionRole = FerrexActionRole.Secondary,
            )
        }
    }
}

@Composable
private fun AccountServerDestinationContent(
    contentPadding: PaddingValues,
    state: SessionState.Authenticated,
    connectionStatus: AuthenticatedConnectionUi,
    freshness: LibraryFreshness,
    selectedLibraryId: String?,
    onRetryConnection: () -> Unit,
    onRetryLibrary: () -> Unit,
    onClearSelected: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .testTag(FerrexQaTags.Phone.AccountServer)
            .padding(contentPadding)
            .padding(horizontal = FerrexDesignTokens.Space.ScreenPhoneHorizontal, vertical = FerrexDesignTokens.Space.ScreenPhoneVertical),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xxl),
    ) {
        item {
            DestinationHeader(
                title = "Account & Server",
                body = "No-wipe recovery exits stay in one place: sign out, change server, retry, reset connection, diagnostics, and cache repair.",
            )
        }
        item {
            AccountSummaryCard(
                state = state,
                connectionStatus = connectionStatus,
                onRetryConnection = onRetryConnection,
                onSignOut = onSignOut,
                onChangeServer = onChangeServer,
                onResetConnection = onResetConnection,
                onOpenDiagnostics = onOpenDiagnostics,
            )
        }
        item {
            LibraryRecoveryPanel(
                freshness = freshness,
                selectedLibraryId = selectedLibraryId,
                onRetry = onRetryLibrary,
                onClearSelected = onClearSelected,
                onChangeServer = onChangeServer,
                onResetConnection = onResetConnection,
                onOpenDiagnostics = onOpenDiagnostics,
            )
        }
    }
}

@Composable
private fun DestinationHeader(
    title: String,
    body: String,
) {
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        Text(
            text = title,
            style = MaterialTheme.typography.headlineMedium,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(body, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
private fun HomeHeader(
    state: SessionState.Authenticated,
    connectionStatus: AuthenticatedConnectionUi,
    playbackNotice: String?,
    density: FerrexStageDensityFamily,
) {
    val typographyDensity = density.toTheaterPlateDensityRole()
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ProjectionShelf,
        density = density,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = Modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Phone.HomeHeader),
        contentDescription = "Phone Home header stage",
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
            TheaterPlateText(
                text = "Ferrex Home",
                role = TheaterPlateTypographyRole.HeroEyebrow,
                densityRole = typographyDensity,
            )
            TheaterPlateText(
                text = FerrexShellCopy.MOBILE_TITLE,
                role = TheaterPlateTypographyRole.HeroTitle,
                densityRole = typographyDensity,
                maxLines = 3,
            )
            TheaterPlateText(
                text = FerrexShellCopy.MOBILE_SUBTITLE,
                role = TheaterPlateTypographyRole.HeroSubtitle,
                densityRole = typographyDensity,
                maxLines = 2,
            )
            TheaterPlateText(
                text = "Signed in as ${state.user.displayName ?: state.user.username} • ${connectionStatus.title} • ${state.serverUrl}",
                role = TheaterPlateTypographyRole.Metadata,
                densityRole = typographyDensity,
                maxLines = 2,
            )
            TheaterPlateText(
                text = FerrexShellCopy.MOBILE_BODY,
                role = TheaterPlateTypographyRole.HeroBody,
                densityRole = typographyDensity,
            )
            if (state.requiresPinSetup) {
                HomeStageStatusBand(
                    title = "PIN setup required",
                    body = "PIN setup is required by this server before PIN sign-in can be used. Use password sign-in or configure PIN support on the server; this app will not show a fake PIN setup flow.",
                    density = density,
                    variant = FerrexStageSurfaceVariant.NoticeSlab,
                    tone = FerrexStatusTone.Retry,
                )
            }
            playbackNotice?.let {
                HomeStageStatusBand(
                    title = "Playback notice",
                    body = it,
                    density = density,
                    variant = FerrexStageSurfaceVariant.NoticeSlab,
                    tone = FerrexStatusTone.Primary,
                )
            }
        }
    }
}

@Composable
private fun HomeEntrySection(
    density: FerrexStageDensityFamily,
    onOpenLibraries: () -> Unit,
    onOpenSearch: () -> Unit,
) {
    Column(
        modifier = Modifier.testTag(FerrexQaTags.Phone.BrowseFind),
        verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
    ) {
        HomeStageSectionTitle("Browse and find", density)
        HomeStageActionSurface(
            title = "Open Libraries",
            subtitle = "Full movie and series grids live on the Libraries tab with sorting, filtering, sync, and cache recovery controls.",
            density = density,
            tone = FerrexStageSurfaceTone.Primary,
            contentDescription = "Open Libraries entry point",
            onClick = onOpenLibraries,
        )
        HomeStageActionSurface(
            title = "Search media",
            subtitle = "Use a dedicated search surface instead of an always-expanded Home panel.",
            density = density,
            tone = FerrexStageSurfaceTone.Neutral,
            contentDescription = "Search media entry point",
            onClick = onOpenSearch,
        )
    }
}

@Composable
private fun HomeUtilityPanel(
    state: SessionState.Authenticated,
    connectionStatus: AuthenticatedConnectionUi,
    freshness: LibraryFreshness,
    density: FerrexStageDensityFamily,
    onRetryConnection: () -> Unit,
    onOpenAccountServer: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    Column(
        modifier = Modifier.testTag(FerrexQaTags.Phone.ServerRecovery),
        verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
    ) {
        HomeStageSectionTitle("Server & recovery", density)
        if (connectionStatus.visible) {
            HomeStageStatusBand(
                title = connectionStatus.title,
                body = connectionStatus.message,
                density = density,
                tone = if (connectionStatus.retryEnabled) FerrexStatusTone.Retry else FerrexStatusTone.StaleOffline,
                action = if (connectionStatus.retryEnabled) {
                    HomeStageAction(
                        label = connectionStatus.retryLabel,
                        role = FerrexActionRole.Retry,
                        onClick = onRetryConnection,
                    )
                } else {
                    null
                },
            )
        } else {
            val cacheStatus = LibraryBrowseModels.libraryStatusCopy(freshness)
            HomeStageStatusBand(
                title = "Recovery exits are ready",
                body = "${state.user.displayName ?: state.user.username} is signed in. Account keeps sign out, change server, reset connection, diagnostics, and cache repair visible without wiping app data. Cache: ${cacheStatus.title}.",
                density = density,
                variant = FerrexStageSurfaceVariant.StatusSlab,
                tone = freshness.statusTone(),
            )
        }
        HomeStageActionButtons(
            density = density,
            onOpenAccountServer = onOpenAccountServer,
            onOpenDiagnostics = onOpenDiagnostics,
        )
    }
}

@Composable
private fun ConnectionRecoveryCard(
    connectionStatus: AuthenticatedConnectionUi,
    onRetryConnection: () -> Unit,
) {
    FerrexStatusCard(
        title = connectionStatus.title,
        body = connectionStatus.message,
        tone = if (connectionStatus.retryEnabled) FerrexStatusTone.Retry else FerrexStatusTone.StaleOffline,
        action = FerrexStatusAction(
            label = connectionStatus.retryLabel,
            role = FerrexActionRole.Retry,
            enabled = connectionStatus.retryEnabled,
            onClick = onRetryConnection,
        ),
    )
}

@Composable
private fun AccountSummaryCard(
    state: SessionState.Authenticated,
    connectionStatus: AuthenticatedConnectionUi,
    onRetryConnection: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    Column(
        modifier = Modifier.testTag(FerrexQaTags.Phone.AccountSummary),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
    ) {
        StateCard(
            title = "Signed in as ${state.user.displayName ?: state.user.username}",
            body = "Server: ${state.serverUrl}. ${if (connectionStatus.visible) connectionStatus.message else "Connection is online; recovery actions remain available."}",
            tone = if (connectionStatus.visible) FerrexStatusTone.StaleOffline else FerrexStatusTone.Secondary,
        )
        if (connectionStatus.visible) {
            FerrexActionButton(
                label = connectionStatus.retryLabel,
                role = FerrexActionRole.Retry,
                enabled = connectionStatus.retryEnabled,
                onClick = onRetryConnection,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            FerrexActionButton(
                label = "Change server",
                role = FerrexActionRole.Secondary,
                onClick = onChangeServer,
                modifier = Modifier.weight(1f),
            )
            FerrexActionButton(
                label = "Sign out",
                role = FerrexActionRole.Secondary,
                onClick = onSignOut,
                modifier = Modifier.weight(1f),
            )
        }
        FerrexActionButton(
            label = "Reset connection",
            role = FerrexActionRole.DestructiveReset,
            onClick = onResetConnection,
            modifier = Modifier.fillMaxWidth(),
        )
        FerrexActionButton(
            label = "Diagnostics / Export diagnostics",
            role = FerrexActionRole.Secondary,
            onClick = onOpenDiagnostics,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

private fun PhoneShellDestination.navIcon(): String = when (this) {
    PhoneShellDestination.Home -> "H"
    PhoneShellDestination.Libraries -> "L"
    PhoneShellDestination.Search -> "S"
    PhoneShellDestination.AccountServer -> "A"
}

@Composable
private fun ContinueWatchingSection(
    continueState: ContinueWatchingState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    density: FerrexStageDensityFamily,
    onRetry: () -> Unit,
    onSelect: (ContinueWatchingCard) -> Unit,
) {
    val heroCard = continueState.cards.firstOrNull()
    val remainingCards = continueState.cards.drop(1)
    val typographyDensity = density.toTheaterPlateDensityRole()
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.RailBand,
        density = density,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = Modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Phone.ContinueWatching),
        contentDescription = "Continue Watching Theater Plate rail band",
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
            HomeStageSectionTitle("Continue Watching", density)
            when (val status = continueState.status) {
                ContinueWatchingStatus.Idle,
                ContinueWatchingStatus.Loading -> HomeStageStatusBand(
                    title = "Loading Continue Watching",
                    body = "The /api/v1/watch/continue shelf loads independently and never blocks library browsing.",
                    density = density,
                    variant = FerrexStageSurfaceVariant.StatusSlab,
                    tone = FerrexStatusTone.Secondary,
                    loading = status == ContinueWatchingStatus.Loading,
                )
                ContinueWatchingStatus.Empty -> HomeStageStatusBand(
                    title = "Nothing in progress",
                    body = "Start playback on a movie or episode and it will appear here.",
                    density = density,
                    variant = FerrexStageSurfaceVariant.EmptyState,
                    tone = FerrexStatusTone.Secondary,
                    action = HomeStageAction("Retry", FerrexActionRole.Retry, onRetry),
                )
                is ContinueWatchingStatus.ErrorRetryable -> HomeStageStatusBand(
                    title = "Continue Watching unavailable",
                    body = status.message,
                    density = density,
                    variant = FerrexStageSurfaceVariant.StatusSlab,
                    tone = FerrexStatusTone.Retry,
                    action = HomeStageAction("Retry", FerrexActionRole.Retry, onRetry),
                )
                is ContinueWatchingStatus.StaleOffline -> HomeStageStatusBand(
                    title = "Stale/offline Continue Watching",
                    body = "Showing ${status.itemCount} previous item(s): ${status.message}",
                    density = density,
                    variant = FerrexStageSurfaceVariant.StatusSlab,
                    tone = FerrexStatusTone.StaleOffline,
                    action = HomeStageAction("Retry", FerrexActionRole.Retry, onRetry),
                )
                is ContinueWatchingStatus.Fresh -> FerrexStageSurface(
                    variant = FerrexStageSurfaceVariant.FactRibbon,
                    density = density,
                    tone = FerrexStageSurfaceTone.Cache,
                    modifier = Modifier.fillMaxWidth(),
                    contentDescription = "Continue Watching freshness",
                ) {
                    TheaterPlateText(
                        text = "${status.itemCount} current item(s) from /api/v1/watch/continue.",
                        role = TheaterPlateTypographyRole.FactValue,
                        densityRole = typographyDensity,
                    )
                }
            }
            heroCard?.let { card ->
                ContinueWatchingHeroCard(
                    card = card,
                    imageResolutions = imageResolutions,
                    imageLoaderAvailable = imageLoaderAvailable,
                    imageLoader = imageLoader,
                    scope = scope,
                    density = density,
                    onClick = { onSelect(card) },
                )
            }
            if (remainingCards.isNotEmpty()) {
                TheaterPlateText(
                    text = "More in progress",
                    role = TheaterPlateTypographyRole.RailTitle,
                    densityRole = typographyDensity,
                )
                val railItems = remember(remainingCards) {
                    remainingCards.zip(
                        MediaRailIdentityResolver.assign(
                            railKey = "continue-watching-more",
                            stableIds = remainingCards.map { it.stableKey },
                        ),
                    )
                }
                LazyRow(horizontalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
                    items(railItems, key = { it.second.renderKey }) { (card, identity) ->
                        ContinueWatchingCardView(
                            card = card,
                            imageResolutions = imageResolutions,
                            imageLoaderAvailable = imageLoaderAvailable,
                            imageLoader = imageLoader,
                            scope = scope,
                            density = density,
                            semanticLabel = identity.semanticLabel(card.title),
                            onClick = { onSelect(card) },
                        )
                    }
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
    density: FerrexStageDensityFamily,
    onSelect: (LibraryMediaCard) -> Unit,
) {
    val typographyDensity = density.toTheaterPlateDensityRole()
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.RailBand,
        density = density,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = Modifier.fillMaxWidth(),
        contentDescription = "${shelf.title} Home shelf",
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
            HomeStageSectionTitle(shelf.title, density)
            TheaterPlateText(
                text = shelf.subtitle,
                role = TheaterPlateTypographyRole.RailSubtitle,
                densityRole = typographyDensity,
                maxLines = 2,
            )
            TheaterPlateText(
                text = shelf.limitCopy,
                role = TheaterPlateTypographyRole.Metadata,
                densityRole = typographyDensity,
                maxLines = 2,
            )
            val railItems = remember(shelf.title, shelf.items) {
                shelf.items.zip(
                    MediaRailIdentityResolver.assign(
                        railKey = shelf.title,
                        stableIds = shelf.items.map { it.stableKey },
                    ),
                )
            }
            LazyRow(horizontalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
                items(railItems, key = { it.second.renderKey }) { (card, identity) ->
                    HomeMediaRailCard(
                        card = card,
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoaderAvailable,
                        imageLoader = imageLoader,
                        scope = scope,
                        density = density,
                        semanticLabel = identity.semanticLabel(card.title),
                        onClick = { onSelect(card) },
                    )
                }
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
    Column(
        modifier = Modifier.testTag(FerrexQaTags.Phone.LibraryTabs),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
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
        modifier = Modifier
            .testTag(FerrexQaTags.Phone.LibraryChooser)
            .horizontalScroll(rememberScrollState()),
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
            .testTag(FerrexQaTags.Phone.LibraryGrid)
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
private fun ContinueWatchingHeroCard(
    card: ContinueWatchingCard,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    density: FerrexStageDensityFamily,
    onClick: () -> Unit,
) {
    val typographyDensity = density.toTheaterPlateDensityRole()
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ProjectionShelf,
        density = density,
        tone = FerrexStageSurfaceTone.Primary,
        modifier = Modifier.fillMaxWidth(),
        onClick = onClick,
        contentDescription = "Continue Watching ${card.title}",
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Poster(
                imageKey = card.imageKey,
                title = card.title,
                fallbackPath = null,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
                modifier = Modifier.width(if (density == FerrexStageDensityFamily.Compact) 112.dp else 132.dp),
            )
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
            ) {
                TheaterPlateText(
                    text = "Continue Watching",
                    role = TheaterPlateTypographyRole.HeroEyebrow,
                    densityRole = typographyDensity,
                )
                TheaterPlateText(
                    text = card.title,
                    role = TheaterPlateTypographyRole.RailTitle,
                    densityRole = typographyDensity,
                    maxLines = 2,
                )
                TheaterPlateText(
                    text = card.subtitle,
                    role = TheaterPlateTypographyRole.RailSubtitle,
                    densityRole = typographyDensity,
                    maxLines = 2,
                )
                TheaterPlateText(
                    text = card.progressLabel,
                    role = TheaterPlateTypographyRole.FactValue,
                    densityRole = typographyDensity,
                )
                FerrexActionButton(
                    label = "Open",
                    role = FerrexActionRole.Primary,
                    onClick = onClick,
                )
            }
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
    density: FerrexStageDensityFamily,
    semanticLabel: String = card.title,
    onClick: () -> Unit,
) {
    val typographyDensity = density.toTheaterPlateDensityRole()
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ProjectionShelf,
        density = density,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = Modifier.width(FerrexDesignTokens.Poster.PhoneWidth),
        onClick = onClick,
        contentDescription = semanticLabel,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
            Poster(
                imageKey = card.imageKey,
                title = card.title,
                fallbackPath = null,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
                semanticLabel = semanticLabel,
            )
            TheaterPlateText(
                text = card.title,
                role = TheaterPlateTypographyRole.RailTitle,
                densityRole = typographyDensity,
            )
            TheaterPlateText(
                text = card.subtitle,
                role = TheaterPlateTypographyRole.RailSubtitle,
                densityRole = typographyDensity,
                maxLines = 2,
            )
            TheaterPlateText(
                text = card.progressLabel,
                role = TheaterPlateTypographyRole.Metadata,
                densityRole = typographyDensity,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

@Composable
private fun HomeMediaRailCard(
    card: LibraryMediaCard,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    density: FerrexStageDensityFamily,
    semanticLabel: String = card.title,
    onClick: () -> Unit,
) {
    val typographyDensity = density.toTheaterPlateDensityRole()
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ProjectionShelf,
        density = density,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = Modifier.width(FerrexDesignTokens.Poster.PhoneCompactWidth),
        onClick = onClick,
        contentDescription = semanticLabel,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
            Poster(
                imageKey = card.imageKey,
                title = card.title,
                fallbackPath = card.publicFallbackPath,
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
                semanticLabel = semanticLabel,
            )
            TheaterPlateText(
                text = card.title,
                role = TheaterPlateTypographyRole.RailTitle,
                densityRole = typographyDensity,
            )
            TheaterPlateText(
                text = card.subtitle,
                role = TheaterPlateTypographyRole.RailSubtitle,
                densityRole = typographyDensity,
                maxLines = 2,
            )
            TheaterPlateText(
                text = card.libraryName,
                role = TheaterPlateTypographyRole.Metadata,
                densityRole = typographyDensity,
            )
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
    semanticLabel: String = card.title,
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
                semanticLabel = semanticLabel,
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
    modifier: Modifier = Modifier,
    semanticLabel: String = title,
) {
    if (imageKey == null || !imageLoaderAvailable || imageLoader == null) {
        FerrexPosterPlaceholder(
            if (imageKey == null) "No poster" else "Images unavailable",
            modifier = modifier.semantics(mergeDescendants = true) { contentDescription = semanticLabel },
        )
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
        contentDescription = semanticLabel,
        modifier = modifier,
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
    Column(
        modifier = Modifier.testTag(FerrexQaTags.Phone.LibraryRecovery),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
    ) {
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

private fun homeStageSourceContext(
    viewport: TheaterPlateViewport,
    connectionStatus: AuthenticatedConnectionUi,
    freshness: LibraryFreshness,
): TheaterPlateSourceContext = TheaterPlateSourceContext(
    source = TheaterPlateImageSource.fallback(TheaterPlateImageSourceKind.GeneratedFallback),
    viewport = viewport,
    themeColor = homeStageSeedColor(connectionStatus, freshness),
    defaultColor = TheaterPlateColor.DefaultStage,
)

private fun homeStageSeedColor(
    connectionStatus: AuthenticatedConnectionUi,
    freshness: LibraryFreshness,
): TheaterPlateColor = when {
    connectionStatus.health != AuthConnectionHealth.Online -> TheaterPlateColor.rgb(51, 65, 85)
    freshness is LibraryFreshness.ErrorRetryable -> TheaterPlateColor.rgb(127, 29, 29)
    freshness is LibraryFreshness.CorruptRebuilding -> TheaterPlateColor.rgb(49, 46, 129)
    freshness is LibraryFreshness.StaleOffline -> TheaterPlateColor.rgb(71, 85, 105)
    freshness is LibraryFreshness.Fresh -> TheaterPlateColor.rgb(22, 78, 99)
    freshness is LibraryFreshness.Syncing -> TheaterPlateColor.rgb(56, 189, 248)
    LibraryFreshness.Empty == freshness -> TheaterPlateColor.rgb(30, 41, 59)
    else -> TheaterPlateColor.DefaultStage
}

private fun homeStageHasStaleOrOfflineState(
    connectionStatus: AuthenticatedConnectionUi,
    freshness: LibraryFreshness,
): Boolean = connectionStatus.health != AuthConnectionHealth.Online || homeStageHasCacheRecoveryState(freshness)

private fun homeStageHasCacheRecoveryState(freshness: LibraryFreshness): Boolean = when (freshness) {
    LibraryFreshness.Empty,
    LibraryFreshness.Syncing,
    is LibraryFreshness.Fresh -> false
    is LibraryFreshness.StaleOffline,
    is LibraryFreshness.CorruptRebuilding,
    is LibraryFreshness.ErrorRetryable -> true
}

private fun FerrexStageDensityFamily.toTheaterPlateDensityRole(): TheaterPlateDensityRole = when (this) {
    FerrexStageDensityFamily.Compact -> TheaterPlateDensityRole.PhonePortrait
    FerrexStageDensityFamily.Standard -> TheaterPlateDensityRole.PhoneLandscape
    FerrexStageDensityFamily.TenFoot -> TheaterPlateDensityRole.Tv1080p
}

private fun FerrexStatusTone.toStageSurfaceTone(): FerrexStageSurfaceTone = when (this) {
    FerrexStatusTone.Primary,
    FerrexStatusTone.Retry -> FerrexStageSurfaceTone.Primary
    FerrexStatusTone.Secondary -> FerrexStageSurfaceTone.Neutral
    FerrexStatusTone.Cache -> FerrexStageSurfaceTone.Cache
    FerrexStatusTone.StaleOffline -> FerrexStageSurfaceTone.StaleOffline
    FerrexStatusTone.DestructiveReset,
    FerrexStatusTone.Error -> FerrexStageSurfaceTone.Error
}

private fun TheaterPlateTypographyRole.defaultMaxLinesForHome(densityRole: TheaterPlateDensityRole): Int = when (this) {
    TheaterPlateTypographyRole.StatusCopy,
    TheaterPlateTypographyRole.RecoveryCopy -> if (densityRole == TheaterPlateDensityRole.PhonePortrait) 5 else 4
    TheaterPlateTypographyRole.HeroTitle -> if (densityRole == TheaterPlateDensityRole.PhonePortrait) 3 else 2
    else -> 2
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
