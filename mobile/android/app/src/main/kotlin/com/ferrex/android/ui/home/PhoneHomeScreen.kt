package com.ferrex.android.ui.home

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import com.ferrex.android.core.auth.AuthConnectionHealth
import com.ferrex.android.core.auth.AuthenticatedConnectionSurface
import com.ferrex.android.core.auth.AuthenticatedConnectionUi
import com.ferrex.android.core.auth.ConnectionRecoveryRefreshGate
import com.ferrex.android.core.auth.NoWipeRecoveryActionKind
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.auth.connectionRecoveryUi
import com.ferrex.android.core.auth.noWipeRecoveryActions
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
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.detail.DetailCache
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.mediaart.MediaArtGrounding
import com.ferrex.android.core.mediaart.MediaArtObject
import com.ferrex.android.core.mediaart.MediaArtRequest
import com.ferrex.android.core.mediaart.MediaArtTargetIdentity
import com.ferrex.android.core.mediaart.MediaRailItemIdentity
import com.ferrex.android.core.playback.MediaRoutePersistence
import com.ferrex.android.core.playback.PlaybackLaunchDecision
import com.ferrex.android.core.playback.PlaybackLaunchPolicy
import com.ferrex.android.core.playback.PlaybackProgressReporter
import com.ferrex.android.core.playback.PlaybackResumeProgressProvider
import com.ferrex.android.core.playback.PlaybackRoutePersistence
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
import com.ferrex.android.core.watch.ContinueWatchingProgressState
import com.ferrex.android.core.watch.ContinueWatchingRepository
import com.ferrex.android.core.watch.ContinueWatchingState
import com.ferrex.android.core.watch.ContinueWatchingStatus
import com.ferrex.android.core.watch.WatchRepository
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.core.watch.WatchStateInvalidationBus
import com.ferrex.android.ui.components.FerrexActionButton
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexMobileMediaCard
import com.ferrex.android.ui.components.FerrexMobileMediaGrid
import com.ferrex.android.ui.components.FerrexMobileMediaRail
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.MobileMediaCardLayout
import com.ferrex.android.ui.components.MobileMediaCardState
import com.ferrex.android.ui.components.MobileMediaWatchState
import com.ferrex.android.ui.components.TheaterPlateDensityRole
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.navigation.MediaRouteArgsSaver
import com.ferrex.android.navigation.PlaybackRouteContractSaver
import com.ferrex.android.navigation.enumNameSaver
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
    val routePersistenceScope = remember(state.serverUrl, state.user.id) {
        PlaybackRoutePersistence.scopeKey(state.serverUrl, state.user.id)
    }
    var selectedDestination by rememberSaveable(
        routePersistenceScope,
        stateSaver = enumNameSaver(PhoneShellDestination.entries, PhoneShellDestination.Home),
    ) { mutableStateOf(PhoneShellDestination.Home) }
    var selectedTab by rememberSaveable(
        routePersistenceScope,
        stateSaver = enumNameSaver(HomeLibraryTab.entries, HomeLibraryTab.Movies),
    ) { mutableStateOf(HomeLibraryTab.Movies) }
    var selectedMovieLibraryId by rememberSaveable(routePersistenceScope) { mutableStateOf<String?>(null) }
    var selectedSeriesLibraryId by rememberSaveable(routePersistenceScope) { mutableStateOf<String?>(null) }
    var movieSort by rememberSaveable(
        routePersistenceScope,
        stateSaver = enumNameSaver(MovieSortMode.entries, MovieSortMode.TitleAsc),
    ) { mutableStateOf(MovieSortMode.TitleAsc) }
    var movieFilter by rememberSaveable(
        routePersistenceScope,
        stateSaver = enumNameSaver(MovieFilterMode.entries, MovieFilterMode.All),
    ) { mutableStateOf(MovieFilterMode.All) }
    var movieIndexState by remember { mutableStateOf<MovieIndexUiState>(MovieIndexUiState.Idle) }
    var selectedDetailRoute by rememberSaveable(routePersistenceScope, stateSaver = MediaRouteArgsSaver) {
        mutableStateOf<MediaRouteArgs?>(null)
    }
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
    LaunchedEffect(selectedDetailRoute) {
        playbackNotice = null
    }
    LaunchedEffect(activePlaybackContract, selectedDetailRoute) {
        val playbackRoute = activePlaybackContract ?: return@LaunchedEffect
        if (selectedDetailRoute == null) {
            selectedDetailRoute = MediaRoutePersistence.decodeRouteString(playbackRoute.sourceDetailRoute)
        }
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
    val selectedMovieFreshness = repositoryState?.takeIf { it.selectedLibraryId == selectedMovieInfo?.id }?.freshness ?: LibraryFreshness.Empty
    val selectedSeriesFreshness = repositoryState?.takeIf { it.selectedLibraryId == selectedSeriesInfo?.id }?.freshness ?: LibraryFreshness.Empty

    LaunchedEffect(libraryRepository, scope.directoryName, selectedDestination, selectedTab, selectedSeriesInfo?.id) {
        if (selectedDestination == PhoneShellDestination.Libraries && selectedTab == HomeLibraryTab.Series) {
            selectedSeriesInfo?.let { library ->
                libraryRepository?.syncSeriesLibrary(scope, library, repositoryState?.libraries.orEmpty())
            }
        }
    }

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
        syncSelectedLibrary()
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
                onChangeServer = {
                    activePlaybackContract = null
                    selectedDetailRoute = null
                    onChangeServer()
                },
                onSignOut = {
                    activePlaybackContract = null
                    selectedDetailRoute = null
                    onSignOut()
                },
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
                onClearAllCache = { libraryRepository?.clearAllCache(scope) },
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
                libraryFreshness = repositoryState?.takeIf { it.selectedLibraryId == selectedDetailRoute?.libraryId }?.freshness,
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
                        connectionStatus = homeConnectionUi,
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
                        freshness = when (selectedTab) {
                            HomeLibraryTab.Movies -> selectedMovieFreshness
                            HomeLibraryTab.Series -> selectedSeriesFreshness
                        },
                        selectedLibraryId = selectedBrowseLibraryId,
                        onSelect = { selectedDetailRoute = it.route },
                        onSyncSelected = { syncSelectedLibrary() },
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
                        selectedLibraryId = selectedBrowseLibraryId,
                        onClearSelected = { clearSelectedLibraryCache() },
                        onClearAll = { libraryRepository?.clearAllCache(scope) },
                        onChangeServer = onChangeServer,
                        onResetConnection = onResetConnection,
                        onOpenAccountServer = { selectedDestination = PhoneShellDestination.AccountServer },
                        onOpenDiagnostics = onOpenDiagnostics,
                    )
                    PhoneShellDestination.AccountServer -> AccountServerDestinationContent(
                        contentPadding = contentPadding,
                        state = state,
                        connectionStatus = homeConnectionUi,
                        freshness = when (selectedTab) {
                            HomeLibraryTab.Movies -> selectedMovieFreshness
                            HomeLibraryTab.Series -> selectedSeriesFreshness
                        },
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
                if (phoneHomeShouldShowStatusNotices(connectionStatus.visible, freshness)) {
                    item {
                        HomeStatusBands(
                            connectionStatus = connectionStatus,
                            freshness = freshness,
                            density = density,
                            onRetryConnection = onRetryConnection,
                            onOpenAccountServer = onOpenAccountServer,
                        )
                    }
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
                        connectionStatus = connectionStatus,
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
    onOpenAccountServer: () -> Unit,
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
        }

        if (homeStageHasCacheRecoveryState(freshness)) {
            val cacheStatus = LibraryBrowseModels.libraryStatusCopy(freshness)
            HomeStageStatusBand(
                title = "Library cache • ${cacheStatus.title}",
                body = cacheStatus.detail,
                density = density,
                variant = FerrexStageSurfaceVariant.StatusSlab,
                tone = freshness.statusTone(),
                action = HomeStageAction(
                    label = "Open recovery",
                    role = freshness.statusTone().toActionRole(),
                    onClick = onOpenAccountServer,
                ),
            )
        }
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
    connectionStatus: AuthenticatedConnectionUi,
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
    onClearSelected: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
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

        var showLibraryChooser by remember { mutableStateOf(false) }
        var showStatusDialog by remember { mutableStateOf(false) }
        var showRecoveryDialog by remember { mutableStateOf(false) }
        val activeLibraryId = when (selectedTab) {
            HomeLibraryTab.Movies -> selectedMovieLibraryId
            HomeLibraryTab.Series -> selectedSeriesLibraryId
        }
        val activeLibraryInfo = when (selectedTab) {
            HomeLibraryTab.Movies -> selectedMovieInfo
            HomeLibraryTab.Series -> selectedSeriesInfo
        }
        val activeLibraries = when (selectedTab) {
            HomeLibraryTab.Movies -> movieLibraryInfos.ifEmpty { movieLibraries.map { it.library } }
            HomeLibraryTab.Series -> seriesLibraryInfos.ifEmpty { seriesLibraries.map { it.library } }
        }
        val cachedLibraryIds = when (selectedTab) {
            HomeLibraryTab.Movies -> movieLibraries.map { it.library.id }.toSet()
            HomeLibraryTab.Series -> seriesLibraries.map { it.library.id }.toSet()
        }
        val activeCards = when (selectedTab) {
            HomeLibraryTab.Movies -> indexedMovieCards.cards
            HomeLibraryTab.Series -> selectedSeriesCards
        }
        val activeFullCount = when (selectedTab) {
            HomeLibraryTab.Movies -> movieLibraries.firstOrNull { it.library.id == selectedMovieLibraryId }?.accessor?.movieCount ?: activeCards.size
            HomeLibraryTab.Series -> seriesLibraries.firstOrNull { it.library.id == selectedSeriesLibraryId }?.accessor?.seriesReferenceCount ?: activeCards.size
        }
        val statusSummary = libraryGridStatusSummary(
            selectedTab = selectedTab,
            activeLibraryName = activeLibraryInfo?.name,
            visibleCount = activeCards.size,
            fullCachedCount = activeFullCount,
            freshness = freshness,
            movieIndexState = movieIndexState,
            movieSort = movieSort,
            movieFilter = movieFilter,
            invalidIndexCount = indexedMovieCards.invalidIndexCount,
            appendedMissingCount = indexedMovieCards.appendedMissingCount,
        )

        TheaterPlateStage(
            analysis = stageAnalysis,
            adaptation = adaptation,
            density = density,
            modifier = Modifier
                .fillMaxSize()
                .testTag(FerrexQaTags.Phone.Libraries),
            contentDescription = "Phone Libraries Theater Plate stage with compact controls, one dense grid scroll, and no-wipe recovery actions",
        ) {
            Column(
                modifier = Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(density.tokens().contentGap),
            ) {
                LibraryCompactControls(
                    selectedTab = selectedTab,
                    onSelectedTab = onSelectedTab,
                    selectedLibraryName = activeLibraryInfo?.name,
                    libraryCount = activeLibraries.size,
                    activeCardCount = activeCards.size,
                    movieSort = movieSort,
                    movieFilter = movieFilter,
                    showMovieControls = selectedTab == HomeLibraryTab.Movies,
                    statusSummary = statusSummary,
                    density = density,
                    onOpenLibraryChooser = { showLibraryChooser = true },
                    onMovieSort = onMovieSort,
                    onMovieFilter = onMovieFilter,
                    onOpenStatus = { showStatusDialog = true },
                    onOpenRecovery = { showRecoveryDialog = true },
                )
                if (activeCards.isEmpty()) {
                    LibraryEmptyStatePanel(
                        selectedTab = selectedTab,
                        selectedLibraryName = activeLibraryInfo?.name,
                        freshness = freshness,
                        selectedLibraryId = selectedLibraryId ?: activeLibraryId,
                        statusSummary = statusSummary,
                        density = density,
                        modifier = Modifier.weight(1f),
                        onRetry = onSyncSelected,
                        onClearSelected = onClearSelected,
                        onChangeServer = onChangeServer,
                        onResetConnection = onResetConnection,
                        onOpenDiagnostics = onOpenDiagnostics,
                    )
                } else {
                    DenseLibraryGrid(
                        cards = activeCards,
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoaderAvailable,
                        imageLoader = imageLoader,
                        scope = scope,
                        density = density,
                        modifier = Modifier.weight(1f),
                        onSelect = onSelect,
                    )
                }
            }
            if (showLibraryChooser) {
                LibraryChooserDialog(
                    selectedTab = selectedTab,
                    libraries = activeLibraries,
                    selectedLibraryId = activeLibraryId,
                    cachedIds = cachedLibraryIds,
                    onDismiss = { showLibraryChooser = false },
                    onSelectedLibrary = { libraryId ->
                        when (selectedTab) {
                            HomeLibraryTab.Movies -> onSelectedMovieLibrary(libraryId)
                            HomeLibraryTab.Series -> onSelectedSeriesLibrary(libraryId)
                        }
                        showLibraryChooser = false
                    },
                )
            }
            if (showStatusDialog) {
                LibraryStatusDialog(
                    statusSummary = statusSummary,
                    freshness = freshness,
                    selectedLibraryName = activeLibraryInfo?.name,
                    activeCardCount = activeCards.size,
                    fullCachedCount = activeFullCount,
                    selectedTab = selectedTab,
                    onDismiss = { showStatusDialog = false },
                    onRetry = onSyncSelected,
                    onOpenDiagnostics = onOpenDiagnostics,
                )
            }
            if (showRecoveryDialog) {
                LibraryRecoveryDialog(
                    freshness = freshness,
                    selectedLibraryId = selectedLibraryId ?: activeLibraryId,
                    onDismiss = { showRecoveryDialog = false },
                    onRetry = onSyncSelected,
                    onClearSelected = onClearSelected,
                    onChangeServer = onChangeServer,
                    onResetConnection = onResetConnection,
                    onOpenDiagnostics = onOpenDiagnostics,
                )
            }
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
    selectedLibraryId: String?,
    onClearSelected: () -> Unit,
    onClearAll: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenAccountServer: () -> Unit,
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
        val searchFreshness = LibraryFreshness.Empty
        val stageContext = remember(viewport, connectionStatus.health) {
            homeStageSourceContext(viewport, connectionStatus, searchFreshness)
        }
        val stageAnalysis = remember(analyzer, stageContext) { analyzer.analyzeMissingBackdrop(stageContext) }
        val adaptation = remember(connectionStatus.health) {
            if (connectionStatus.health != AuthConnectionHealth.Online) {
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
                .testTag(FerrexQaTags.Phone.Search),
            contentDescription = "Phone Search Theater Plate stage with query, retry, cache-miss, diagnostics, and open-result actions",
        ) {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(density.tokens().contentGap),
            ) {
                if (connectionStatus.visible) {
                    item {
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
                        selectedLibraryId = selectedLibraryId,
                        onClearSelectedCache = onClearSelected,
                        onClearAllCache = onClearAll,
                        onChangeServer = onChangeServer,
                        onResetConnection = onResetConnection,
                        density = density,
                    )
                }
                item {
                    StateCard(
                        title = "Account and server recovery",
                        body = "Sign out, change server, reset connection, diagnostics, and cache repair stay available without wiping app data.",
                        density = density,
                        variant = FerrexStageSurfaceVariant.ControlShelf,
                        action = "Open Account" to onOpenAccountServer,
                        actionRole = FerrexActionRole.Secondary,
                    )
                }
            }
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
                density = FerrexStageDensityFamily.Standard,
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
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Phone.HomeHeader),
        verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
    ) {
        TheaterPlateText(
            text = phoneHomeSignedInLine(
                displayName = state.user.displayName,
                username = state.user.username,
                connectionTitle = connectionStatus.title,
                serverUrl = state.serverUrl,
            ),
            role = TheaterPlateTypographyRole.Metadata,
            densityRole = typographyDensity,
            maxLines = 2,
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
    connectionStatus: AuthenticatedConnectionUi,
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
        }
        HomeStageActionButtons(
            density = density,
            onOpenAccountServer = onOpenAccountServer,
            onOpenDiagnostics = onOpenDiagnostics,
        )
    }
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
    val recoveryActions = noWipeRecoveryActions(includeCacheClear = false).associateBy { it.kind }
    val retryAction = recoveryActions.getValue(NoWipeRecoveryActionKind.Retry)
    val signOutAction = recoveryActions.getValue(NoWipeRecoveryActionKind.SignOut)
    val changeServerAction = recoveryActions.getValue(NoWipeRecoveryActionKind.ChangeServer)
    val resetConnectionAction = recoveryActions.getValue(NoWipeRecoveryActionKind.ResetConnection)
    val diagnosticsAction = recoveryActions.getValue(NoWipeRecoveryActionKind.Diagnostics)
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
                subtitle = retryAction.subtitle,
                role = FerrexActionRole.Retry,
                enabled = connectionStatus.retryEnabled,
                onClick = onRetryConnection,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            FerrexActionButton(
                label = changeServerAction.label,
                subtitle = changeServerAction.subtitle,
                role = FerrexActionRole.Secondary,
                onClick = onChangeServer,
                modifier = Modifier.weight(1f),
            )
            FerrexActionButton(
                label = signOutAction.label,
                subtitle = signOutAction.subtitle,
                role = FerrexActionRole.Secondary,
                onClick = onSignOut,
                modifier = Modifier.weight(1f),
            )
        }
        FerrexActionButton(
            label = resetConnectionAction.label,
            subtitle = resetConnectionAction.subtitle,
            role = FerrexActionRole.DestructiveReset,
            onClick = onResetConnection,
            modifier = Modifier.fillMaxWidth(),
        )
        FerrexActionButton(
            label = diagnosticsAction.label,
            subtitle = diagnosticsAction.subtitle,
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
                FerrexMobileMediaRail(
                    railKey = "continue-watching-more",
                    title = "More in progress",
                    subtitle = "Touch a card to open the preserved Continue Watching route.",
                    items = remainingCards,
                    itemStableId = { it.stableKey },
                    density = density,
                    contentDescription = "More Continue Watching items rail",
                ) { card, identity ->
                    ContinueWatchingCardView(
                        card = card,
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoaderAvailable,
                        imageLoader = imageLoader,
                        scope = scope,
                        density = density,
                        itemIdentity = identity,
                        semanticLabel = identity.semanticLabel(card.title),
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
    density: FerrexStageDensityFamily,
    onSelect: (LibraryMediaCard) -> Unit,
) {
    FerrexMobileMediaRail(
        railKey = shelf.title,
        title = shelf.title,
        subtitle = "${shelf.subtitle} ${shelf.limitCopy}",
        items = shelf.items,
        itemStableId = { it.stableKey },
        density = density,
        modifier = Modifier.fillMaxWidth(),
        contentDescription = "${shelf.title} Home shelf rail",
    ) { card, identity ->
        HomeMediaRailCard(
            card = card,
            imageResolutions = imageResolutions,
            imageLoaderAvailable = imageLoaderAvailable,
            imageLoader = imageLoader,
            scope = scope,
            density = density,
            itemIdentity = identity,
            semanticLabel = identity.semanticLabel(card.title),
            onClick = { onSelect(card) },
        )
    }
}

@Composable
private fun LibraryCompactControls(
    selectedTab: HomeLibraryTab,
    onSelectedTab: (HomeLibraryTab) -> Unit,
    selectedLibraryName: String?,
    libraryCount: Int,
    activeCardCount: Int,
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    showMovieControls: Boolean,
    statusSummary: LibraryGridStatusSummary,
    density: FerrexStageDensityFamily,
    onOpenLibraryChooser: () -> Unit,
    onMovieSort: (MovieSortMode) -> Unit,
    onMovieFilter: (MovieFilterMode) -> Unit,
    onOpenStatus: () -> Unit,
    onOpenRecovery: () -> Unit,
) {
    var sortExpanded by remember { mutableStateOf(false) }
    var filterExpanded by remember { mutableStateOf(false) }
    val selectedLibraryLabel = selectedLibraryName ?: if (libraryCount == 0) "No libraries" else "Choose library"
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ControlShelf,
        density = density,
        tone = FerrexStageSurfaceTone.Primary,
        modifier = Modifier.fillMaxWidth(),
        testTag = FerrexQaTags.Phone.LibraryTabs,
        contentDescription = "Compact Libraries controls for tab, library, movie sort/filter, status, and recovery actions",
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState()),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        ) {
            HomeLibraryTab.entries.forEach { tab ->
                FerrexActionButton(
                    label = tab.label,
                    role = if (tab == selectedTab) FerrexActionRole.Primary else FerrexActionRole.Secondary,
                    onClick = { onSelectedTab(tab) },
                    testTag = FerrexQaTags.Phone.libraryAction("tab-${tab.name}"),
                    contentDescription = "Show ${tab.label} libraries",
                )
            }
            FerrexActionButton(
                label = "Library: $selectedLibraryLabel",
                role = FerrexActionRole.Cache,
                onClick = onOpenLibraryChooser,
                testTag = FerrexQaTags.Phone.LibraryChooser,
                contentDescription = "Open library chooser. $libraryCount available; $activeCardCount visible cards.",
            )
            if (showMovieControls) {
                Box {
                    FerrexActionButton(
                        label = "Sort: ${movieSort.label}",
                        role = FerrexActionRole.Secondary,
                        onClick = { sortExpanded = true },
                        testTag = FerrexQaTags.Phone.libraryControl("sort"),
                        contentDescription = "Open movie sort menu. Current sort ${movieSort.label}.",
                    )
                    DropdownMenu(
                        expanded = sortExpanded,
                        onDismissRequest = { sortExpanded = false },
                    ) {
                        MovieSortMode.entries.forEach { mode ->
                            DropdownMenuItem(
                                text = { Text(mode.label) },
                                onClick = {
                                    onMovieSort(mode)
                                    sortExpanded = false
                                },
                            )
                        }
                    }
                }
                Box {
                    FerrexActionButton(
                        label = "Filter: ${movieFilter.label}",
                        role = FerrexActionRole.Secondary,
                        onClick = { filterExpanded = true },
                        testTag = FerrexQaTags.Phone.libraryControl("filter"),
                        contentDescription = "Open movie filter menu. Current filter ${movieFilter.label}.",
                    )
                    DropdownMenu(
                        expanded = filterExpanded,
                        onDismissRequest = { filterExpanded = false },
                    ) {
                        MovieFilterMode.entries.forEach { mode ->
                            DropdownMenuItem(
                                text = { Text(mode.label) },
                                onClick = {
                                    onMovieFilter(mode)
                                    filterExpanded = false
                                },
                            )
                        }
                    }
                }
            }
            FerrexActionButton(
                label = "Status: ${statusSummary.label}",
                role = statusSummary.tone.toActionRole(),
                onClick = onOpenStatus,
                testTag = FerrexQaTags.Phone.LibraryIndexStatus,
                contentDescription = "Open library status. ${statusSummary.detail}",
            )
            FerrexActionButton(
                label = "More / Recovery",
                role = statusSummary.tone.toActionRole(),
                onClick = onOpenRecovery,
                testTag = FerrexQaTags.Phone.LibraryRecovery,
                contentDescription = "Open retry, clear cache, change server, reset connection, and diagnostics actions",
            )
        }
    }
}

