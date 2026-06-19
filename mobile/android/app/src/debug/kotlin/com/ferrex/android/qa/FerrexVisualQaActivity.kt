package com.ferrex.android.qa

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.ferrex.android.BuildConfig
import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.auth.AuthenticatedConnectionSurface
import com.ferrex.android.core.auth.connectionRecoveryUi
import com.ferrex.android.core.library.CachedMediaReference
import com.ferrex.android.core.library.CachedMediaResyncSummary
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.mediaart.MediaArtObject
import com.ferrex.android.core.mediaart.MediaArtTargetIdentity
import com.ferrex.android.core.search.MediaSearchCache
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.MediaSearchTransport
import com.ferrex.android.core.search.SearchMediaId
import com.ferrex.android.core.search.SearchMediaType
import com.ferrex.android.core.search.SearchMediaWithStatus
import com.ferrex.android.core.theaterplate.TheaterPlateAnalysis
import com.ferrex.android.core.theaterplate.TheaterPlateColor
import com.ferrex.android.core.theaterplate.TheaterPlateDownsample
import com.ferrex.android.core.theaterplate.TheaterPlateGrade
import com.ferrex.android.core.theaterplate.TheaterPlateGradeClass
import com.ferrex.android.core.theaterplate.TheaterPlateGradeControls
import com.ferrex.android.core.theaterplate.TheaterPlateLocalLuma
import com.ferrex.android.core.theaterplate.TheaterPlatePalette
import com.ferrex.android.core.theaterplate.TheaterPlateSourceContext
import com.ferrex.android.core.theaterplate.TheaterPlateViewport
import com.ferrex.android.core.tvfocus.TvGridFocusPolicy
import com.ferrex.android.ui.components.FerrexActionButton
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexPosterCard
import com.ferrex.android.ui.components.FerrexMobileMediaCard
import com.ferrex.android.ui.components.FerrexMobileMediaGrid
import com.ferrex.android.ui.components.FerrexPosterPlaceholder
import com.ferrex.android.ui.components.FerrexStatusAction
import com.ferrex.android.ui.components.FerrexStatusCard
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.MobileMediaCardLayout
import com.ferrex.android.ui.components.MobileMediaCardState
import com.ferrex.android.ui.components.TheaterPlateDensityRole
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.components.colors
import com.ferrex.android.ui.components.statusTone
import com.ferrex.android.ui.detail.PhoneDetailScreen
import com.ferrex.android.ui.home.PhoneHomeScreen
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.qa.FerrexVisualQaFixtures
import com.ferrex.android.ui.qa.FerrexVisualQaLaunch
import com.ferrex.android.ui.qa.FerrexVisualQaScenarios
import com.ferrex.android.ui.qa.VisualQaDevice
import com.ferrex.android.ui.qa.VisualQaMediaCardSample
import com.ferrex.android.ui.qa.VisualQaRecoveryActionSample
import com.ferrex.android.ui.qa.VisualQaScenario
import com.ferrex.android.ui.qa.VisualQaScenarioKind
import com.ferrex.android.ui.qa.VisualQaTheaterPlateState
import com.ferrex.android.ui.recovery.PhoneRecoverableScreen
import com.ferrex.android.ui.search.PhoneSearchPanel
import com.ferrex.android.ui.theaterplate.FerrexStageDensityFamily
import com.ferrex.android.ui.theaterplate.FerrexStageSurface
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceTone
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceVariant
import com.ferrex.android.ui.theaterplate.TheaterPlateBackdropAdaptation
import com.ferrex.android.ui.theaterplate.TheaterPlateStage
import com.ferrex.android.ui.theme.FerrexDesignTokens
import com.ferrex.android.ui.theme.FerrexTheme

class FerrexVisualQaActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val scenarioId = FerrexVisualQaLaunch.resolveScenarioId(
            rawId = intent?.getStringExtra(FerrexVisualQaLaunch.EXTRA_SCENARIO_ID),
            isDebugBuild = BuildConfig.DEBUG,
        )
        if (scenarioId == null) {
            finish()
            return
        }
        val scenario = FerrexVisualQaScenarios.find(scenarioId) ?: FerrexVisualQaScenarios.defaultScenario
        setContent {
            FerrexTheme(tv = scenario.device == VisualQaDevice.Tv) {
                FerrexVisualQaRoot(initialScenario = scenario)
            }
        }
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
private fun FerrexVisualQaRoot(initialScenario: VisualQaScenario) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .semantics { testTagsAsResourceId = true }
            .background(MaterialTheme.colorScheme.background),
    ) {
        ScenarioHeader(scenario = initialScenario)
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f),
        ) {
            FerrexVisualQaScenarioContent(initialScenario)
        }
    }
}

