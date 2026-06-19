package com.ferrex.android.ui.qa

import androidx.compose.ui.graphics.Color
import com.ferrex.android.core.api.CurrentUser
import com.ferrex.android.core.auth.AuthConnectionHealth
import com.ferrex.android.core.auth.RecoverableFailureReason
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.detail.DetailImageSet
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.detail.EpisodeDetail
import com.ferrex.android.core.detail.EpisodesAvailability
import com.ferrex.android.core.detail.MovieDetail
import com.ferrex.android.core.detail.SeasonDetail
import com.ferrex.android.core.detail.SeriesBundleDetail
import com.ferrex.android.core.detail.SeriesDetail
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.search.SearchMediaId
import com.ferrex.android.core.tvfocus.TvGridFocusPolicy
import com.ferrex.android.core.search.SearchMediaType
import com.ferrex.android.core.watch.WatchEpisodeKey
import com.ferrex.android.core.watch.WatchEpisodeState
import com.ferrex.android.core.watch.WatchEpisodeStatus
import com.ferrex.android.core.watch.WatchMediaProgress
import com.ferrex.android.core.watch.WatchNextEpisode
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.core.watch.WatchSeasonStatus
import com.ferrex.android.core.watch.WatchSeriesStatus
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.requiredTheaterPlateRecoveryActions
import com.ferrex.android.ui.theme.FerrexDesignTokens

/** Stable Compose semantics tags used by visual QA, accessibility smoke tests, and manual runbooks. */
object FerrexQaTags {
    object Phone {
        const val Shell = "phone.shell"
        const val ShellNav = "phone.shell.nav"
        const val Home = "phone.home"
        const val HomeHeader = "phone.home.header"
        const val ContinueWatching = "phone.home.continue-watching"
        const val BrowseFind = "phone.home.browse-find"
        const val ServerRecovery = "phone.home.server-recovery"
        const val Libraries = "phone.libraries"
        const val LibraryTabs = "phone.libraries.tabs"
        const val LibraryChooser = "phone.libraries.chooser"
        const val LibraryControls = "phone.libraries.controls"
        const val LibraryIndexStatus = "phone.libraries.index-status"
        const val LibraryGrid = "phone.libraries.grid"
        const val LibraryRecovery = "phone.library.recovery"
        const val Search = "phone.search"
        const val SearchHeader = "phone.search.header"
        const val SearchPanel = "phone.search.panel"
        const val SearchField = "phone.search.field"
        const val SearchActions = "phone.search.actions"
        const val SearchResults = "phone.search.results"
        const val MovieDetail = "phone.detail.movie"
        const val SeriesDetail = "phone.detail.series"
        const val SeasonEpisode = "phone.detail.season-episode"
        const val PlaybackEntry = "phone.playback-entry"
        const val RecoveryOfflineStale = "phone.recovery.offline-stale"
        const val AccountServer = "phone.account-server"
        const val AccountSummary = "phone.account-server.summary"

        fun navItem(destination: String): String = namespaced("phone", "shell", "nav", destination)
        fun libraryAction(action: String): String = namespaced("phone", "libraries", "action", action)
        fun libraryControl(control: String): String = namespaced("phone", "libraries", "control", control)
        fun searchAction(action: String): String = namespaced("phone", "search", "action", action)
        fun searchResult(result: String): String = namespaced("phone", "search", "result", result)
        fun searchStatus(status: String): String = namespaced("phone", "search", "status", status)
    }

    object Tv {
        const val Home = "tv.home"
        const val Search = "tv.search"
        const val SearchField = "tv.search.field"
        const val SearchResults = "tv.search.results"
        const val Detail = "tv.detail"

        fun surface(surfaceKey: String): String = namespaced("tv", "surface", surfaceKey)
        fun action(surfaceKey: String, actionKey: String): String = namespaced("tv", "action", surfaceKey, actionKey)
        fun poster(surfaceKey: String, itemKey: String): String = namespaced("tv", "poster", surfaceKey, itemKey)
    }

    object TheaterPlate {
        fun root(target: String, stateKey: String): String = namespaced(target, "theater-plate", stateKey)
        fun status(target: String, stateKey: String): String = namespaced(target, "theater-plate", "status", stateKey)
        fun action(target: String, stateKey: String, actionKey: String): String =
            namespaced(target, "theater-plate", "action", stateKey, actionKey)
        fun media(target: String, stateKey: String, mediaKey: String): String =
            namespaced(target, "theater-plate", "media", stateKey, mediaKey)
        fun rail(target: String, stateKey: String, railKey: String): String =
            namespaced(target, "theater-plate", "rail", stateKey, railKey)
        fun search(target: String, stateKey: String, searchKey: String): String =
            namespaced(target, "theater-plate", "search", stateKey, searchKey)
    }

    object Shared {
        fun statusCard(id: String): String = namespaced("status-card", id)
    }

    fun namespaced(vararg parts: String): String = parts.joinToString(separator = ".") { segment(it) }

    fun segment(raw: String): String = tagUnsafeCharacters
        .replace(raw.lowercase(), "-")
        .trim('-')
        .ifBlank { "item" }

    private val tagUnsafeCharacters = Regex("[^a-z0-9_-]+")
}

data class VisualQaSurfaceSample(
    val id: String,
    val testTag: String,
    val contentDescription: String,
    val evidencePath: String,
)

data class VisualQaStatusToneSample(
    val id: String,
    val tone: FerrexStatusTone,
    val actionRole: FerrexActionRole,
    val container: Color,
    val content: Color,
    val accent: Color,
    val blendBackground: Color,
    val testTag: String,
    val contentDescription: String,
)

enum class VisualQaDevice {
    Phone,
    Tv,
}

enum class VisualQaScenarioKind {
    PhoneHome,
    PhoneSearch,
    PhoneBrowseGrid,
    PhoneMovieDetail,
    PhoneSeriesDetail,
    PhoneSeasonEpisode,
    PhonePlaybackEntry,
    PhoneRecoveryOfflineStale,
    TvHomeFocus,
    TvGridFocus,
    TvDetailFocus,
    TvSearchFocus,
    TvRecoveryFocus,
    TheaterPlate,
}