@Composable
private fun LibraryChooserDialog(
    selectedTab: HomeLibraryTab,
    libraries: List<LibraryInfo>,
    selectedLibraryId: String?,
    cachedIds: Set<String>,
    onDismiss: () -> Unit,
    onSelectedLibrary: (String) -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Select ${selectedTab.label.lowercase()} library") },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
            ) {
                if (libraries.isEmpty()) {
                    Text("No ${selectedTab.label.lowercase()} libraries are cached or reported yet. Use Retry sync, Change server, Reset connection, or Diagnostics from More / Recovery before wiping app data.")
                } else {
                    libraries.forEach { library ->
                        val cached = library.id in cachedIds
                        FerrexActionButton(
                            label = library.name,
                            subtitle = if (cached) "Cached" else "Not cached yet",
                            role = if (library.id == selectedLibraryId) FerrexActionRole.Primary else if (cached) FerrexActionRole.Secondary else FerrexActionRole.Cache,
                            onClick = { onSelectedLibrary(library.id) },
                            modifier = Modifier.fillMaxWidth(),
                            testTag = FerrexQaTags.Phone.libraryAction("choose-${library.id}"),
                            contentDescription = "Choose ${library.name}${if (cached) "" else "; cache not loaded"}",
                        )
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) { Text("Close") }
        },
    )
}