@Composable
private fun ScenarioHeader(scenario: VisualQaScenario) {
    val compact = scenario.device == VisualQaDevice.Tv
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
            .padding(FerrexDesignTokens.Space.Md),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
    ) {
        Text(
            text = "Ferrex visual QA • ${scenario.id}",
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
            fontWeight = FontWeight.SemiBold,
        )
        if (!compact) {
            Text(
                text = "Launch with action ${FerrexVisualQaLaunch.ACTION_VISUAL_QA} and extra ${FerrexVisualQaLaunch.EXTRA_SCENARIO_ID}.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun FerrexVisualQaScenarioContent(scenario: VisualQaScenario) {
    when (scenario.kind) {
        VisualQaScenarioKind.PhoneHome -> PhoneScenarioFrame(scenario) {
            PhoneHomeScreen(
                state = FerrexVisualQaFixtures.staleAuthenticatedState,
                onSignOut = {},
                onChangeServer = {},
                onResetConnection = {},
                onRetryConnection = {},
                onOpenDiagnostics = {},
            )
        }
        VisualQaScenarioKind.PhoneSearch -> PhoneSearchScenario(scenario)
        VisualQaScenarioKind.PhoneBrowseGrid -> PhoneBrowseGridScenario(scenario)
        VisualQaScenarioKind.PhoneMovieDetail -> PhoneDetailScenario(
            scenario = scenario,
            detailResult = FerrexVisualQaFixtures.movieDetailResult,
            preparedPlayback = null,
        )
        VisualQaScenarioKind.PhoneSeriesDetail -> PhoneDetailScenario(
            scenario = scenario,
            detailResult = FerrexVisualQaFixtures.seriesDetailResult,
            preparedPlayback = null,
        )
        VisualQaScenarioKind.PhoneSeasonEpisode -> PhoneDetailScenario(
            scenario = scenario,
            detailResult = FerrexVisualQaFixtures.episodeDetailResult,
            preparedPlayback = null,
        )
        VisualQaScenarioKind.PhonePlaybackEntry -> PhoneDetailScenario(
            scenario = scenario,
            detailResult = FerrexVisualQaFixtures.movieDetailResult,
            preparedPlayback = FerrexVisualQaFixtures.playbackContract,
        )
        VisualQaScenarioKind.PhoneRecoveryOfflineStale -> PhoneScenarioFrame(scenario) {
            PhoneRecoverableScreen(
                state = FerrexVisualQaFixtures.recoverableFailureState,
                onRetry = {},
                onSignOut = {},
                onChangeServer = {},
                onResetConnection = {},
                onOpenDiagnostics = {},
            )
        }
        VisualQaScenarioKind.TvHomeFocus -> TvFocusScenario(
            scenario = scenario,
            title = "TV home actions",
            body = "D-pad focus targets stay reachable for search, retry, diagnostics, and account recovery.",
            surfaceKey = "home-actions",
            actions = listOf(
                VisualQaRecoveryActionSample("search", "Search", FerrexActionRole.Primary),
                VisualQaRecoveryActionSample("retry", "Retry connection", FerrexActionRole.Retry),
                VisualQaRecoveryActionSample("diagnostics", "Diagnostics", FerrexActionRole.Secondary),
            ),
        )
        VisualQaScenarioKind.TvGridFocus -> TvGridFocusScenario(scenario)
        VisualQaScenarioKind.TvDetailFocus -> TvFocusScenario(
            scenario = scenario,
            title = "TV detail actions",
            body = "Back, playback, watch-state, cache repair, and diagnostics focus targets remain visible.",
            surfaceKey = "detail-actions",
            actions = listOf(
                VisualQaRecoveryActionSample("back", "Back", FerrexActionRole.Secondary),
                VisualQaRecoveryActionSample("play", "Play", FerrexActionRole.Primary),
                VisualQaRecoveryActionSample("mark-watched", "Mark watched", FerrexActionRole.Secondary),
                VisualQaRecoveryActionSample("retry-cache", "Retry cache sync", FerrexActionRole.Cache),
                VisualQaRecoveryActionSample("diagnostics", "Diagnostics", FerrexActionRole.Secondary),
            ),
        )
        VisualQaScenarioKind.TvSearchFocus -> TvFocusScenario(
            scenario = scenario,
            title = "TV search focus",
            body = "Search input, retry, clear, result, and cache-miss actions use stable focus tags.",
            surfaceKey = "search-results",
            actions = listOf(
                VisualQaRecoveryActionSample("field", "Search field: qa", FerrexActionRole.Primary),
                VisualQaRecoveryActionSample("open-result", "Open Aurora Station", FerrexActionRole.Primary),
                VisualQaRecoveryActionSample("retry", "Retry sync / search", FerrexActionRole.Retry),
                VisualQaRecoveryActionSample("clear", "Clear search", FerrexActionRole.Secondary),
                VisualQaRecoveryActionSample("diagnostics", "Diagnostics", FerrexActionRole.Secondary),
            ),
        )
        VisualQaScenarioKind.TvRecoveryFocus -> TvFocusScenario(
            scenario = scenario,
            title = "TV recovery actions",
            body = "No-wipe recovery exits keep retry, sign out, server change, reset, and diagnostics reachable.",
            surfaceKey = "recovery-actions",
            actions = FerrexVisualQaFixtures.noWipeRecoveryActions,
            statusTone = FerrexStatusTone.StaleOffline,
        )
        VisualQaScenarioKind.TheaterPlate -> TheaterPlateScenario(
            scenario = scenario,
            state = requireNotNull(scenario.theaterPlateState) { "Theater Plate scenario requires state metadata" },
        )
    }
}

@Composable
private fun PhoneScenarioFrame(
    scenario: VisualQaScenario,
    content: @Composable () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .testTag(scenario.testTag)
            .semantics { contentDescription = scenario.description },
    ) {
        content()
    }
}

@Composable
private fun PhoneSearchScenario(scenario: VisualQaScenario) {
    val searchRepository = remember { MediaSearchRepository(StaticQaSearchTransport, StaticQaSearchCache) }
    val scope = rememberQaScope()
    QaScrollableScenario(scenario = scenario) {
        ScenarioTitle(scenario)
        FerrexStageSurface(
            variant = FerrexStageSurfaceVariant.StatusSlab,
            density = FerrexStageDensityFamily.Standard,
            tone = FerrexStageSurfaceTone.Cache,
            modifier = Modifier.fillMaxWidth(),
            contentDescription = "Deterministic search fixture",
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                TheaterPlateText(
                    text = "Deterministic search fixture",
                    role = TheaterPlateTypographyRole.StatusTitle,
                    densityRole = TheaterPlateDensityRole.PhoneLandscape,
                )
                TheaterPlateText(
                    text = "The query is preloaded with qa and resolves entirely from in-memory rows, including one cache miss with retry actions.",
                    role = TheaterPlateTypographyRole.StatusCopy,
                    densityRole = TheaterPlateDensityRole.PhoneLandscape,
                    maxLines = 4,
                )
            }
        }
        PhoneSearchPanel(
            scope = scope,
            searchRepository = searchRepository,
            imageRepository = null,
            imagePipeline = null,
            onOpenResult = {},
            onOpenDiagnostics = {},
            initialQuery = "qa",
            searchDebounceMillis = 0L,
        )
    }
}