enum class VisualQaTheaterPlateState(
    val key: String,
    val label: String,
    val summary: String,
    val statusCopy: String,
    val primaryActionLabel: String,
    val mediaTitle: String,
    val mediaSubtitle: String,
    val artworkLabel: String,
    val tone: FerrexStatusTone,
) {
    Bright(
        key = "bright",
        label = "Bright backdrop",
        summary = "High-key artwork with cyan action contrast and readable foreground plate copy.",
        statusCopy = "Bright backdrops keep the Theater Plate scrim and controls legible.",
        primaryActionLabel = "Review bright contrast",
        mediaTitle = "Aurora Station",
        mediaSubtitle = "Bright nebula backdrop • 118 min",
        artworkLabel = "Bright backdrop",
        tone = FerrexStatusTone.Primary,
    ),
    Dark(
        key = "dark",
        label = "Dark backdrop",
        summary = "Low-key artwork with preserved edge separation and visible metadata.",
        statusCopy = "Dark backdrops keep the title, progress, and action row visible.",
        primaryActionLabel = "Review dark contrast",
        mediaTitle = "Night Relay",
        mediaSubtitle = "Dark corridor backdrop • 52 min",
        artworkLabel = "Dark backdrop",
        tone = FerrexStatusTone.Secondary,
    ),
    Busy(
        key = "busy",
        label = "Busy backdrop",
        summary = "Noisy artwork stress case for foreground grammar, text truncation, and control contrast.",
        statusCopy = "Busy backdrops must not overpower action/status nodes.",
        primaryActionLabel = "Reduce backdrop noise",
        mediaTitle = "Signal Bloom",
        mediaSubtitle = "Busy poster treatment • 44 min",
        artworkLabel = "Busy backdrop",
        tone = FerrexStatusTone.Cache,
    ),
    MissingBackdrop(
        key = "missing-backdrop",
        label = "Missing backdrop",
        summary = "Fallback gradient state when backdrop artwork is absent.",
        statusCopy = "Missing backdrops use the fallback plate instead of a blank surface.",
        primaryActionLabel = "Use fallback backdrop",
        mediaTitle = "Fallback Horizon",
        mediaSubtitle = "No backdrop available",
        artworkLabel = "Backdrop fallback",
        tone = FerrexStatusTone.StaleOffline,
    ),
    LongTitle(
        key = "long-title",
        label = "Long title",
        summary = "Long localized title wraps and truncates without hiding recovery or playback actions.",
        statusCopy = "The title block keeps hierarchy stable when copy is unusually long.",
        primaryActionLabel = "Open long-title detail",
        mediaTitle = "The Extremely Long Voyage of the Little Relay Ship That Kept Every Recovery Action Visible",
        mediaSubtitle = "Long title stress case • 126 min",
        artworkLabel = "Long title poster",
        tone = FerrexStatusTone.Primary,
    ),
    MissingArtwork(
        key = "missing-artwork",
        label = "Missing artwork",
        summary = "Poster and still fallbacks stay labeled when all artwork is unavailable.",
        statusCopy = "Missing artwork shows stable placeholders and semantic media labels.",
        primaryActionLabel = "Use artwork fallback",
        mediaTitle = "Placeholder Run",
        mediaSubtitle = "Poster and still unavailable",
        artworkLabel = "Artwork fallback",
        tone = FerrexStatusTone.Cache,
    ),
    StaleOffline(
        key = "stale-offline",
        label = "Stale/offline",
        summary = "Cached detail remains readable while offline status and retry paths stay visible.",
        statusCopy = "Stale offline data preserves context and no-wipe recovery actions.",
        primaryActionLabel = "Retry offline data",
        mediaTitle = "Cloudline Archives",
        mediaSubtitle = "Stale cache • last synced earlier",
        artworkLabel = "Offline cache poster",
        tone = FerrexStatusTone.StaleOffline,
    ),
    Recovery(
        key = "recovery",
        label = "Recovery",
        summary = "No-wipe recovery surface with retry, server change, reset, and diagnostics exits.",
        statusCopy = "Recovery paths remain visible without clearing app data.",
        primaryActionLabel = "Retry",
        mediaTitle = "Recovery Queue",
        mediaSubtitle = "Server unreachable",
        artworkLabel = "Recovery poster",
        tone = FerrexStatusTone.Error,
    ),
    Search(
        key = "search",
        label = "Search",
        summary = "Search entry, retry, clear, result, and diagnostics actions remain reachable.",
        statusCopy = "Search keeps field and results semantics stable across viewports.",
        primaryActionLabel = "Search again",
        mediaTitle = "Search Result: Aurora Station",
        mediaSubtitle = "Result row • Movie",
        artworkLabel = "Search result poster",
        tone = FerrexStatusTone.Primary,
    ),
    Browse(
        key = "browse",
        label = "Browse",
        summary = "Browse grid and related title entry points remain labeled in compact and 10-foot layouts.",
        statusCopy = "Browse surfaces expose deterministic media cards and actions.",
        primaryActionLabel = "Browse related titles",
        mediaTitle = "Browse: Cloudline Archives",
        mediaSubtitle = "Series • 1 season",
        artworkLabel = "Browse poster",
        tone = FerrexStatusTone.Secondary,
    ),
    Detail(
        key = "detail",
        label = "Detail",
        summary = "Detail action/status region with media metadata, progress, and repair actions.",
        statusCopy = "Detail semantics cover status, media, and action nodes.",
        primaryActionLabel = "Open detail actions",
        mediaTitle = "Detail: Signals in the Static",
        mediaSubtitle = "S1 E1 • resume available",
        artworkLabel = "Detail still",
        tone = FerrexStatusTone.Cache,
    ),
    Rails(
        key = "rails",
        label = "Rails",
        summary = "Rail cards keep focus, labels, and poster fallback semantics visible.",
        statusCopy = "Rails expose deterministic media nodes for visual and accessibility review.",
        primaryActionLabel = "Browse rail item",
        mediaTitle = "Rail: Continue Watching",
        mediaSubtitle = "Next episode rail",
        artworkLabel = "Rail poster",
        tone = FerrexStatusTone.Secondary,
    ),
    PlaybackEntry(
        key = "playback-entry",
        label = "Playback entry",
        summary = "Playback-entry state keeps resume, start over, and recovery exits available.",
        statusCopy = "Playback entry uses a prepared route while preserving no-wipe exits.",
        primaryActionLabel = "Resume playback",
        mediaTitle = "Aurora Station",
        mediaSubtitle = "Resume at 30:42",
        artworkLabel = "Playback poster",
        tone = FerrexStatusTone.Primary,
    ),
}