@Composable
private fun LibraryStatusDialog(
    statusSummary: LibraryGridStatusSummary,
    freshness: LibraryFreshness,
    selectedLibraryName: String?,
    activeCardCount: Int,
    fullCachedCount: Int,
    selectedTab: HomeLibraryTab,
    onDismiss: () -> Unit,
    onRetry: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val cacheStatus = LibraryBrowseModels.libraryStatusCopy(freshness)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(statusSummary.title) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                Text("Library: ${selectedLibraryName ?: "None selected"}")
                Text("Grid: $activeCardCount visible of $fullCachedCount cached ${selectedTab.label.lowercase()} item(s).")
                Text("Cache: ${cacheStatus.title}")
                Text(statusSummary.detail)
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    onRetry()
                    onDismiss()
                },
            ) { Text("Retry sync") }
        },
        dismissButton = {
            Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                TextButton(
                    onClick = {
                        onOpenDiagnostics()
                        onDismiss()
                    },
                ) { Text("Diagnostics") }
                TextButton(onClick = onDismiss) { Text("Close") }
            }
        },
    )
}

@Composable
private fun LibraryRecoveryDialog(
    freshness: LibraryFreshness,
    selectedLibraryId: String?,
    onDismiss: () -> Unit,
    onRetry: () -> Unit,
    onClearSelected: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val status = LibraryBrowseModels.libraryStatusCopy(freshness)
    val actions = LibraryBrowseModels.recoveryActionVisibility(selectedLibraryId)
    fun runAndDismiss(action: () -> Unit) {
        action()
        onDismiss()
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Library recovery") },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
            ) {
                Text(status.title)
                Text(status.detail)
                if (actions.retry) {
                    FerrexActionButton(
                        label = "Retry sync",
                        role = FerrexActionRole.Retry,
                        onClick = { runAndDismiss(onRetry) },
                        modifier = Modifier.fillMaxWidth(),
                        testTag = FerrexQaTags.Phone.libraryAction("retry"),
                    )
                }
                if (actions.clearSelectedCache) {
                    FerrexActionButton(
                        label = "Clear selected cache",
                        role = FerrexActionRole.Cache,
                        onClick = { runAndDismiss(onClearSelected) },
                        modifier = Modifier.fillMaxWidth(),
                        testTag = FerrexQaTags.Phone.libraryAction("clear-selected-cache"),
                    )
                }
                if (actions.changeServer) {
                    FerrexActionButton(
                        label = "Change server",
                        role = FerrexActionRole.Secondary,
                        onClick = { runAndDismiss(onChangeServer) },
                        modifier = Modifier.fillMaxWidth(),
                        testTag = FerrexQaTags.Phone.libraryAction("change-server"),
                    )
                }
                if (actions.resetConnection) {
                    FerrexActionButton(
                        label = "Reset connection",
                        role = FerrexActionRole.DestructiveReset,
                        onClick = { runAndDismiss(onResetConnection) },
                        modifier = Modifier.fillMaxWidth(),
                        testTag = FerrexQaTags.Phone.libraryAction("reset-connection"),
                    )
                }
                FerrexActionButton(
                    label = "Diagnostics / Export diagnostics",
                    role = FerrexActionRole.Secondary,
                    onClick = { runAndDismiss(onOpenDiagnostics) },
                    modifier = Modifier.fillMaxWidth(),
                    testTag = FerrexQaTags.Phone.libraryAction("diagnostics"),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) { Text("Close") }
        },
    )
}