@Composable
private fun PhoneBrowseGridScenario(scenario: VisualQaScenario) {
    Surface(
        modifier = Modifier
            .fillMaxSize()
            .testTag(scenario.testTag)
            .semantics { contentDescription = scenario.description },
        color = MaterialTheme.colorScheme.background,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(
                    horizontal = FerrexDesignTokens.Space.ScreenPhoneHorizontal,
                    vertical = FerrexDesignTokens.Space.ScreenPhoneVertical,
                ),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
        ) {
            ScenarioTitle(scenario)
            FerrexStageSurface(
                variant = FerrexStageSurfaceVariant.ControlShelf,
                density = FerrexStageDensityFamily.Standard,
                tone = FerrexStageSurfaceTone.Primary,
                modifier = Modifier.fillMaxWidth(),
                testTag = FerrexQaTags.Phone.LibraryTabs,
                contentDescription = "Phone browse compact controls: tabs, library chooser, sort/filter, status, and More recovery menu",
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
                ) {
                    FerrexActionButton(label = "Movies", role = FerrexActionRole.Primary, onClick = {})
                    FerrexActionButton(label = "Series", role = FerrexActionRole.Secondary, onClick = {})
                    FerrexActionButton(label = "Library: QA", role = FerrexActionRole.Cache, onClick = {})
                    FerrexActionButton(label = "Sort", role = FerrexActionRole.Secondary, onClick = {})
                    FerrexActionButton(label = "Filter", role = FerrexActionRole.Secondary, onClick = {})
                    FerrexActionButton(label = "Status", role = FerrexActionRole.Secondary, onClick = {})
                    FerrexActionButton(label = "More", role = FerrexActionRole.Secondary, onClick = {})
                }
            }
            FerrexStageSurface(
                variant = FerrexStageSurfaceVariant.RailBand,
                density = FerrexStageDensityFamily.Standard,
                tone = FerrexStageSurfaceTone.Neutral,
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f),
                testTag = FerrexQaTags.Phone.LibraryGrid,
                contentDescription = "Phone browse dense library grid; the grid owns the vertical scroll below compact controls",
            ) {
                val gridSpec = FerrexDesignTokens.DenseLibraryGrid.phone
                FerrexMobileMediaGrid(
                    gridKey = "phone-browse-grid-qa",
                    items = FerrexVisualQaFixtures.browseCards,
                    itemStableId = { it.stableKey },
                    columns = GridCells.Adaptive(minSize = gridSpec.minCellWidth),
                    modifier = Modifier.fillMaxSize(),
                    contentDescription = "Dense phone browse grid with ${FerrexVisualQaFixtures.browseCards.size} cards",
                    contentPadding = PaddingValues(
                        horizontal = gridSpec.contentPaddingHorizontal,
                        vertical = gridSpec.contentPaddingVertical,
                    ),
                    horizontalArrangement = Arrangement.spacedBy(gridSpec.horizontalSpacing),
                    verticalArrangement = Arrangement.spacedBy(gridSpec.verticalSpacing),
                ) { card, _ ->
                    PhoneBrowseCard(card)
                }
            }
        }
    }
}

@Composable
private fun PhoneBrowseCard(card: VisualQaMediaCardSample) {
    val art = remember(card.stableKey, card.imageLabel) {
        qaMobileMediaArt(
            surfaceKey = "phone-browse-grid",
            itemKey = card.stableKey,
            semanticLabel = card.title,
            fallbackLabel = card.imageLabel,
        )
    }
    FerrexMobileMediaCard(
        title = card.title,
        subtitle = card.subtitle,
        metadata = card.libraryName,
        art = art,
        resolution = null,
        imageLoader = null,
        serverUrl = "https://qa.invalid",
        layout = MobileMediaCardLayout.DenseGrid,
        state = MobileMediaCardState(
            actionLabel = "Open",
            actionRole = FerrexActionRole.Secondary,
        ),
        modifier = Modifier.fillMaxWidth(),
        testTag = FerrexQaTags.namespaced("phone", "poster", card.stableKey),
        contentDescription = "${card.title} ${card.subtitle}. Action: Open",
        onClick = {},
    )
}

private fun qaMobileMediaArt(
    surfaceKey: String,
    itemKey: String,
    semanticLabel: String,
    fallbackLabel: String,
): MediaArtObject = MediaArtObject.forCategory(
    category = BrowseImageCategory.Poster,
    request = null,
    fallbackLabel = fallbackLabel,
    targetIdentity = MediaArtTargetIdentity(
        surfaceKey = surfaceKey,
        itemKey = itemKey,
        semanticLabel = semanticLabel,
    ),
)

@Composable
private fun PhoneDetailScenario(
    scenario: VisualQaScenario,
    detailResult: com.ferrex.android.core.detail.DetailLoadResult,
    preparedPlayback: com.ferrex.android.core.playback.PlaybackRouteContract?,
) {
    val scope = rememberQaScope()
    val connectionStatus = FerrexVisualQaFixtures.staleAuthenticatedState.connectionRecoveryUi(AuthenticatedConnectionSurface.Detail)
    Box(
        modifier = Modifier
            .fillMaxSize()
            .testTag(scenario.testTag)
            .semantics { contentDescription = scenario.description },
    ) {
        PhoneDetailScreen(
            detailResult = detailResult,
            watchState = FerrexVisualQaFixtures.watchState,
            imageResolutions = emptyMap(),
            imageLoaderAvailable = false,
            imageLoader = null,
            scope = scope,
            preparedPlaybackContract = preparedPlayback,
            connectionStatus = connectionStatus,
            actionNotice = if (scenario.kind == VisualQaScenarioKind.PhonePlaybackEntry) {
                "Visual QA uses a prepared playback route without network tickets."
            } else {
                null
            },
            onBack = {},
            onRetryConnection = {},
            onRetryCacheSync = {},
            onClearSelectedCache = {},
            onChangeServer = {},
            onResetConnection = {},
            onRetryWatch = {},
            onRetryEpisodes = {},
            onClearProgress = {},
            onMarkMovieWatched = { _, _ -> },
            onMarkEpisodeWatched = { _, _ -> },
            onMarkSeriesWatched = { _, _ -> },
            onPlaybackContract = {},
            onOpenDiagnostics = {},
        )
    }
}

