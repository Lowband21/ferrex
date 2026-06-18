package com.ferrex.android.qa

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
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
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.search.MediaSearchCache
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.MediaSearchTransport
import com.ferrex.android.core.search.SearchMediaId
import com.ferrex.android.core.search.SearchMediaType
import com.ferrex.android.core.search.SearchMediaWithStatus
import com.ferrex.android.ui.components.FerrexActionButton
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexPosterCard
import com.ferrex.android.ui.components.FerrexPosterPlaceholder
import com.ferrex.android.ui.components.FerrexStatusAction
import com.ferrex.android.ui.components.FerrexStatusCard
import com.ferrex.android.ui.components.FerrexStatusTone
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
    var selectedScenario by remember { mutableStateOf(initialScenario) }
    Column(
        modifier = Modifier
            .fillMaxSize()
            .semantics { testTagsAsResourceId = true }
            .background(MaterialTheme.colorScheme.background),
    ) {
        ScenarioHeader(
            scenario = selectedScenario,
            onScenarioSelected = { selectedScenario = it },
        )
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f),
        ) {
            FerrexVisualQaScenarioContent(selectedScenario)
        }
    }
}

@Composable
private fun ScenarioHeader(
    scenario: VisualQaScenario,
    onScenarioSelected: (VisualQaScenario) -> Unit,
) {
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
            Row(
                modifier = Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
            ) {
                FerrexVisualQaScenarios.all.forEach { candidate ->
                    FerrexActionButton(
                        label = candidate.id,
                        role = if (candidate.id == scenario.id) FerrexActionRole.Primary else FerrexActionRole.Secondary,
                        onClick = { onScenarioSelected(candidate) },
                    ) {
                        Text(
                            text = candidate.id,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
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
        FerrexStatusCard(
            title = "Deterministic search fixture",
            body = "The query is preloaded with qa and resolves entirely from in-memory rows, including one cache miss with retry actions.",
            tone = FerrexStatusTone.Cache,
        )
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
    QaScrollableScenario(scenario = scenario) {
        ScenarioTitle(scenario)
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(FerrexQaTags.Phone.LibraryTabs),
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        ) {
            FerrexActionButton(label = "Movies", role = FerrexActionRole.Primary, onClick = {}, modifier = Modifier.weight(1f))
            FerrexActionButton(label = "Series", role = FerrexActionRole.Secondary, onClick = {}, modifier = Modifier.weight(1f))
        }
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(FerrexQaTags.Phone.LibraryGrid),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
        ) {
            FerrexVisualQaFixtures.browseCards.forEach { card ->
                PhoneBrowseCard(card)
            }
        }
        FerrexStatusCard(
            modifier = Modifier.testTag(FerrexQaTags.Phone.LibraryRecovery),
            title = "Stale cache recovery",
            body = "Retry sync, clear selected cache, change server, reset connection, and diagnostics remain visible without OS app-data wipes.",
            tone = FerrexStatusTone.StaleOffline,
            contentDescription = "Stale cache recovery: retry sync, cache repair, server change, reset connection, and diagnostics remain visible.",
            action = FerrexStatusAction(
                label = "Retry sync",
                role = FerrexActionRole.Retry,
                onClick = {},
            ),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            FerrexActionButton(label = "Clear selected cache", role = FerrexActionRole.Cache, onClick = {}, modifier = Modifier.weight(1f))
            FerrexActionButton(label = "Change server", role = FerrexActionRole.Secondary, onClick = {}, modifier = Modifier.weight(1f))
        }
        FerrexActionButton(
            label = "Reset connection",
            role = FerrexActionRole.DestructiveReset,
            onClick = {},
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun PhoneBrowseCard(card: VisualQaMediaCardSample) {
    FerrexPosterCard(
        modifier = Modifier.fillMaxWidth(),
        testTag = FerrexQaTags.namespaced("phone", "poster", card.stableKey),
        contentDescription = "${card.title} ${card.subtitle}",
        onClick = {},
    ) {
        Row(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Md),
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            FerrexPosterPlaceholder(label = card.imageLabel, modifier = Modifier.width(96.dp))
            Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
                Text(text = card.libraryName, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
                Text(text = card.title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(text = card.subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            FerrexActionButton(label = "Open", role = FerrexActionRole.Secondary, onClick = {})
        }
    }
}

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
    QaScrollableScenario(scenario = scenario, tv = tv) {
        ScenarioTitle(scenario, centered = tv)
        if (tv) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
                verticalAlignment = Alignment.Top,
            ) {
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
                ) {
                    TheaterPlateStatus(target = target, state = state)
                    TheaterPlateMediaCard(target = target, state = state, tv = true, densityRole = densityRole)
                    TheaterPlateRail(target = target, state = state, tv = true, densityRole = densityRole)
                }
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
                ) {
                    if (state == VisualQaTheaterPlateState.Search) {
                        TheaterPlateSearchField(target = target, state = state, tv = true)
                    }
                    TheaterPlateActions(target = target, state = state, tv = true)
                }
            }
        } else {
            TheaterPlateStatus(target = target, state = state)
            TheaterPlateMediaCard(target = target, state = state, tv = false, densityRole = densityRole)
            if (state == VisualQaTheaterPlateState.Search) {
                TheaterPlateSearchField(target = target, state = state, tv = false)
            }
            TheaterPlateActions(target = target, state = state, tv = false)
            TheaterPlateRail(target = target, state = state, tv = false, densityRole = densityRole)
        }
    }
}

@Composable
private fun TheaterPlateStatus(
    target: String,
    state: VisualQaTheaterPlateState,
) {
    FerrexStatusCard(
        modifier = Modifier.testTag(FerrexQaTags.TheaterPlate.status(target, state.key)),
        title = state.label,
        body = state.statusCopy,
        tone = state.tone,
        contentDescription = "${state.label} status: ${state.statusCopy}",
    )
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
            .onFocusChanged { focused = it.isFocused }
            .semantics(mergeDescendants = true) {
                role = Role.Button
                this.contentDescription = contentDescription
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
) {
    val primary = VisualQaRecoveryActionSample("primary", state.primaryActionLabel, FerrexActionRole.Primary)
    val actions = buildList {
        add(primary)
        if (state == VisualQaTheaterPlateState.Recovery || state == VisualQaTheaterPlateState.StaleOffline) {
            addAll(FerrexVisualQaFixtures.noWipeCacheRecoveryActions)
        } else {
            add(VisualQaRecoveryActionSample("diagnostics", "Diagnostics / Export diagnostics", FerrexActionRole.Secondary))
        }
        if (state == VisualQaTheaterPlateState.PlaybackEntry) {
            add(VisualQaRecoveryActionSample("start-over", "Start over", FerrexActionRole.Secondary))
        }
    }
    if (tv) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
        ) {
            actions.forEachIndexed { index, action ->
                val key = if (index == 0) "primary" else action.key
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
            actions.forEachIndexed { index, action ->
                val key = if (index == 0) "primary" else action.key
                FerrexActionButton(
                    label = action.label,
                    role = action.role,
                    onClick = {},
                    modifier = Modifier.fillMaxWidth(),
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
) {
    val tag = FerrexQaTags.TheaterPlate.rail(target, state.key, "primary")
    val description = "${state.label} rail with ${state.mediaTitle} and fallback artwork"
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .widthIn(max = if (tv) FerrexDesignTokens.Tv.DetailMaxWidth else 640.dp)
            .testTag(tag)
            .semantics(mergeDescendants = true) { contentDescription = description },
        shape = FerrexDesignTokens.Shapes.RecoveryCard,
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, MaterialTheme.colorScheme.outline.copy(alpha = 0.45f)),
    ) {
        Column(
            modifier = Modifier.padding(if (tv) FerrexDesignTokens.Space.Xl else FerrexDesignTokens.Space.Lg),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        ) {
            TheaterPlateText(text = "Theater Plate rail", role = TheaterPlateTypographyRole.SectionTitle, densityRole = densityRole)
            TheaterPlateText(text = state.mediaTitle, role = TheaterPlateTypographyRole.RailTitle, densityRole = densityRole)
            TheaterPlateText(text = state.mediaSubtitle, role = TheaterPlateTypographyRole.RailSubtitle, densityRole = densityRole)
        }
    }
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
            title = "TV poster grid",
            body = "Each poster card is focusable, has a deterministic tag, and carries synthetic media metadata only.",
            tone = FerrexStatusTone.Cache,
        )
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(FerrexQaTags.Tv.surface("grid-cards")),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
        ) {
            FerrexVisualQaFixtures.browseCards.forEach { card ->
                QaTvPosterCard(card)
            }
        }
        QaTvActionPanel(
            surfaceKey = "library-actions",
            actions = listOf(
                VisualQaRecoveryActionSample("browse-all", "Browse all", FerrexActionRole.Primary),
                VisualQaRecoveryActionSample("retry-library", "Retry selected library", FerrexActionRole.Retry),
                VisualQaRecoveryActionSample("clear-cache", "Clear selected cache", FerrexActionRole.Cache),
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
) {
    var focused by remember { mutableStateOf(false) }
    val tone = action.role.statusTone()
    val colors = tone.colors()
    Surface(
        modifier = Modifier
            .widthIn(max = FerrexDesignTokens.Tv.ActionPanelMaxWidth)
            .fillMaxWidth()
            .heightIn(min = FerrexDesignTokens.Focus.TvButtonMinHeight)
            .testTag(testTag)
            .onFocusChanged { focused = it.isFocused }
            .semantics(mergeDescendants = true) {
                role = Role.Button
                this.contentDescription = contentDescription
            }
            .focusable(),
        shape = FerrexDesignTokens.Shapes.FocusSurface,
        color = if (focused) colors.container.copy(alpha = 0.96f) else colors.container,
        contentColor = colors.content,
        border = BorderStroke(
            width = if (focused) FerrexDesignTokens.Focus.TvFocusedBorder else FerrexDesignTokens.Focus.TvRestingBorder,
            color = if (focused) colors.accent else colors.border,
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
private fun QaTvPosterCard(card: VisualQaMediaCardSample) {
    var focused by remember { mutableStateOf(false) }
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .testTag(card.testTag)
            .onFocusChanged { focused = it.isFocused }
            .semantics(mergeDescendants = true) {
                role = Role.Button
                contentDescription = "${card.title} ${card.subtitle}"
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
            FerrexPosterPlaceholder(label = card.imageLabel, modifier = Modifier.width(132.dp))
            Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                Text(text = card.title, style = MaterialTheme.typography.headlineSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(text = card.subtitle, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(text = card.libraryName, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
            }
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