@Composable
private fun LibraryEmptyStatePanel(
    selectedTab: HomeLibraryTab,
    selectedLibraryName: String?,
    freshness: LibraryFreshness,
    selectedLibraryId: String?,
    statusSummary: LibraryGridStatusSummary,
    density: FerrexStageDensityFamily,
    modifier: Modifier = Modifier,
    onRetry: () -> Unit,
    onClearSelected: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val actions = LibraryBrowseModels.recoveryActionVisibility(selectedLibraryId)
    val title = selectedLibraryName?.let { "No cached ${selectedTab.label.lowercase()} for $it" } ?: "No ${selectedTab.label.lowercase()} library selected"
    val body = buildList {
        add(statusSummary.detail)
        add("Retry sync, clear the selected cache, change server, reset connection, or open diagnostics from this panel without wiping app data.")
    }.joinToString(" ")
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.EmptyState,
        density = density,
        tone = freshness.statusTone().toStageSurfaceTone(),
        modifier = modifier.fillMaxWidth(),
        testTag = FerrexQaTags.Phone.LibraryRecovery,
        contentDescription = "$title. $body",
    ) {
        Column(
            modifier = Modifier.verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap),
        ) {
            TheaterPlateText(
                text = title,
                role = TheaterPlateTypographyRole.StatusTitle,
                densityRole = density.toTheaterPlateDensityRole(),
                color = MaterialTheme.colorScheme.primary,
                maxLines = 3,
            )
            TheaterPlateText(
                text = body,
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = density.toTheaterPlateDensityRole(),
                maxLines = 8,
            )
            if (actions.retry) {
                FerrexActionButton(
                    label = "Retry sync",
                    role = FerrexActionRole.Retry,
                    onClick = onRetry,
                    modifier = Modifier.fillMaxWidth(),
                    testTag = FerrexQaTags.Phone.libraryAction("retry"),
                )
            }
            if (actions.clearSelectedCache) {
                FerrexActionButton(
                    label = "Clear selected cache",
                    role = FerrexActionRole.Cache,
                    onClick = onClearSelected,
                    modifier = Modifier.fillMaxWidth(),
                    testTag = FerrexQaTags.Phone.libraryAction("clear-selected-cache"),
                )
            }
            if (density == FerrexStageDensityFamily.Compact) {
                Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                    if (actions.changeServer) {
                        FerrexActionButton(
                            label = "Change server",
                            role = FerrexActionRole.Secondary,
                            onClick = onChangeServer,
                            modifier = Modifier.fillMaxWidth(),
                            testTag = FerrexQaTags.Phone.libraryAction("change-server"),
                        )
                    }
                    if (actions.resetConnection) {
                        FerrexActionButton(
                            label = "Reset connection",
                            role = FerrexActionRole.DestructiveReset,
                            onClick = onResetConnection,
                            modifier = Modifier.fillMaxWidth(),
                            testTag = FerrexQaTags.Phone.libraryAction("reset-connection"),
                        )
                    }
                }
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                    if (actions.changeServer) {
                        FerrexActionButton(
                            label = "Change server",
                            role = FerrexActionRole.Secondary,
                            onClick = onChangeServer,
                            modifier = Modifier.weight(1f),
                            testTag = FerrexQaTags.Phone.libraryAction("change-server"),
                        )
                    }
                    if (actions.resetConnection) {
                        FerrexActionButton(
                            label = "Reset connection",
                            role = FerrexActionRole.DestructiveReset,
                            onClick = onResetConnection,
                            modifier = Modifier.weight(1f),
                            testTag = FerrexQaTags.Phone.libraryAction("reset-connection"),
                        )
                    }
                }
            }
            FerrexActionButton(
                label = "Diagnostics / Export diagnostics",
                role = FerrexActionRole.Secondary,
                onClick = onOpenDiagnostics,
                modifier = Modifier.fillMaxWidth(),
                testTag = FerrexQaTags.Phone.libraryAction("diagnostics"),
            )
        }
    }
}