data class VisualQaScenario(
    val id: String,
    val device: VisualQaDevice,
    val kind: VisualQaScenarioKind,
    val title: String,
    val description: String,
    val testTag: String,
    val evidencePath: String,
    val fixtureSamples: List<String>,
    val recoveryActions: List<VisualQaRecoveryActionSample> = emptyList(),
    val theaterPlateState: VisualQaTheaterPlateState? = null,
)

data class VisualQaRecoveryActionSample(
    val key: String,
    val label: String,
    val role: FerrexActionRole,
    val requiresDataWipe: Boolean = false,
    val enabled: Boolean = true,
)

data class VisualQaMediaCardSample(
    val stableKey: String,
    val title: String,
    val subtitle: String,
    val libraryName: String,
    val route: MediaRouteArgs,
    val imageLabel: String,
    val testTag: String,
)

object FerrexQaScenarioIds {
    const val PhoneHome = "phone-home"
    const val PhoneSearch = "phone-search"
    const val PhoneBrowseGrid = "phone-browse-grid"
    const val PhoneMovieDetail = "phone-movie-detail"
    const val PhoneSeriesDetail = "phone-series-detail"
    const val PhoneSeasonEpisode = "phone-season-episode"
    const val PhonePlaybackEntry = "phone-playback-entry"
    const val PhoneRecoveryOfflineStale = "phone-recovery-offline-stale"
    const val TvHomeFocus = "tv-home-focus"
    const val TvGridFocus = "tv-grid-focus"
    const val TvDetailFocus = "tv-detail-focus"
    const val TvSearchFocus = "tv-search-focus"
    const val TvRecoveryFocus = "tv-recovery-focus"
    const val PhoneTheaterPlateBright = "phone-theater-plate-bright"
    const val PhoneTheaterPlateDark = "phone-theater-plate-dark"
    const val PhoneTheaterPlateBusy = "phone-theater-plate-busy"
    const val PhoneTheaterPlateMissingBackdrop = "phone-theater-plate-missing-backdrop"
    const val PhoneTheaterPlateLongTitle = "phone-theater-plate-long-title"
    const val PhoneTheaterPlateMissingArtwork = "phone-theater-plate-missing-artwork"
    const val PhoneTheaterPlateStaleOffline = "phone-theater-plate-stale-offline"
    const val PhoneTheaterPlateRecovery = "phone-theater-plate-recovery"
    const val PhoneTheaterPlateSearch = "phone-theater-plate-search"
    const val PhoneTheaterPlateBrowse = "phone-theater-plate-browse"
    const val PhoneTheaterPlateDetail = "phone-theater-plate-detail"
    const val PhoneTheaterPlateRails = "phone-theater-plate-rails"
    const val PhoneTheaterPlatePlaybackEntry = "phone-theater-plate-playback-entry"
    const val TvTheaterPlateBright = "tv-theater-plate-bright"
    const val TvTheaterPlateDark = "tv-theater-plate-dark"
    const val TvTheaterPlateBusy = "tv-theater-plate-busy"
    const val TvTheaterPlateMissingBackdrop = "tv-theater-plate-missing-backdrop"
    const val TvTheaterPlateLongTitle = "tv-theater-plate-long-title"
    const val TvTheaterPlateMissingArtwork = "tv-theater-plate-missing-artwork"
    const val TvTheaterPlateStaleOffline = "tv-theater-plate-stale-offline"
    const val TvTheaterPlateRecovery = "tv-theater-plate-recovery"
    const val TvTheaterPlateSearch = "tv-theater-plate-search"
    const val TvTheaterPlateBrowse = "tv-theater-plate-browse"
    const val TvTheaterPlateDetail = "tv-theater-plate-detail"
    const val TvTheaterPlateRails = "tv-theater-plate-rails"
    const val TvTheaterPlatePlaybackEntry = "tv-theater-plate-playback-entry"
}

object FerrexVisualQaLaunch {
    const val ACTION_VISUAL_QA = "com.ferrex.android.action.VISUAL_QA"
    const val EXTRA_SCENARIO_ID = "com.ferrex.android.extra.QA_SCENARIO_ID"

    fun isEnabled(isDebugBuild: Boolean): Boolean = isDebugBuild

    fun resolveScenarioId(rawId: String?, isDebugBuild: Boolean): String? {
        if (!isEnabled(isDebugBuild)) return null
        val requested = rawId?.trim()?.takeIf { it.isNotEmpty() }
        return FerrexVisualQaScenarios.find(requested)?.id ?: FerrexVisualQaScenarios.defaultScenario.id
    }
}