@Composable
private fun TheaterPlateScenario(
    scenario: VisualQaScenario,
    state: VisualQaTheaterPlateState,
) {
    val tv = scenario.device == VisualQaDevice.Tv
    val target = if (tv) "tv" else "phone"
    val densityRole = if (tv) TheaterPlateDensityRole.Tv1080p else TheaterPlateDensityRole.PhonePortrait
    val analysis = remember(state, tv) { qaTheaterPlateAnalysis(state, tv) }
    val stageDensity = remember(analysis) { FerrexStageDensityFamily.forViewport(analysis.context.viewport) }
    val adaptation = state.theaterPlateBackdropAdaptation()
    TheaterPlateStage(
        analysis = analysis,
        adaptation = adaptation,
        density = stageDensity,
        modifier = Modifier
            .fillMaxSize()
            .testTag(scenario.testTag),
        contentDescription = scenario.description,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState()),
            horizontalAlignment = if (tv) Alignment.CenterHorizontally else Alignment.Start,
            verticalArrangement = Arrangement.spacedBy(if (tv) FerrexDesignTokens.Space.Xl else FerrexDesignTokens.Space.Lg),
        ) {
            ScenarioTitle(scenario, centered = tv)
            if (tv) {
                val recoveryFirst = state == VisualQaTheaterPlateState.Recovery || state == VisualQaTheaterPlateState.StaleOffline
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
                    verticalAlignment = Alignment.Top,
                ) {
                    Column(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
                    ) {
                        if (recoveryFirst) {
                            TheaterPlateRail(
                                target = target,
                                state = state,
                                tv = true,
                                densityRole = densityRole,
                                density = stageDensity,
                                compact = true,
                            )
                            TheaterPlateStatus(target = target, state = state, density = stageDensity)
                        } else {
                            TheaterPlateStatus(target = target, state = state, density = stageDensity)
                            TheaterPlateRail(
                                target = target,
                                state = state,
                                tv = true,
                                densityRole = densityRole,
                                density = stageDensity,
                                compact = true,
                            )
                            TheaterPlateMediaCard(target = target, state = state, tv = true, densityRole = densityRole)
                        }
                    }
                    Column(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
                    ) {
                        if (state == VisualQaTheaterPlateState.Search) {
                            TheaterPlateSearchField(target = target, state = state, tv = true)
                        }
                        if (recoveryFirst) {
                            TheaterPlateActions(target = target, state = state, tv = true, includeSupportingActions = false)
                            TheaterPlateMediaCard(target = target, state = state, tv = true, densityRole = densityRole)
                            TheaterPlateActions(target = target, state = state, tv = true, includePrimary = false)
                        } else {
                            TheaterPlateActions(target = target, state = state, tv = true, includePrimary = true)
                        }
                    }
                }
            } else {
                val recoveryFirst = state == VisualQaTheaterPlateState.Recovery || state == VisualQaTheaterPlateState.StaleOffline
                TheaterPlateStatus(target = target, state = state, density = stageDensity)
                if (recoveryFirst) {
                    TheaterPlateActions(target = target, state = state, tv = false)
                }
                TheaterPlateMediaCard(target = target, state = state, tv = false, densityRole = densityRole)
                if (state == VisualQaTheaterPlateState.Search) {
                    TheaterPlateSearchField(target = target, state = state, tv = false)
                }
                if (!recoveryFirst) {
                    TheaterPlateActions(target = target, state = state, tv = false)
                }
                TheaterPlateRail(target = target, state = state, tv = false, densityRole = densityRole, density = stageDensity)
            }
        }
    }
}

@Composable
private fun TheaterPlateStatus(
    target: String,
    state: VisualQaTheaterPlateState,
    density: FerrexStageDensityFamily,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.StatusSlab,
        density = density,
        tone = state.tone.toStageSurfaceTone(),
        testTag = FerrexQaTags.TheaterPlate.status(target, state.key),
        contentDescription = "${state.label} status: ${state.statusCopy}",
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
            TheaterPlateText(
                text = state.label,
                role = TheaterPlateTypographyRole.StatusTitle,
                color = MaterialTheme.colorScheme.primary,
            )
            TheaterPlateText(
                text = state.statusCopy,
                role = TheaterPlateTypographyRole.StatusCopy,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = if (density == FerrexStageDensityFamily.TenFoot) 2 else 4,
            )
        }
    }
}

@Composable
private fun TheaterPlateMediaCard(
    target: String,
    state: VisualQaTheaterPlateState,
    tv: Boolean,
    densityRole: TheaterPlateDensityRole,
) {
    val tag = FerrexQaTags.TheaterPlate.media(target, state.key, "hero")
    val description = "Theater Plate media ${state.mediaTitle}: ${state.mediaSubtitle}"
    if (tv) {
        QaTvTheaterPlateMediaCard(
            tag = tag,
            contentDescription = description,
            title = state.mediaTitle,
            subtitle = state.mediaSubtitle,
            artworkLabel = state.artworkLabel,
            densityRole = densityRole,
        )
    } else {
        FerrexPosterCard(
            modifier = Modifier.fillMaxWidth(),
            testTag = tag,
            contentDescription = description,
            onClick = {},
        ) {
            Row(
                modifier = Modifier.padding(FerrexDesignTokens.Space.Md),
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                FerrexPosterPlaceholder(label = state.artworkLabel, modifier = Modifier.width(104.dp))
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
                    TheaterPlateText(text = state.mediaTitle, role = TheaterPlateTypographyRole.HeroTitle, densityRole = densityRole)
                    TheaterPlateText(text = state.mediaSubtitle, role = TheaterPlateTypographyRole.HeroSubtitle, densityRole = densityRole)
                    TheaterPlateText(text = state.summary, role = TheaterPlateTypographyRole.HeroBody, densityRole = densityRole, maxLines = 3)
                }
            }
        }
    }
}

@Composable
private fun QaTvTheaterPlateMediaCard(
    tag: String,
    contentDescription: String,
    title: String,
    subtitle: String,
    artworkLabel: String,
    densityRole: TheaterPlateDensityRole,
) {
    var focused by remember { mutableStateOf(false) }
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .widthIn(max = FerrexDesignTokens.Tv.DetailMaxWidth)
            .testTag(tag)
            .clickable(onClick = {})
            .onFocusChanged { focused = it.isFocused }
            .semantics(mergeDescendants = true) {
                role = Role.Button
                this.contentDescription = contentDescription
                onClick(label = contentDescription) { true }
            }
            .focusable(),
        shape = FerrexDesignTokens.Shapes.PosterCard,
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        border = BorderStroke(
            width = if (focused) FerrexDesignTokens.Focus.TvFocusedBorder else FerrexDesignTokens.Focus.TvRestingBorder,
            color = if (focused) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline.copy(alpha = 0.45f),
        ),
    ) {
        Row(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Lg),
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            FerrexPosterPlaceholder(label = artworkLabel, modifier = Modifier.width(154.dp))
            Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                TheaterPlateText(text = title, role = TheaterPlateTypographyRole.HeroTitle, densityRole = densityRole)
                TheaterPlateText(text = subtitle, role = TheaterPlateTypographyRole.HeroSubtitle, densityRole = densityRole)
            }
        }
    }
}

@Composable
private fun TheaterPlateSearchField(
    target: String,
    state: VisualQaTheaterPlateState,
    tv: Boolean,
) {
    val tag = FerrexQaTags.TheaterPlate.search(target, state.key, "field")
    val action = VisualQaRecoveryActionSample("search-field", "Search Theater Plate", FerrexActionRole.Primary)
    if (tv) {
        QaTvFocusableAction(
            surfaceKey = "theater-${state.key}",
            action = action,
            testTag = tag,
            contentDescription = "Search Theater Plate",
        )
    } else {
        FerrexActionButton(
            label = "Search Theater Plate",
            role = FerrexActionRole.Primary,
            onClick = {},
            modifier = Modifier.fillMaxWidth(),
            testTag = tag,
            contentDescription = "Search Theater Plate",
        )
    }
}