@Composable
private fun DenseLibraryGrid(
    cards: List<LibraryMediaCard>,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    density: FerrexStageDensityFamily,
    modifier: Modifier = Modifier,
    onSelect: (LibraryMediaCard) -> Unit,
) {
    val gridSpec = FerrexDesignTokens.DenseLibraryGrid.phone
    FerrexMobileMediaGrid(
        gridKey = "library-grid",
        items = cards,
        itemStableId = { it.stableKey },
        columns = GridCells.Adaptive(minSize = gridSpec.minCellWidth),
        modifier = modifier.fillMaxSize(),
        testTag = FerrexQaTags.Phone.LibraryGrid,
        contentDescription = "Dense library media grid with ${cards.size} item${if (cards.size == 1) "" else "s"}; no enclosing rail band",
        contentPadding = PaddingValues(
            horizontal = gridSpec.contentPaddingHorizontal,
            vertical = gridSpec.contentPaddingVertical,
        ),
        horizontalArrangement = Arrangement.spacedBy(gridSpec.horizontalSpacing),
        verticalArrangement = Arrangement.spacedBy(gridSpec.verticalSpacing),
    ) { card, identity ->
        MediaCardView(
            card = card,
            imageResolutions = imageResolutions,
            imageLoaderAvailable = imageLoaderAvailable,
            imageLoader = imageLoader,
            scope = scope,
            density = density,
            semanticLabel = identity.semanticLabel(card.title),
            itemIdentity = identity,
            onClick = { onSelect(card) },
        )
    }
}