/** Deterministic, in-memory scenario registry consumed by the debug-only Android visual QA activity. */
object FerrexVisualQaScenarios {
    val requiredScenarioIds = listOf(
        FerrexQaScenarioIds.PhoneHome,
        FerrexQaScenarioIds.PhoneSearch,
        FerrexQaScenarioIds.PhoneBrowseGrid,
        FerrexQaScenarioIds.PhoneMovieDetail,
        FerrexQaScenarioIds.PhoneSeriesDetail,
        FerrexQaScenarioIds.PhoneSeasonEpisode,
        FerrexQaScenarioIds.PhonePlaybackEntry,
        FerrexQaScenarioIds.PhoneRecoveryOfflineStale,
        FerrexQaScenarioIds.TvHomeFocus,
        FerrexQaScenarioIds.TvGridFocus,
        FerrexQaScenarioIds.TvDetailFocus,
        FerrexQaScenarioIds.TvSearchFocus,
        FerrexQaScenarioIds.TvRecoveryFocus,
        FerrexQaScenarioIds.PhoneTheaterPlateBright,
        FerrexQaScenarioIds.PhoneTheaterPlateDark,
        FerrexQaScenarioIds.PhoneTheaterPlateBusy,
        FerrexQaScenarioIds.PhoneTheaterPlateMissingBackdrop,
        FerrexQaScenarioIds.PhoneTheaterPlateLongTitle,
        FerrexQaScenarioIds.PhoneTheaterPlateMissingArtwork,
        FerrexQaScenarioIds.PhoneTheaterPlateStaleOffline,
        FerrexQaScenarioIds.PhoneTheaterPlateRecovery,
        FerrexQaScenarioIds.PhoneTheaterPlateSearch,
        FerrexQaScenarioIds.PhoneTheaterPlateBrowse,
        FerrexQaScenarioIds.PhoneTheaterPlateDetail,
        FerrexQaScenarioIds.PhoneTheaterPlateRails,
        FerrexQaScenarioIds.PhoneTheaterPlatePlaybackEntry,
        FerrexQaScenarioIds.TvTheaterPlateBright,
        FerrexQaScenarioIds.TvTheaterPlateDark,
        FerrexQaScenarioIds.TvTheaterPlateBusy,
        FerrexQaScenarioIds.TvTheaterPlateMissingBackdrop,
        FerrexQaScenarioIds.TvTheaterPlateLongTitle,
        FerrexQaScenarioIds.TvTheaterPlateMissingArtwork,
        FerrexQaScenarioIds.TvTheaterPlateStaleOffline,
        FerrexQaScenarioIds.TvTheaterPlateRecovery,
        FerrexQaScenarioIds.TvTheaterPlateSearch,
        FerrexQaScenarioIds.TvTheaterPlateBrowse,
        FerrexQaScenarioIds.TvTheaterPlateDetail,
        FerrexQaScenarioIds.TvTheaterPlateRails,
        FerrexQaScenarioIds.TvTheaterPlatePlaybackEntry,
    )

    val all: List<VisualQaScenario> = listOf(
        scenario(
            id = FerrexQaScenarioIds.PhoneHome,
            device = VisualQaDevice.Phone,
            kind = VisualQaScenarioKind.PhoneHome,
            title = "Phone Theater Plate home",
            description = "Authenticated phone Home route on the Theater Plate stage; portrait and landscape/foldable QA keep resume, browse, search, and recovery actions visible.",
            testTag = FerrexQaTags.Phone.Home,
            evidencePath = "Debug Visual QA → Phone home → portrait + landscape/foldable",
            fixtureSamples = listOf("phone-portrait", "phone-landscape-foldable", "Theater Plate fallback stage", "Offline recovery card"),
        ),
        scenario(
            id = FerrexQaScenarioIds.PhoneSearch,
            device = VisualQaDevice.Phone,
            kind = VisualQaScenarioKind.PhoneSearch,
            title = "Phone Theater Plate search",
            description = "Phone Theater Plate search panel with staged query controls, deterministic resolved rows, and a cache-miss recovery row.",
            testTag = FerrexQaTags.Phone.Search,
            evidencePath = "Debug Visual QA → Phone Theater Plate search → query qa",
            fixtureSamples = listOf("qa", "Aurora Station", "Episode unavailable in cache"),
        ),
        scenario(
            id = FerrexQaScenarioIds.PhoneBrowseGrid,
            device = VisualQaDevice.Phone,
            kind = VisualQaScenarioKind.PhoneBrowseGrid,
            title = "Phone Theater Plate browse/grid",
            description = "Phone library browse sample with one compact control row and a dense grid owning the vertical scroll; recovery moves to status/More affordances.",
            testTag = FerrexQaTags.Phone.LibraryGrid,
            evidencePath = "Debug Visual QA → Phone Theater Plate browse/grid",
            fixtureSamples = FerrexVisualQaFixtures.browseCards.map { it.stableKey },
        ),
        scenario(
            id = FerrexQaScenarioIds.PhoneMovieDetail,
            device = VisualQaDevice.Phone,
            kind = VisualQaScenarioKind.PhoneMovieDetail,
            title = "Phone movie detail",
            description = "Movie detail surface with artwork placeholders, watch progress, playback, and recovery actions.",
            testTag = FerrexQaTags.Phone.MovieDetail,
            evidencePath = "Debug Visual QA → Phone movie detail",
            fixtureSamples = listOf(FerrexVisualQaFixtures.MovieId, FerrexVisualQaFixtures.MovieFileId),
        ),
        scenario(
            id = FerrexQaScenarioIds.PhoneSeriesDetail,
            device = VisualQaDevice.Phone,
            kind = VisualQaScenarioKind.PhoneSeriesDetail,
            title = "Phone series detail",
            description = "Series detail surface with next-episode, season summary, and watch-state recovery samples.",
            testTag = FerrexQaTags.Phone.SeriesDetail,
            evidencePath = "Debug Visual QA → Phone series detail",
            fixtureSamples = listOf(FerrexVisualQaFixtures.SeriesId, FerrexVisualQaFixtures.SeasonOneId),
        ),
        scenario(
            id = FerrexQaScenarioIds.PhoneSeasonEpisode,
            device = VisualQaDevice.Phone,
            kind = VisualQaScenarioKind.PhoneSeasonEpisode,
            title = "Phone season/episode",
            description = "Standalone episode detail for season/episode progress, resume, and mark-watched actions.",
            testTag = FerrexQaTags.Phone.SeasonEpisode,
            evidencePath = "Debug Visual QA → Phone season/episode",
            fixtureSamples = listOf(FerrexVisualQaFixtures.EpisodeOneId, FerrexVisualQaFixtures.EpisodeTwoId),
        ),
        scenario(
            id = FerrexQaScenarioIds.PhonePlaybackEntry,
            device = VisualQaDevice.Phone,
            kind = VisualQaScenarioKind.PhonePlaybackEntry,
            title = "Phone playback entry",
            description = "Playback-entry detail state with a prepared route contract and disabled network-free launch target.",
            testTag = FerrexQaTags.Phone.PlaybackEntry,
            evidencePath = "Debug Visual QA → Phone playback-entry",
            fixtureSamples = listOf(FerrexVisualQaFixtures.playbackContract.toDisplayString()),
        ),
        scenario(
            id = FerrexQaScenarioIds.PhoneRecoveryOfflineStale,
            device = VisualQaDevice.Phone,
            kind = VisualQaScenarioKind.PhoneRecoveryOfflineStale,
            title = "Phone recovery/offline/stale",
            description = "No-wipe phone recovery screen for stale or offline authenticated state.",
            testTag = FerrexQaTags.Phone.RecoveryOfflineStale,
            evidencePath = "Debug Visual QA → Phone recovery/offline/stale",
            fixtureSamples = listOf("qa-ferrex.invalid", "Retry", "Reset connection"),
            recoveryActions = FerrexVisualQaFixtures.noWipeRecoveryActions,
        ),
        scenario(
            id = FerrexQaScenarioIds.TvHomeFocus,
            device = VisualQaDevice.Tv,
            kind = VisualQaScenarioKind.TvHomeFocus,
            title = "TV Theater Plate home focus",
            description = "TV Theater Plate home stage with deterministic D-pad focus targets for search, retry, diagnostics, and recovery shelves.",
            testTag = FerrexQaTags.Tv.surface("home-actions"),
            evidencePath = "Debug Visual QA → TV home focus",
            fixtureSamples = listOf("tv.action.home-actions.search", "tv.action.home-actions.retry"),
        ),
        scenario(
            id = FerrexQaScenarioIds.TvGridFocus,
            device = VisualQaDevice.Tv,
            kind = VisualQaScenarioKind.TvGridFocus,
            title = "TV compact library grid focus",
            description = "TV full-library grid sample with compact top controls, dense poster cards, modal sort/filter and status/recovery panels.",
            testTag = FerrexQaTags.Tv.surface(TvGridFocusPolicy.SURFACE_TOP_CONTROLS),
            evidencePath = "Debug Visual QA → TV compact library grid",
            fixtureSamples = listOf(
                FerrexQaTags.Tv.action(TvGridFocusPolicy.SURFACE_TOP_CONTROLS, "media-type"),
                FerrexQaTags.Tv.action(TvGridFocusPolicy.SURFACE_TOP_CONTROLS, "sort-filter"),
                FerrexQaTags.Tv.surface(TvGridFocusPolicy.SURFACE_MOVIE_CONTROLS_PANEL),
                FerrexQaTags.Tv.surface(TvGridFocusPolicy.SURFACE_STATUS_PANEL),
                FerrexVisualQaFixtures.browseCards.first().testTag,
            ),
        ),
        scenario(
            id = FerrexQaScenarioIds.TvDetailFocus,
            device = VisualQaDevice.Tv,
            kind = VisualQaScenarioKind.TvDetailFocus,
            title = "TV detail focus",
            description = "TV detail action row with back, play, mark watched, and cache repair focus states.",
            testTag = FerrexQaTags.Tv.Detail,
            evidencePath = "Debug Visual QA → TV detail focus",
            fixtureSamples = listOf("tv.action.detail-actions.play", "tv.action.detail-actions.back"),
        ),
        scenario(
            id = FerrexQaScenarioIds.TvSearchFocus,
            device = VisualQaDevice.Tv,
            kind = VisualQaScenarioKind.TvSearchFocus,
            title = "TV search focus",
            description = "TV search field, search actions, and cache-miss recovery focus variants.",
            testTag = FerrexQaTags.Tv.Search,
            evidencePath = "Debug Visual QA → TV search focus",
            fixtureSamples = listOf("tv.search.field", "tv.action.search-results.retry"),
        ),
        scenario(
            id = FerrexQaScenarioIds.TvRecoveryFocus,
            device = VisualQaDevice.Tv,
            kind = VisualQaScenarioKind.TvRecoveryFocus,
            title = "TV recovery focus",
            description = "TV recovery action panel with retry, sign out, server change, reset, and diagnostics exits.",
            testTag = FerrexQaTags.Tv.surface("recovery-actions"),
            evidencePath = "Debug Visual QA → TV recovery focus",
            fixtureSamples = listOf("tv.action.recovery-actions.retry", "tv.action.recovery-actions.reset-connection"),
            recoveryActions = FerrexVisualQaFixtures.noWipeRecoveryActions,
        ),
    ) + theaterPlateScenarios(VisualQaDevice.Phone) + theaterPlateScenarios(VisualQaDevice.Tv)