@Composable
private fun TheaterPlateActions(
    target: String,
    state: VisualQaTheaterPlateState,
    tv: Boolean,
    includePrimary: Boolean = true,
    includeSupportingActions: Boolean = true,
) {
    val primary = VisualQaRecoveryActionSample("primary", state.primaryActionLabel, FerrexActionRole.Primary)
    val actions = buildList {
        if (includePrimary || state == VisualQaTheaterPlateState.PlaybackEntry) add(primary)
        if (includeSupportingActions) {
            if (state == VisualQaTheaterPlateState.Recovery || state == VisualQaTheaterPlateState.StaleOffline) {
                val recoveryActions = FerrexVisualQaFixtures.noWipeCacheRecoveryActions.filterNot { action ->
                    // Phone Theater Plate QA already promotes retry as the primary action. Avoid a
                    // second adjacent Retry button so the gate screenshots stay calmer without
                    // removing the no-wipe recovery exits that distinguish this state.
                    !tv && includePrimary && action.key == "retry"
                }
                addAll(recoveryActions)
            } else {
                if (state == VisualQaTheaterPlateState.PlaybackEntry) {
                    add(
                        VisualQaRecoveryActionSample(
                            key = "network-required",
                            label = "Network playback requires a playback ticket",
                            role = FerrexActionRole.Secondary,
                            enabled = false,
                        ),
                    )
                    add(VisualQaRecoveryActionSample("start-over", "Start over", FerrexActionRole.Secondary))
                }
                add(VisualQaRecoveryActionSample("diagnostics", "Diagnostics / Export diagnostics", FerrexActionRole.Secondary))
            }
        }
    }
    if (tv) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
        ) {
            actions.forEach { action ->
                val key = action.key
                QaTvFocusableAction(
                    surfaceKey = "theater-${state.key}",
                    action = action,
                    testTag = FerrexQaTags.TheaterPlate.action(target, state.key, key),
                    contentDescription = action.label,
                )
            }
        }
    } else {
        Column(
            modifier = Modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        ) {
            actions.forEach { action ->
                val key = action.key
                FerrexActionButton(
                    label = action.label,
                    role = action.role,
                    onClick = {},
                    modifier = Modifier.fillMaxWidth(),
                    enabled = action.enabled,
                    testTag = FerrexQaTags.TheaterPlate.action(target, state.key, key),
                    contentDescription = action.label,
                )
            }
        }
    }
}

@Composable
private fun TheaterPlateRail(
    target: String,
    state: VisualQaTheaterPlateState,
    tv: Boolean,
    densityRole: TheaterPlateDensityRole,
    density: FerrexStageDensityFamily,
    compact: Boolean = false,
) {
    val tag = FerrexQaTags.TheaterPlate.rail(target, state.key, "primary")
    val description = "${state.label} rail with ${state.mediaTitle} and fallback artwork"
    FerrexStageSurface(
        variant = if (compact) FerrexStageSurfaceVariant.FactRibbon else FerrexStageSurfaceVariant.RailBand,
        density = density,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = Modifier
            .widthIn(max = if (tv) FerrexDesignTokens.Tv.DetailMaxWidth else 640.dp)
            .then(if (tv) Modifier.focusable() else Modifier),
        onClick = if (tv) ({}) else null,
        testTag = tag,
        contentDescription = description,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(if (compact) FerrexDesignTokens.Space.Xs else FerrexDesignTokens.Space.Sm)) {
            if (compact) {
                TheaterPlateText(
                    text = "Theater Plate rail • ${state.mediaTitle}",
                    role = TheaterPlateTypographyRole.RailTitle,
                    densityRole = densityRole,
                    maxLines = 1,
                )
                TheaterPlateText(
                    text = state.mediaSubtitle,
                    role = TheaterPlateTypographyRole.RailSubtitle,
                    densityRole = densityRole,
                    maxLines = 1,
                )
            } else {
                TheaterPlateText(text = "Theater Plate rail", role = TheaterPlateTypographyRole.SectionTitle, densityRole = densityRole)
                TheaterPlateText(text = state.mediaTitle, role = TheaterPlateTypographyRole.RailTitle, densityRole = densityRole)
                TheaterPlateText(text = state.mediaSubtitle, role = TheaterPlateTypographyRole.RailSubtitle, densityRole = densityRole)
            }
        }
    }
}

private fun qaTheaterPlateAnalysis(state: VisualQaTheaterPlateState, tv: Boolean): TheaterPlateAnalysis {
    val viewport = if (tv) TheaterPlateViewport.of(1920, 1080) else TheaterPlateViewport.of(393, 852)
    val gradeClass = state.qaGradeClass()
    val palette = state.qaPalette()
    val context = if (gradeClass == TheaterPlateGradeClass.MissingBackdrop) {
        TheaterPlateSourceContext.missingBackdrop(viewport).copy(
            posterColor = palette.dominant,
            themeColor = palette.accent,
            defaultColor = palette.stage,
        )
    } else {
        TheaterPlateSourceContext.backdrop(
            request = ImageRequestKey("qa-theater-plate-${state.key}", BrowseImageCategory.Backdrop),
            token = "qa-theater-plate-${state.key}",
            viewport = viewport,
        )
    }
    val controls = qaControlsFor(gradeClass)
    val grade = TheaterPlateGrade(
        gradeClass = gradeClass,
        isMissingBackdrop = gradeClass == TheaterPlateGradeClass.MissingBackdrop,
        isBright = gradeClass == TheaterPlateGradeClass.Bright,
        isDark = gradeClass == TheaterPlateGradeClass.Dark,
        isBusy = gradeClass == TheaterPlateGradeClass.Busy,
        isSaturated = gradeClass == TheaterPlateGradeClass.Saturated,
        isLowDetail = gradeClass == TheaterPlateGradeClass.LowDetail,
        controls = controls,
        stageColor = palette.stage,
    )
    return TheaterPlateAnalysis(
        context = context,
        sourceDimensions = viewport.width to viewport.height,
        downsample = qaDownsampleFor(gradeClass, palette),
        palette = palette,
        averageLuminance = when (gradeClass) {
            TheaterPlateGradeClass.Bright -> 0.84f
            TheaterPlateGradeClass.Dark -> 0.10f
            TheaterPlateGradeClass.Busy -> 0.48f
            TheaterPlateGradeClass.MissingBackdrop -> 0.22f
            else -> 0.38f
        },
        medianLuminance = when (gradeClass) {
            TheaterPlateGradeClass.Bright -> 0.82f
            TheaterPlateGradeClass.Dark -> 0.08f
            else -> 0.36f
        },
        p95Luminance = when (gradeClass) {
            TheaterPlateGradeClass.Bright -> 0.96f
            TheaterPlateGradeClass.Dark -> 0.18f
            else -> 0.72f
        },
        averageSaturation = if (gradeClass == TheaterPlateGradeClass.Saturated) 0.88f else 0.52f,
        edgeDensity = if (gradeClass == TheaterPlateGradeClass.Busy) 0.34f else 0.10f,
        edgeEnergy = if (gradeClass == TheaterPlateGradeClass.Busy) 0.20f else 0.06f,
        localLuma = qaLocalLumaFor(gradeClass),
        grade = grade,
    )
}