private data class LibraryGridStatusSummary(
    val label: String,
    val title: String,
    val detail: String,
    val tone: FerrexStatusTone,
)

private fun libraryGridStatusSummary(
    selectedTab: HomeLibraryTab,
    activeLibraryName: String?,
    visibleCount: Int,
    fullCachedCount: Int,
    freshness: LibraryFreshness,
    movieIndexState: MovieIndexUiState,
    movieSort: MovieSortMode,
    movieFilter: MovieFilterMode,
    invalidIndexCount: Int,
    appendedMissingCount: Int,
): LibraryGridStatusSummary {
    val cacheStatus = LibraryBrowseModels.libraryStatusCopy(freshness)
    val gridCount = if (fullCachedCount > 0 && visibleCount != fullCachedCount) "$visibleCount/$fullCachedCount" else "$visibleCount"
    val libraryCopy = activeLibraryName ?: "No library selected"
    val indexCopy = if (selectedTab == HomeLibraryTab.Movies) {
        movieIndexStatusCopy(
            movieIndexState = movieIndexState,
            totalCards = visibleCount,
            fullCachedCount = fullCachedCount,
            invalidIndexCount = invalidIndexCount,
            appendedMissingCount = appendedMissingCount,
        )
    } else {
        LibraryBrowseModels.unsupportedSeriesControlsCopy()
    }
    val movieControlsCopy = if (selectedTab == HomeLibraryTab.Movies) {
        " Selected movie controls: ${movieSort.label}; ${movieFilter.label}."
    } else {
        ""
    }
    val indexTone = if (selectedTab == HomeLibraryTab.Movies) movieIndexState.statusTone() else FerrexStatusTone.Secondary
    val tone = when {
        cacheStatus.isRecoverableError -> FerrexStatusTone.Error
        cacheStatus.isStale -> FerrexStatusTone.StaleOffline
        indexTone == FerrexStatusTone.Error -> FerrexStatusTone.Error
        indexTone == FerrexStatusTone.StaleOffline -> FerrexStatusTone.StaleOffline
        indexTone == FerrexStatusTone.Retry -> FerrexStatusTone.Retry
        else -> FerrexStatusTone.Cache
    }
    val label = when {
        cacheStatus.isRecoverableError -> "Recover"
        cacheStatus.isStale -> "Stale"
        selectedTab == HomeLibraryTab.Movies -> "Movies $gridCount"
        else -> "Series $gridCount"
    }
    return LibraryGridStatusSummary(
        label = label,
        title = "${selectedTab.label} library status",
        detail = "$libraryCopy • $gridCount visible cached item(s). ${cacheStatus.detail} $indexCopy$movieControlsCopy",
        tone = tone,
    )
}