    val defaultScenario: VisualQaScenario = all.first()

    fun find(id: String?): VisualQaScenario? = id?.trim()?.takeIf { it.isNotEmpty() }?.let { requested ->
        all.firstOrNull { it.id == requested }
    }

    private fun theaterPlateScenarios(device: VisualQaDevice): List<VisualQaScenario> = VisualQaTheaterPlateState.entries.map { state ->
        scenario(
            id = theaterPlateScenarioId(device, state),
            device = device,
            kind = VisualQaScenarioKind.TheaterPlate,
            title = "${device.label} Theater Plate • ${state.label}",
            description = state.summary,
            testTag = FerrexQaTags.TheaterPlate.root(device.targetKey, state.key),
            evidencePath = "Debug Visual QA → ${device.label} Theater Plate → ${state.label}",
            fixtureSamples = listOf(state.key, state.mediaTitle, state.primaryActionLabel),
            recoveryActions = if (state == VisualQaTheaterPlateState.Recovery || state == VisualQaTheaterPlateState.StaleOffline) {
                FerrexVisualQaFixtures.noWipeCacheRecoveryActions
            } else {
                emptyList()
            },
            theaterPlateState = state,
        )
    }

    private fun theaterPlateScenarioId(device: VisualQaDevice, state: VisualQaTheaterPlateState): String = when (device) {
        VisualQaDevice.Phone -> when (state) {
            VisualQaTheaterPlateState.Bright -> FerrexQaScenarioIds.PhoneTheaterPlateBright
            VisualQaTheaterPlateState.Dark -> FerrexQaScenarioIds.PhoneTheaterPlateDark
            VisualQaTheaterPlateState.Busy -> FerrexQaScenarioIds.PhoneTheaterPlateBusy
            VisualQaTheaterPlateState.MissingBackdrop -> FerrexQaScenarioIds.PhoneTheaterPlateMissingBackdrop
            VisualQaTheaterPlateState.LongTitle -> FerrexQaScenarioIds.PhoneTheaterPlateLongTitle
            VisualQaTheaterPlateState.MissingArtwork -> FerrexQaScenarioIds.PhoneTheaterPlateMissingArtwork
            VisualQaTheaterPlateState.StaleOffline -> FerrexQaScenarioIds.PhoneTheaterPlateStaleOffline
            VisualQaTheaterPlateState.Recovery -> FerrexQaScenarioIds.PhoneTheaterPlateRecovery
            VisualQaTheaterPlateState.Search -> FerrexQaScenarioIds.PhoneTheaterPlateSearch
            VisualQaTheaterPlateState.Browse -> FerrexQaScenarioIds.PhoneTheaterPlateBrowse
            VisualQaTheaterPlateState.Detail -> FerrexQaScenarioIds.PhoneTheaterPlateDetail
            VisualQaTheaterPlateState.Rails -> FerrexQaScenarioIds.PhoneTheaterPlateRails
            VisualQaTheaterPlateState.PlaybackEntry -> FerrexQaScenarioIds.PhoneTheaterPlatePlaybackEntry
        }
        VisualQaDevice.Tv -> when (state) {
            VisualQaTheaterPlateState.Bright -> FerrexQaScenarioIds.TvTheaterPlateBright
            VisualQaTheaterPlateState.Dark -> FerrexQaScenarioIds.TvTheaterPlateDark
            VisualQaTheaterPlateState.Busy -> FerrexQaScenarioIds.TvTheaterPlateBusy
            VisualQaTheaterPlateState.MissingBackdrop -> FerrexQaScenarioIds.TvTheaterPlateMissingBackdrop
            VisualQaTheaterPlateState.LongTitle -> FerrexQaScenarioIds.TvTheaterPlateLongTitle
            VisualQaTheaterPlateState.MissingArtwork -> FerrexQaScenarioIds.TvTheaterPlateMissingArtwork
            VisualQaTheaterPlateState.StaleOffline -> FerrexQaScenarioIds.TvTheaterPlateStaleOffline
            VisualQaTheaterPlateState.Recovery -> FerrexQaScenarioIds.TvTheaterPlateRecovery
            VisualQaTheaterPlateState.Search -> FerrexQaScenarioIds.TvTheaterPlateSearch
            VisualQaTheaterPlateState.Browse -> FerrexQaScenarioIds.TvTheaterPlateBrowse
            VisualQaTheaterPlateState.Detail -> FerrexQaScenarioIds.TvTheaterPlateDetail
            VisualQaTheaterPlateState.Rails -> FerrexQaScenarioIds.TvTheaterPlateRails
            VisualQaTheaterPlateState.PlaybackEntry -> FerrexQaScenarioIds.TvTheaterPlatePlaybackEntry
        }
    }