private fun VisualQaTheaterPlateState.qaGradeClass(): TheaterPlateGradeClass = when (this) {
    VisualQaTheaterPlateState.Bright -> TheaterPlateGradeClass.Bright
    VisualQaTheaterPlateState.Dark -> TheaterPlateGradeClass.Dark
    VisualQaTheaterPlateState.Busy -> TheaterPlateGradeClass.Busy
    VisualQaTheaterPlateState.MissingBackdrop -> TheaterPlateGradeClass.MissingBackdrop
    VisualQaTheaterPlateState.MissingArtwork -> TheaterPlateGradeClass.LowDetail
    VisualQaTheaterPlateState.StaleOffline,
    VisualQaTheaterPlateState.Recovery -> TheaterPlateGradeClass.Dark
    VisualQaTheaterPlateState.LongTitle,
    VisualQaTheaterPlateState.Search,
    VisualQaTheaterPlateState.Browse,
    VisualQaTheaterPlateState.Detail,
    VisualQaTheaterPlateState.Rails,
    VisualQaTheaterPlateState.PlaybackEntry -> TheaterPlateGradeClass.Balanced
}

private fun VisualQaTheaterPlateState.theaterPlateBackdropAdaptation(): TheaterPlateBackdropAdaptation = when (this) {
    VisualQaTheaterPlateState.MissingBackdrop -> TheaterPlateBackdropAdaptation.MissingBackdrop
    VisualQaTheaterPlateState.MissingArtwork -> TheaterPlateBackdropAdaptation.LowQuality
    VisualQaTheaterPlateState.StaleOffline,
    VisualQaTheaterPlateState.Recovery -> TheaterPlateBackdropAdaptation.StaleOffline
    else -> TheaterPlateBackdropAdaptation.Ready
}

private fun VisualQaTheaterPlateState.qaPalette(): TheaterPlatePalette = when (this) {
    VisualQaTheaterPlateState.Bright -> qaPalette(236, 244, 255, 103, 232, 249, 48, 65, 86, 22, 28, 36)
    VisualQaTheaterPlateState.Dark -> qaPalette(10, 14, 24, 167, 139, 250, 30, 41, 59, 8, 12, 18)
    VisualQaTheaterPlateState.Busy -> qaPalette(55, 65, 81, 251, 191, 36, 30, 41, 59, 14, 20, 28)
    VisualQaTheaterPlateState.MissingBackdrop -> qaPalette(49, 46, 129, 103, 232, 249, 30, 41, 59, 18, 20, 24)
    VisualQaTheaterPlateState.MissingArtwork -> qaPalette(31, 41, 55, 148, 163, 184, 15, 23, 42, 11, 18, 32)
    VisualQaTheaterPlateState.StaleOffline -> qaPalette(51, 65, 85, 148, 163, 184, 30, 41, 59, 12, 18, 28)
    VisualQaTheaterPlateState.Recovery -> qaPalette(127, 29, 29, 251, 113, 133, 49, 46, 129, 15, 23, 42)
    VisualQaTheaterPlateState.Search -> qaPalette(22, 78, 99, 103, 232, 249, 49, 46, 129, 9, 16, 28)
    VisualQaTheaterPlateState.Browse -> qaPalette(49, 46, 129, 167, 139, 250, 15, 23, 42, 11, 18, 32)
    VisualQaTheaterPlateState.Detail -> qaPalette(30, 64, 175, 103, 232, 249, 49, 46, 129, 8, 13, 23)
    VisualQaTheaterPlateState.Rails -> qaPalette(15, 23, 42, 167, 139, 250, 30, 41, 59, 8, 12, 18)
    VisualQaTheaterPlateState.PlaybackEntry -> qaPalette(12, 74, 110, 103, 232, 249, 49, 46, 129, 7, 13, 22)
    VisualQaTheaterPlateState.LongTitle -> qaPalette(30, 41, 59, 103, 232, 249, 49, 46, 129, 10, 16, 28)
}

private fun qaPalette(
    dominantR: Int,
    dominantG: Int,
    dominantB: Int,
    accentR: Int,
    accentG: Int,
    accentB: Int,
    mutedR: Int,
    mutedG: Int,
    mutedB: Int,
    stageR: Int,
    stageG: Int,
    stageB: Int,
): TheaterPlatePalette = TheaterPlatePalette(
    dominant = TheaterPlateColor.rgb(dominantR, dominantG, dominantB),
    accent = TheaterPlateColor.rgb(accentR, accentG, accentB),
    muted = TheaterPlateColor.rgb(mutedR, mutedG, mutedB),
    stage = TheaterPlateColor.rgb(stageR, stageG, stageB),
)

private fun qaControlsFor(gradeClass: TheaterPlateGradeClass): TheaterPlateGradeControls = when (gradeClass) {
    TheaterPlateGradeClass.MissingBackdrop -> TheaterPlateGradeControls(0.25f, 0.48f, 0.62f, 0.0f, 0.05f, 0.015f)
    TheaterPlateGradeClass.Busy -> TheaterPlateGradeControls(0.70f, 0.72f, 0.54f, 0.38f, 0.28f, 0.035f)
    TheaterPlateGradeClass.Bright -> TheaterPlateGradeControls(0.78f, 0.66f, 0.40f, 0.50f, 0.16f, 0.020f)
    TheaterPlateGradeClass.Dark -> TheaterPlateGradeControls(0.18f, 0.34f, 0.48f, 0.66f, 0.04f, 0.012f)
    TheaterPlateGradeClass.Saturated -> TheaterPlateGradeControls(0.38f, 0.54f, 0.50f, 0.58f, 0.22f, 0.018f)
    TheaterPlateGradeClass.LowDetail -> TheaterPlateGradeControls(0.30f, 0.46f, 0.62f, 0.34f, 0.08f, 0.020f)
    TheaterPlateGradeClass.Balanced -> TheaterPlateGradeControls(0.34f, 0.50f, 0.46f, 0.60f, 0.08f, 0.016f)
}

private fun qaDownsampleFor(
    gradeClass: TheaterPlateGradeClass,
    palette: TheaterPlatePalette,
): TheaterPlateDownsample {
    val colors = if (gradeClass == TheaterPlateGradeClass.Busy) {
        listOf(
            palette.dominant,
            palette.accent,
            palette.muted,
            palette.stage,
            TheaterPlateColor.rgb(226, 232, 240),
            palette.dominant,
            palette.accent,
            palette.muted,
            palette.stage,
            palette.accent,
            palette.muted,
            palette.dominant,
        )
    } else {
        List(12) { index ->
            when (index % 4) {
                0 -> palette.dominant
                1 -> palette.dominant.mix(palette.accent, 0.24f)
                2 -> palette.muted
                else -> palette.stage
            }
        }
    }
    return TheaterPlateDownsample(width = 4, height = 3, pixels = colors)
}