private fun movieIndexStatusCopy(
    movieIndexState: MovieIndexUiState,
    totalCards: Int,
    fullCachedCount: Int,
    invalidIndexCount: Int,
    appendedMissingCount: Int,
): String {
    val base = when (movieIndexState) {
        MovieIndexUiState.Idle -> "Movie index idle; showing cached batch order."
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
    val reconciliation = if (invalidIndexCount > 0 || appendedMissingCount > 0) {
        " Index reconciliation kept the grid complete: $invalidIndexCount invalid index value(s), $appendedMissingCount cached item(s) appended."
    } else {
        ""
    }
    return base + reconciliation
}

private fun MovieIndexUiState.statusTone(): FerrexStatusTone = when (this) {
    is MovieIndexUiState.Error -> FerrexStatusTone.Error
    is MovieIndexUiState.Unsupported,
    is MovieIndexUiState.Unavailable -> FerrexStatusTone.StaleOffline
    MovieIndexUiState.Loading -> FerrexStatusTone.Retry
    else -> FerrexStatusTone.Cache
}

private fun FerrexStatusTone.toActionRole(): FerrexActionRole = when (this) {
    FerrexStatusTone.Primary -> FerrexActionRole.Primary
    FerrexStatusTone.Retry -> FerrexActionRole.Retry
    FerrexStatusTone.Secondary -> FerrexActionRole.Secondary
    FerrexStatusTone.Cache -> FerrexActionRole.Cache
    FerrexStatusTone.StaleOffline -> FerrexActionRole.StaleOffline
    FerrexStatusTone.DestructiveReset -> FerrexActionRole.DestructiveReset
    FerrexStatusTone.Error -> FerrexActionRole.Error
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
    val art = remember(card.imageKey, card.stableKey, card.title) {
        mobilePosterArt(
            imageKey = card.imageKey,
            fallbackPath = null,
            fallbackLabel = "No poster",
            surfaceKey = "continue-watching-hero",
            itemKey = card.stableKey,
            semanticLabel = card.title,
            grounding = MediaArtGrounding.TheaterPlateContactShadow,
        )
    }
    FerrexMobileMediaCard(
        title = card.title,
        subtitle = card.subtitle,
        metadata = "Continue Watching",
        art = art,
        resolution = card.imageKey?.let(imageResolutions::get),
        imageLoader = imageLoader.takeIf { imageLoaderAvailable },
        serverUrl = scope.canonicalServerUrl,
        density = density,
        layout = MobileMediaCardLayout.Hero,
        state = continueWatchingCardState(card, actionRole = FerrexActionRole.Primary),
        modifier = Modifier.fillMaxWidth(),
        contentDescription = "Continue Watching ${card.title}. ${card.progressLabel}. Action: Open",
        onClick = onClick,
    )
}

@Composable
private fun ContinueWatchingCardView(
    card: ContinueWatchingCard,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    density: FerrexStageDensityFamily,
    itemIdentity: MediaRailItemIdentity,
    semanticLabel: String = card.title,
    onClick: () -> Unit,
) {
    val art = remember(card.imageKey, itemIdentity.focusKey, semanticLabel) {
        mobilePosterArt(
            imageKey = card.imageKey,
            fallbackPath = null,
            fallbackLabel = "No poster",
            surfaceKey = "continue-watching-more",
            itemKey = itemIdentity.renderKey,
            semanticLabel = semanticLabel,
        )
    }
    FerrexMobileMediaCard(
        title = card.title,
        subtitle = card.subtitle,
        metadata = null,
        art = art,
        resolution = card.imageKey?.let(imageResolutions::get),
        imageLoader = imageLoader.takeIf { imageLoaderAvailable },
        serverUrl = scope.canonicalServerUrl,
        density = density,
        layout = MobileMediaCardLayout.Rail,
        state = continueWatchingCardState(card),
        modifier = Modifier.width(FerrexDesignTokens.Poster.PhoneWidth),
        contentDescription = "$semanticLabel. ${card.subtitle}. ${card.progressLabel}. Action: Open",
        onClick = onClick,
    )
}

@Composable
private fun HomeMediaRailCard(
    card: LibraryMediaCard,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    density: FerrexStageDensityFamily,
    itemIdentity: MediaRailItemIdentity,
    semanticLabel: String = card.title,
    onClick: () -> Unit,
) {
    val art = remember(card.imageKey, card.publicFallbackPath, itemIdentity.focusKey, semanticLabel) {
        mobilePosterArt(
            imageKey = card.imageKey,
            fallbackPath = card.publicFallbackPath,
            fallbackLabel = "No poster",
            surfaceKey = "home-shelf",
            itemKey = itemIdentity.renderKey,
            semanticLabel = semanticLabel,
        )
    }
    FerrexMobileMediaCard(
        title = card.title,
        subtitle = card.subtitle,
        metadata = card.libraryName,
        art = art,
        resolution = card.imageKey?.let(imageResolutions::get),
        imageLoader = imageLoader.takeIf { imageLoaderAvailable },
        serverUrl = scope.canonicalServerUrl,
        density = density,
        layout = MobileMediaCardLayout.CompactRail,
        state = libraryCardState(actionRole = FerrexActionRole.Secondary),
        modifier = Modifier.width(FerrexDesignTokens.Poster.PhoneCompactWidth),
        contentDescription = "$semanticLabel. ${card.subtitle}. ${card.libraryName}. Action: Open",
        onClick = onClick,
    )
}

@Composable
private fun MediaCardView(
    card: LibraryMediaCard,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: coil.ImageLoader?,
    scope: ServerCacheScope,
    density: FerrexStageDensityFamily = FerrexStageDensityFamily.Standard,
    semanticLabel: String = card.title,
    itemIdentity: MediaRailItemIdentity,
    onClick: () -> Unit,
) {
    val art = remember(card.imageKey, card.publicFallbackPath, itemIdentity.focusKey, semanticLabel) {
        mobilePosterArt(
            imageKey = card.imageKey,
            fallbackPath = card.publicFallbackPath,
            fallbackLabel = "No poster",
            surfaceKey = "library-grid",
            itemKey = itemIdentity.renderKey,
            semanticLabel = semanticLabel,
        )
    }
    FerrexMobileMediaCard(
        title = card.title,
        subtitle = card.subtitle,
        metadata = card.libraryName,
        art = art,
        resolution = card.imageKey?.let(imageResolutions::get),
        imageLoader = imageLoader.takeIf { imageLoaderAvailable },
        serverUrl = scope.canonicalServerUrl,
        density = density,
        layout = MobileMediaCardLayout.DenseGrid,
        state = libraryCardState(actionRole = FerrexActionRole.Secondary),
        modifier = Modifier.fillMaxWidth(),
        contentDescription = "$semanticLabel. ${card.subtitle}. ${card.libraryName}. Action: Open",
        onClick = onClick,
    )
}

private fun continueWatchingCardState(
    card: ContinueWatchingCard,
    actionRole: FerrexActionRole = FerrexActionRole.Secondary,
): MobileMediaCardState = MobileMediaCardState(
    progressFraction = card.progressFraction,
    progressLabel = card.progressLabel,
    watchState = card.progressState.toMobileWatchState(),
    actionLabel = "Open",
    actionRole = actionRole,
)

private fun ContinueWatchingProgressState.toMobileWatchState(): MobileMediaWatchState = when (this) {
    ContinueWatchingProgressState.Pending -> MobileMediaWatchState.Unwatched
    ContinueWatchingProgressState.InProgress -> MobileMediaWatchState.InProgress
    ContinueWatchingProgressState.Watched -> MobileMediaWatchState.Watched
}

private fun libraryCardState(actionRole: FerrexActionRole): MobileMediaCardState = MobileMediaCardState(
    actionLabel = "Open",
    actionRole = actionRole,
)

private fun mobilePosterArt(
    imageKey: ImageRequestKey?,
    fallbackPath: String?,
    fallbackLabel: String,
    surfaceKey: String,
    itemKey: String,
    semanticLabel: String,
    grounding: MediaArtGrounding = MediaArtGrounding.CardObject,
): MediaArtObject = MediaArtObject.forCategory(
    category = imageKey?.category ?: BrowseImageCategory.Poster,
    request = imageKey?.let { MediaArtRequest(it, fallbackPath) },
    fallbackLabel = fallbackLabel,
    targetIdentity = MediaArtTargetIdentity(
        surfaceKey = surfaceKey,
        itemKey = itemKey,
        semanticLabel = semanticLabel,
    ),
    grounding = grounding,
)

@Composable
private fun LibraryRecoveryPanel(
    freshness: LibraryFreshness,
    selectedLibraryId: String?,
    density: FerrexStageDensityFamily,
    onRetry: () -> Unit,
    onClearSelected: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val status = LibraryBrowseModels.libraryStatusCopy(freshness)
    val actions = LibraryBrowseModels.recoveryActionVisibility(selectedLibraryId)
    Column(verticalArrangement = Arrangement.spacedBy(density.tokens().surfaceGap)) {
        StateCard(
            title = status.title,
            body = status.detail,
            density = density,
            tone = freshness.statusTone(),
            variant = FerrexStageSurfaceVariant.StatusSlab,
        )
        FerrexStageSurface(
            variant = FerrexStageSurfaceVariant.ControlShelf,
            density = density,
            tone = freshness.statusTone().toStageSurfaceTone(),
            modifier = Modifier.fillMaxWidth(),
            testTag = FerrexQaTags.Phone.LibraryRecovery,
            contentDescription = "Library recovery Theater Plate actions",
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                if (actions.retry) {
                    FerrexActionButton(
                        label = "Retry",
                        role = FerrexActionRole.Retry,
                        onClick = onRetry,
                        modifier = Modifier.fillMaxWidth(),
                        testTag = FerrexQaTags.Phone.libraryAction("retry"),
                    )
                }
                if (actions.clearSelectedCache) {
                    FerrexActionButton(
                        label = "Clear selected cache",
                        role = FerrexActionRole.Cache,
                        onClick = onClearSelected,
                        modifier = Modifier.fillMaxWidth(),
                        testTag = FerrexQaTags.Phone.libraryAction("clear-selected-cache"),
                    )
                }
                if (actions.changeServer || actions.resetConnection) {
                    if (density == FerrexStageDensityFamily.Compact) {
                        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                            if (actions.changeServer) {
                                FerrexActionButton(
                                    label = "Change server",
                                    role = FerrexActionRole.Secondary,
                                    onClick = onChangeServer,
                                    modifier = Modifier.fillMaxWidth(),
                                    testTag = FerrexQaTags.Phone.libraryAction("change-server"),
                                )
                            }
                            if (actions.resetConnection) {
                                FerrexActionButton(
                                    label = "Reset connection",
                                    role = FerrexActionRole.DestructiveReset,
                                    onClick = onResetConnection,
                                    modifier = Modifier.fillMaxWidth(),
                                    testTag = FerrexQaTags.Phone.libraryAction("reset-connection"),
                                )
                            }
                        }
                    } else {
                        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                            if (actions.changeServer) {
                                FerrexActionButton(
                                    label = "Change server",
                                    role = FerrexActionRole.Secondary,
                                    onClick = onChangeServer,
                                    modifier = Modifier.weight(1f),
                                    testTag = FerrexQaTags.Phone.libraryAction("change-server"),
                                )
                            }
                            if (actions.resetConnection) {
                                FerrexActionButton(
                                    label = "Reset connection",
                                    role = FerrexActionRole.DestructiveReset,
                                    onClick = onResetConnection,
                                    modifier = Modifier.weight(1f),
                                    testTag = FerrexQaTags.Phone.libraryAction("reset-connection"),
                                )
                            }
                        }
                    }
                }
                FerrexActionButton(
                    label = "Diagnostics / Export diagnostics",
                    role = FerrexActionRole.Secondary,
                    onClick = onOpenDiagnostics,
                    modifier = Modifier.fillMaxWidth(),
                    testTag = FerrexQaTags.Phone.libraryAction("diagnostics"),
                )
            }
        }
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
    density: FerrexStageDensityFamily = FerrexStageDensityFamily.Standard,
    variant: FerrexStageSurfaceVariant = FerrexStageSurfaceVariant.StatusSlab,
) {
    FerrexStageSurface(
        variant = variant,
        density = density,
        tone = tone.toStageSurfaceTone(),
        modifier = Modifier.fillMaxWidth(),
        contentDescription = "$title. $body",
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
                    densityRole = density.toTheaterPlateDensityRole(),
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            TheaterPlateText(
                text = body,
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = density.toTheaterPlateDensityRole(),
                maxLines = 6,
            )
            action?.let { (label, callback) ->
                FerrexActionButton(
                    label = label,
                    role = actionRole,
                    onClick = callback,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

internal fun phoneHomeSignedInLine(
    displayName: String?,
    username: String,
    connectionTitle: String,
    serverUrl: String,
): String = "Signed in as ${displayName?.takeIf { it.isNotBlank() } ?: username} • $connectionTitle • $serverUrl"

internal fun phoneHomeShouldShowStatusNotices(
    connectionVisible: Boolean,
    freshness: LibraryFreshness,
): Boolean = connectionVisible || homeStageHasCacheRecoveryState(freshness)

private fun LibraryFreshness.statusTone(): FerrexStatusTone = when (this) {
    LibraryFreshness.Empty,
    LibraryFreshness.Syncing,
    is LibraryFreshness.Fresh -> FerrexStatusTone.Secondary
    is LibraryFreshness.StaleOffline,
    is LibraryFreshness.SeriesCacheIncomplete -> FerrexStatusTone.StaleOffline
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
    freshness is LibraryFreshness.StaleOffline ||
        freshness is LibraryFreshness.SeriesCacheIncomplete -> TheaterPlateColor.rgb(71, 85, 105)
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
    is LibraryFreshness.SeriesCacheIncomplete,
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