    private val VisualQaDevice.targetKey: String
        get() = when (this) {
            VisualQaDevice.Phone -> "phone"
            VisualQaDevice.Tv -> "tv"
        }

    private val VisualQaDevice.label: String
        get() = when (this) {
            VisualQaDevice.Phone -> "Phone"
            VisualQaDevice.Tv -> "TV"
        }

    private fun scenario(
        id: String,
        device: VisualQaDevice,
        kind: VisualQaScenarioKind,
        title: String,
        description: String,
        testTag: String,
        evidencePath: String,
        fixtureSamples: List<String>,
        recoveryActions: List<VisualQaRecoveryActionSample> = emptyList(),
        theaterPlateState: VisualQaTheaterPlateState? = null,
    ): VisualQaScenario = VisualQaScenario(
        id = id,
        device = device,
        kind = kind,
        title = title,
        description = description,
        testTag = testTag,
        evidencePath = evidencePath,
        fixtureSamples = fixtureSamples,
        recoveryActions = recoveryActions,
        theaterPlateState = theaterPlateState,
    )
}

/** Synthetic, in-memory fixture data. It intentionally avoids credentials, URLs, tokens, media paths, and private artwork. */
object FerrexVisualQaFixtures {
    const val ServerLabel = "qa-ferrex.invalid"
    const val MovieLibraryId = "qa-library-movies"
    const val SeriesLibraryId = "qa-library-series"
    const val MovieId = "qa-movie-aurora-station"
    const val SeriesId = "qa-series-cloudline"
    const val SeasonOneId = "qa-season-cloudline-01"
    const val EpisodeOneId = "qa-episode-cloudline-s01e01"
    const val EpisodeTwoId = "qa-episode-cloudline-s01e02"
    const val MovieFileId = "qa-file-aurora-station-main"
    const val EpisodeOneFileId = "qa-file-cloudline-s01e01"
    const val EpisodeTwoFileId = "qa-file-cloudline-s01e02"
    const val SeriesTmdbId = 99001L

    val user = CurrentUser(
        id = "qa-user-local",
        username = "visual_qa",
        displayName = "Visual QA",
        avatarUrl = null,
        email = null,
    )

    val staleAuthenticatedState = SessionState.Authenticated(
        serverUrl = ServerLabel,
        user = user,
        requiresPinSetup = false,
        connectionHealth = AuthConnectionHealth.Offline,
        offlineReason = RecoverableFailureReason.ServerUnreachable,
    )

    val recoverableFailureState = SessionState.RecoverableFailure(
        serverUrl = ServerLabel,
        reason = RecoverableFailureReason.ServerUnreachable,
    )

    val movieRoute = MediaRouteArgs(
        mediaType = BrowseMediaType.Movie,
        mediaId = MovieId,
        libraryId = MovieLibraryId,
        sourceSurface = BrowseSourceSurface.LibraryGrid,
    )

    val seriesRoute = MediaRouteArgs(
        mediaType = BrowseMediaType.Series,
        mediaId = SeriesId,
        libraryId = SeriesLibraryId,
        sourceSurface = BrowseSourceSurface.LibraryGrid,
    )

    val episodeRoute = MediaRouteArgs(
        mediaType = BrowseMediaType.Episode,
        mediaId = EpisodeOneId,
        libraryId = SeriesLibraryId,
        sourceSurface = BrowseSourceSurface.Search,
    )

    val movieDetail = MovieDetail(
        id = MovieId,
        libraryId = MovieLibraryId,
        title = "Aurora Station",
        overview = "A synthetic crew tests resilient playback, cached detail recovery, and poster fallbacks in an isolated QA fixture.",
        releaseDate = "2026-01-15",
        runtimeMinutes = 118,
        voteAverage = 8.1f,
        voteCount = 420,
        contentRating = "PG-13",
        genres = listOf("Adventure", "Mystery", "Science Fiction"),
        tagline = "Every route home stays visible.",
        status = "Released",
        tmdbId = 88001L,
        fileId = MovieFileId,
        fileName = null,
        images = DetailImageSet(
            poster = imageKey("qa-image-aurora-poster", BrowseImageCategory.Poster),
            backdrop = imageKey("qa-image-aurora-backdrop", BrowseImageCategory.Backdrop),
            posterFallbackPath = null,
            backdropFallbackPath = null,
        ),
    )