private fun qaLocalLumaFor(gradeClass: TheaterPlateGradeClass): TheaterPlateLocalLuma = when (gradeClass) {
    TheaterPlateGradeClass.Bright -> TheaterPlateLocalLuma(2, 2, listOf(0.78f, 0.86f, 0.72f, 0.82f), 0.72f, 0.86f)
    TheaterPlateGradeClass.Dark -> TheaterPlateLocalLuma(2, 2, listOf(0.08f, 0.12f, 0.06f, 0.10f), 0.06f, 0.12f)
    TheaterPlateGradeClass.Busy -> TheaterPlateLocalLuma(2, 2, listOf(0.16f, 0.78f, 0.68f, 0.22f), 0.16f, 0.78f)
    TheaterPlateGradeClass.MissingBackdrop -> TheaterPlateLocalLuma(2, 2, listOf(0.18f, 0.20f, 0.16f, 0.19f), 0.16f, 0.20f)
    else -> TheaterPlateLocalLuma(2, 2, listOf(0.24f, 0.42f, 0.34f, 0.48f), 0.24f, 0.48f)
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

@Composable
private fun TvFocusScenario(
    scenario: VisualQaScenario,
    title: String,
    body: String,
    surfaceKey: String,
    actions: List<VisualQaRecoveryActionSample>,
    statusTone: FerrexStatusTone = FerrexStatusTone.Cache,
) {
    QaScrollableScenario(scenario = scenario, tv = true) {
        ScenarioTitle(scenario, centered = true)
        FerrexStatusCard(
            title = title,
            body = body,
            tone = statusTone,
        )
        QaTvActionPanel(surfaceKey = surfaceKey, actions = actions)
    }
}

@Composable
private fun TvGridFocusScenario(scenario: VisualQaScenario) {
    QaScrollableScenario(scenario = scenario, tv = true) {
        ScenarioTitle(scenario, centered = true)
        FerrexStatusCard(
            title = "TV compact library grid",
            body = "Compact top controls open modal panels while dense poster cards fill the remaining browse surface.",
            tone = FerrexStatusTone.Cache,
        )
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(FerrexQaTags.Tv.surface(TvGridFocusPolicy.SURFACE_TOP_CONTROLS)),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
            ) {
                listOf(
                    VisualQaRecoveryActionSample("back", "Back", FerrexActionRole.Secondary),
                    VisualQaRecoveryActionSample("media-type", "Media: Movies", FerrexActionRole.Primary),
                    VisualQaRecoveryActionSample("library", "Library: QA Movies", FerrexActionRole.Primary),
                    VisualQaRecoveryActionSample("sort-filter", "Sort/filter", FerrexActionRole.Cache),
                    VisualQaRecoveryActionSample("status-more", "Status / More", FerrexActionRole.Cache),
                ).forEach { action ->
                    QaTvFocusableAction(
                        surfaceKey = TvGridFocusPolicy.SURFACE_TOP_CONTROLS,
                        action = action,
                        modifier = Modifier
                            .weight(1f)
                            .heightIn(min = FerrexDesignTokens.Focus.TvButtonMinHeight),
                    )
                }
            }
        }
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(FerrexQaTags.Tv.surface(TvGridFocusPolicy.SURFACE_CARDS)),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
        ) {
            FerrexVisualQaFixtures.browseCards.chunked(3).forEach { rowCards ->
                Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md)) {
                    rowCards.forEach { card ->
                        QaTvPosterCard(card, modifier = Modifier.width(164.dp))
                    }
                }
            }
        }
        QaTvActionPanel(
            surfaceKey = TvGridFocusPolicy.SURFACE_MOVIE_CONTROLS_PANEL,
            actions = listOf(
                VisualQaRecoveryActionSample("sort-titleasc", "Sort: Title A-Z", FerrexActionRole.Primary),
                VisualQaRecoveryActionSample("sort-releasedatedesc", "Sort: Release date", FerrexActionRole.Cache),
                VisualQaRecoveryActionSample("filter-all", "Filter: All movies", FerrexActionRole.Primary),
                VisualQaRecoveryActionSample("filter-highrated", "Filter: Rating 7+", FerrexActionRole.Cache),
                VisualQaRecoveryActionSample("close", "Close panel", FerrexActionRole.Secondary),
            ),
        )
        QaTvActionPanel(
            surfaceKey = TvGridFocusPolicy.SURFACE_STATUS_PANEL,
            actions = listOf(
                VisualQaRecoveryActionSample("sync-selected", "Retry selected library", FerrexActionRole.Retry),
                VisualQaRecoveryActionSample("retry-all", "Retry all libraries", FerrexActionRole.Retry),
                VisualQaRecoveryActionSample("clear-selected-cache", "Clear selected cache", FerrexActionRole.Cache),
                VisualQaRecoveryActionSample("clear-all-cache", "Clear all cache", FerrexActionRole.DestructiveReset),
                VisualQaRecoveryActionSample("change-server", "Change server", FerrexActionRole.Secondary),
                VisualQaRecoveryActionSample("reset-connection", "Reset connection", FerrexActionRole.DestructiveReset),
                VisualQaRecoveryActionSample("diagnostics", "Diagnostics / Export diagnostics", FerrexActionRole.Secondary),
                VisualQaRecoveryActionSample("close", "Close panel", FerrexActionRole.Secondary),
            ),
        )
    }
}

@Composable
private fun QaTvActionPanel(
    surfaceKey: String,
    actions: List<VisualQaRecoveryActionSample>,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Tv.surface(surfaceKey)),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
    ) {
        actions.forEach { action ->
            QaTvFocusableAction(surfaceKey = surfaceKey, action = action)
        }
    }
}