    val seriesDetail = SeriesDetail(
        id = SeriesId,
        libraryId = SeriesLibraryId,
        title = "Cloudline Archives",
        overview = "A deterministic series fixture covering season summaries, next-episode recovery, stale cache messaging, and TV focus rails.",
        firstAirDate = "2025-09-01",
        lastAirDate = null,
        availableSeasons = 1,
        availableEpisodes = 2,
        numberOfSeasons = 1,
        numberOfEpisodes = 2,
        voteAverage = 7.7f,
        voteCount = 118,
        contentRating = "TV-PG",
        genres = listOf("Drama", "Discovery"),
        tagline = "Cache first, recover without wiping.",
        status = "Returning Series",
        inProduction = true,
        tmdbId = SeriesTmdbId,
        images = DetailImageSet(
            poster = imageKey("qa-image-cloudline-poster", BrowseImageCategory.Poster),
            backdrop = imageKey("qa-image-cloudline-backdrop", BrowseImageCategory.Backdrop),
            posterFallbackPath = null,
            backdropFallbackPath = null,
        ),
    )

    val seasonOne = SeasonDetail(
        id = SeasonOneId,
        seasonNumber = 1,
        title = "Season 1",
        overview = "Synthetic season metadata for episode rows and season progress.",
        airDate = "2025-09-01",
        episodeCount = 2,
        runtimeMinutes = 45,
        images = DetailImageSet(
            poster = imageKey("qa-image-cloudline-s01-poster", BrowseImageCategory.Poster),
            posterFallbackPath = null,
        ),
    )

    val episodeOne = EpisodeDetail(
        id = EpisodeOneId,
        libraryId = SeriesLibraryId,
        seriesId = SeriesId,
        seasonNumber = 1,
        episodeNumber = 1,
        title = "Signals in the Static",
        overview = "The first synthetic episode keeps resume, clear-progress, and mark-watched actions visible.",
        airDate = "2025-09-01",
        runtimeMinutes = 44,
        tmdbSeriesId = SeriesTmdbId,
        fileId = EpisodeOneFileId,
        fileName = null,
        images = DetailImageSet(
            still = imageKey("qa-image-cloudline-e01-still", BrowseImageCategory.Episode),
            stillFallbackPath = null,
        ),
    )

    val episodeTwo = EpisodeDetail(
        id = EpisodeTwoId,
        libraryId = SeriesLibraryId,
        seriesId = SeriesId,
        seasonNumber = 1,
        episodeNumber = 2,
        title = "The Cache Remembers",
        overview = "The next-episode fixture validates stale/offline recovery without clearing app data.",
        airDate = "2025-09-08",
        runtimeMinutes = 47,
        tmdbSeriesId = SeriesTmdbId,
        fileId = EpisodeTwoFileId,
        fileName = null,
        images = DetailImageSet(
            still = imageKey("qa-image-cloudline-e02-still", BrowseImageCategory.Episode),
            stillFallbackPath = null,
        ),
    )

    val seriesBundleDetail = SeriesBundleDetail(
        series = seriesDetail,
        seasons = listOf(seasonOne),
        episodesBySeason = mapOf(1 to listOf(episodeOne, episodeTwo)),
        episodesAvailability = EpisodesAvailability.Available(2),
    )

    val movieDetailResult = DetailLoadResult.Movie(movieRoute, movieDetail)
    val seriesDetailResult = DetailLoadResult.Series(seriesRoute, seriesBundleDetail)
    val episodeDetailResult = DetailLoadResult.Episode(episodeRoute, episodeOne, seriesDetail)

    val playbackContract = PlaybackRouteContract(
        targetMediaId = MovieFileId,
        logicalMediaId = MovieId,
        mediaType = BrowseMediaType.Movie,
        startPositionSeconds = 1842.0,
        startOver = false,
        sourceDetailRoute = movieRoute.toRouteString(),
    )

    val watchState = WatchRepositoryState(
        media = mapOf(
            MovieId to WatchMediaProgress(
                mediaId = MovieId,
                positionSeconds = 1842.0,
                durationSeconds = 7080.0,
                percentage = 26.0,
                isCompleted = false,
            ),
            EpisodeOneId to WatchMediaProgress(
                mediaId = EpisodeOneId,
                positionSeconds = 1260.0,
                durationSeconds = 2640.0,
                percentage = 47.7,
                isCompleted = false,
            ),
            EpisodeTwoId to WatchMediaProgress.unwatched(EpisodeTwoId),
        ),
        series = mapOf(
            SeriesTmdbId to WatchSeriesStatus(
                tmdbSeriesId = SeriesTmdbId,
                totalEpisodes = 2,
                watched = 0,
                inProgress = 1,
                seasons = mapOf(
                    1 to WatchSeasonStatus(
                        seasonNumber = 1,
                        total = 2,
                        watched = 0,
                        inProgress = 1,
                        isCompleted = false,
                        episodes = mapOf(
                            1 to WatchEpisodeStatus(WatchEpisodeState.InProgress, progress = 0.48f),
                            2 to WatchEpisodeStatus(WatchEpisodeState.Unwatched),
                        ),
                    ),
                ),
                nextEpisode = WatchNextEpisode(
                    key = WatchEpisodeKey(SeriesTmdbId, seasonNumber = 1, episodeNumber = 2),
                    playableMediaId = EpisodeTwoFileId,
                    reason = "next_unwatched",
                ),
            ),
        ),
        nextEpisodes = mapOf(
            SeriesTmdbId to WatchNextEpisode(
                key = WatchEpisodeKey(SeriesTmdbId, seasonNumber = 1, episodeNumber = 2),
                playableMediaId = EpisodeTwoFileId,
                reason = "next_unwatched",
            ),
        ),
    )

    val browseCards = listOf(
        VisualQaMediaCardSample(
            stableKey = "movie:$MovieLibraryId:$MovieId",
            title = movieDetail.title,
            subtitle = "Movie • 2026 • 118 min",
            libraryName = "QA Movies",
            route = movieRoute,
            imageLabel = "Poster placeholder",
            testTag = FerrexQaTags.Tv.poster("grid-cards", "movie-aurora-station"),
        ),
        VisualQaMediaCardSample(
            stableKey = "series:$SeriesLibraryId:$SeriesId",
            title = seriesDetail.title,
            subtitle = "Series • 1 season • 2 episodes",
            libraryName = "QA Series",
            route = seriesRoute,
            imageLabel = "Series placeholder",
            testTag = FerrexQaTags.Tv.poster("grid-cards", "series-cloudline-archives"),
        ),
        VisualQaMediaCardSample(
            stableKey = "episode:$SeriesLibraryId:$EpisodeOneId",
            title = episodeOne.title,
            subtitle = "S1 E1 • resume available",
            libraryName = "QA Series",
            route = episodeRoute,
            imageLabel = "Episode placeholder",
            testTag = FerrexQaTags.Tv.poster("grid-cards", "episode-signals-static"),
        ),
    )

    val searchIds = listOf(
        SearchMediaId(SearchMediaType.Movie, MovieId),
        SearchMediaId(SearchMediaType.Series, SeriesId),
        SearchMediaId(SearchMediaType.Episode, "qa-episode-cache-miss"),
    )

    val noWipeRecoveryActions = requiredTheaterPlateRecoveryActions(includeCacheClear = false).map { action ->
        VisualQaRecoveryActionSample(action.key, action.label, action.role)
    }

    val noWipeCacheRecoveryActions = requiredTheaterPlateRecoveryActions(includeCacheClear = true).map { action ->
        VisualQaRecoveryActionSample(action.key, action.label, action.role)
    }

    fun privacyScanStrings(): List<String> = buildList {
        add(ServerLabel)
        add(user.id)
        add(user.username)
        user.displayName?.let(::add)
        listOf(movieDetail, seriesDetail, seasonOne, episodeOne, episodeTwo).forEach { item ->
            add(item.toString())
        }
        add(playbackContract.toDisplayString())
        browseCards.forEach { card ->
            add(card.stableKey)
            add(card.title)
            add(card.subtitle)
            add(card.libraryName)
            add(card.imageLabel)
        }
        (noWipeRecoveryActions + noWipeCacheRecoveryActions).forEach { action ->
            add(action.key)
            add(action.label)
        }
        VisualQaTheaterPlateState.entries.forEach { state ->
            add(state.key)
            add(state.label)
            add(state.summary)
            add(state.statusCopy)
            add(state.primaryActionLabel)
            add(state.mediaTitle)
            add(state.mediaSubtitle)
            add(state.artworkLabel)
        }
    }

    private fun imageKey(id: String, category: BrowseImageCategory): ImageRequestKey = ImageRequestKey(id, category)
}

/** Deterministic sample states consumed by unit checks and manual visual QA documentation. */
object FerrexVisualQaSamples {
    val phoneSurfaces: List<VisualQaSurfaceSample> = FerrexVisualQaScenarios.all
        .filter { it.device == VisualQaDevice.Phone }
        .map { it.toSurfaceSample() }

    val tvFocusableSurfaces: List<VisualQaSurfaceSample> = FerrexVisualQaScenarios.all
        .filter { it.device == VisualQaDevice.Tv }
        .map { it.toSurfaceSample() }

    val statusToneSamples = listOf(
        VisualQaStatusToneSample(
            id = "primary",
            tone = FerrexStatusTone.Primary,
            actionRole = FerrexActionRole.Primary,
            container = FerrexDesignTokens.Palette.SignalCyanDim.copy(alpha = FerrexDesignTokens.StatusAlpha.PrimaryContainer),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.SignalCyan,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("primary"),
            contentDescription = "Primary action/status tone",
        ),
        VisualQaStatusToneSample(
            id = "secondary",
            tone = FerrexStatusTone.Secondary,
            actionRole = FerrexActionRole.Secondary,
            container = FerrexDesignTokens.Palette.SlateElevated.copy(alpha = FerrexDesignTokens.StatusAlpha.SecondaryContainer),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.PrivateViolet,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("secondary"),
            contentDescription = "Secondary action/status tone",
        ),
        VisualQaStatusToneSample(
            id = "retry",
            tone = FerrexStatusTone.Retry,
            actionRole = FerrexActionRole.Retry,
            container = FerrexDesignTokens.Palette.SignalCyanDim.copy(alpha = 0.24f),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.SignalCyan,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("retry"),
            contentDescription = "Retry recovery tone",
        ),
        VisualQaStatusToneSample(
            id = "destructive-reset",
            tone = FerrexStatusTone.DestructiveReset,
            actionRole = FerrexActionRole.DestructiveReset,
            container = FerrexDesignTokens.Palette.ErrorDim.copy(alpha = 0.48f),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.Error,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("destructive-reset"),
            contentDescription = "Destructive reset recovery tone",
        ),
        VisualQaStatusToneSample(
            id = "cache",
            tone = FerrexStatusTone.Cache,
            actionRole = FerrexActionRole.Cache,
            container = FerrexDesignTokens.Palette.PrivateVioletDim.copy(alpha = 0.34f),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.PrivateViolet,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("cache"),
            contentDescription = "Cache repair tone",
        ),
        VisualQaStatusToneSample(
            id = "stale-offline",
            tone = FerrexStatusTone.StaleOffline,
            actionRole = FerrexActionRole.StaleOffline,
            container = FerrexDesignTokens.Palette.SlateElevated.copy(alpha = 0.52f),
            content = FerrexDesignTokens.Palette.TextSecondary,
            accent = FerrexDesignTokens.Palette.TextMuted,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("stale-offline"),
            contentDescription = "Stale or offline tone",
        ),
        VisualQaStatusToneSample(
            id = "error",
            tone = FerrexStatusTone.Error,
            actionRole = FerrexActionRole.Error,
            container = FerrexDesignTokens.Palette.ErrorDim.copy(alpha = 0.58f),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.Error,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("error"),
            contentDescription = "Error tone",
        ),
    )
}

private fun VisualQaScenario.toSurfaceSample(): VisualQaSurfaceSample = VisualQaSurfaceSample(
    id = id,
    testTag = testTag,
    contentDescription = description,
    evidencePath = evidencePath,
)