@Composable
private fun QaTvFocusableAction(
    surfaceKey: String,
    action: VisualQaRecoveryActionSample,
    testTag: String = FerrexQaTags.Tv.action(surfaceKey, action.key),
    contentDescription: String = action.label,
    modifier: Modifier = Modifier
        .widthIn(max = FerrexDesignTokens.Tv.ActionPanelMaxWidth)
        .fillMaxWidth()
        .heightIn(min = FerrexDesignTokens.Focus.TvButtonMinHeight),
) {
    var focused by remember { mutableStateOf(false) }
    val tone = action.role.statusTone()
    val colors = tone.colors()
    val enabled = action.enabled
    Surface(
        modifier = modifier
            .testTag(testTag)
            .clickable(enabled = enabled, role = Role.Button, onClick = {})
            .onFocusChanged { focused = it.isFocused }
            .semantics(mergeDescendants = true) {
                role = Role.Button
                this.contentDescription = contentDescription
                if (enabled) {
                    onClick(label = contentDescription) { true }
                } else {
                    disabled()
                }
            }
            .focusable(),
        shape = FerrexDesignTokens.Shapes.FocusSurface,
        color = when {
            !enabled -> colors.container.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContainer)
            focused -> colors.container.copy(alpha = 0.96f)
            else -> colors.container
        },
        contentColor = if (enabled) colors.content else colors.content.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContent),
        border = BorderStroke(
            width = if (focused) FerrexDesignTokens.Focus.TvFocusedBorder else FerrexDesignTokens.Focus.TvRestingBorder,
            color = when {
                !enabled -> MaterialTheme.colorScheme.onSurface.copy(alpha = 0.12f)
                focused -> colors.accent
                else -> colors.border
            },
        ),
    ) {
        Text(
            modifier = Modifier.padding(horizontal = FerrexDesignTokens.Space.Xxl, vertical = FerrexDesignTokens.Space.Lg),
            text = action.label,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun QaTvPosterCard(
    card: VisualQaMediaCardSample,
    modifier: Modifier = Modifier.fillMaxWidth(),
) {
    var focused by remember { mutableStateOf(false) }
    Card(
        modifier = modifier
            .testTag(card.testTag)
            .clickable(onClick = {})
            .onFocusChanged { focused = it.isFocused }
            .semantics(mergeDescendants = true) {
                val description = "${card.title} ${card.subtitle}"
                role = Role.Button
                contentDescription = description
                onClick(label = description) { true }
            }
            .focusable(),
        shape = FerrexDesignTokens.Shapes.PosterCard,
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        border = BorderStroke(
            width = if (focused) FerrexDesignTokens.Focus.TvFocusedBorder else FerrexDesignTokens.Focus.TvRestingBorder,
            color = if (focused) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline.copy(alpha = 0.45f),
        ),
    ) {
        Column(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Sm),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
        ) {
            FerrexPosterPlaceholder(label = card.imageLabel)
            Text(text = card.title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(text = card.subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            Text(text = card.libraryName, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary, maxLines = 1)
        }
    }
}

@Composable
private fun QaScrollableScenario(
    scenario: VisualQaScenario,
    tv: Boolean = scenario.device == VisualQaDevice.Tv,
    content: @Composable ColumnScope.() -> Unit,
) {
    Surface(
        modifier = Modifier
            .fillMaxSize()
            .testTag(scenario.testTag)
            .semantics { contentDescription = scenario.description },
        color = MaterialTheme.colorScheme.background,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(
                    horizontal = if (tv) FerrexDesignTokens.Space.ScreenTvHorizontal else FerrexDesignTokens.Space.ScreenPhoneHorizontal,
                    vertical = if (tv) FerrexDesignTokens.Space.ScreenTvVertical else FerrexDesignTokens.Space.ScreenPhoneVertical,
                ),
            horizontalAlignment = if (tv) Alignment.CenterHorizontally else Alignment.Start,
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
            content = content,
        )
    }
}

@Composable
private fun ScenarioTitle(
    scenario: VisualQaScenario,
    centered: Boolean = false,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .widthIn(max = if (scenario.device == VisualQaDevice.Tv) FerrexDesignTokens.Tv.DetailMaxWidth else 640.dp),
        horizontalAlignment = if (centered) Alignment.CenterHorizontally else Alignment.Start,
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
    ) {
        Text(
            text = scenario.title,
            style = if (scenario.device == VisualQaDevice.Tv) MaterialTheme.typography.displaySmall else MaterialTheme.typography.headlineMedium,
            color = MaterialTheme.colorScheme.primary,
            fontWeight = FontWeight.Bold,
            textAlign = if (centered) TextAlign.Center else TextAlign.Start,
            modifier = Modifier.fillMaxWidth(),
        )
        Text(
            text = scenario.description,
            style = MaterialTheme.typography.bodyLarge,
            textAlign = if (centered) TextAlign.Center else TextAlign.Start,
            modifier = Modifier.fillMaxWidth(),
        )
        Text(
            text = scenario.evidencePath,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = if (centered) TextAlign.Center else TextAlign.Start,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun rememberQaScope(): ServerCacheScope = remember {
    ServerCacheScope.from(FerrexVisualQaFixtures.ServerLabel, FerrexVisualQaFixtures.user.id)
}

private object StaticQaSearchTransport : MediaSearchTransport {
    override suspend fun queryMedia(searchText: String, limit: Int): ApiResult<List<SearchMediaWithStatus>> = ApiResult.Success(
        FerrexVisualQaFixtures.searchIds.take(limit).map { SearchMediaWithStatus(it) },
    )
}

private object StaticQaSearchCache : MediaSearchCache {
    override fun resolve(scope: ServerCacheScope, id: SearchMediaId): CachedMediaReference? = when (id.type) {
        SearchMediaType.Movie -> CachedMediaReference.Movie(
            id = FerrexVisualQaFixtures.MovieId,
            libraryId = FerrexVisualQaFixtures.MovieLibraryId,
            title = FerrexVisualQaFixtures.movieDetail.title,
            imageKey = null,
            publicFallbackPath = null,
        ).takeIf { id.id == FerrexVisualQaFixtures.MovieId }
        SearchMediaType.Series -> CachedMediaReference.Series(
            id = FerrexVisualQaFixtures.SeriesId,
            libraryId = FerrexVisualQaFixtures.SeriesLibraryId,
            title = FerrexVisualQaFixtures.seriesDetail.title,
            imageKey = null,
            publicFallbackPath = null,
        ).takeIf { id.id == FerrexVisualQaFixtures.SeriesId }
        SearchMediaType.Season -> null
        SearchMediaType.Episode -> null
    }

    override fun freshness(scope: ServerCacheScope): LibraryFreshness = LibraryFreshness.StaleOffline(
        message = "QA stale cache sample; retry remains visible.",
        itemCount = 2,
        lastSyncedAtMillis = null,
    )

    override suspend fun resync(scope: ServerCacheScope, id: SearchMediaId): CachedMediaResyncSummary = CachedMediaResyncSummary(
        attemptedLibraryIds = listOf(
            when (id.type) {
                SearchMediaType.Movie -> FerrexVisualQaFixtures.MovieLibraryId
                SearchMediaType.Series,
                SearchMediaType.Season,
                SearchMediaType.Episode -> FerrexVisualQaFixtures.SeriesLibraryId
            },
        ),
        bounded = false,
    )
}

