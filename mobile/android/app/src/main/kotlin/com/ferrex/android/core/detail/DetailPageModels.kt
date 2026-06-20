package com.ferrex.android.core.detail

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.cacheHealthSummary
import com.ferrex.android.core.mediaart.MediaArtGrounding
import com.ferrex.android.core.mediaart.MediaArtObject
import com.ferrex.android.core.mediaart.MediaArtRequest
import com.ferrex.android.core.mediaart.MediaArtTargetIdentity
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.watch.WatchEpisodeState
import com.ferrex.android.core.watch.WatchEpisodeStatus
import com.ferrex.android.core.watch.WatchMediaProgress
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.core.watch.WatchSeasonStatus
import com.ferrex.android.core.watch.WatchSeriesStatus

/**
 * Repository-free, shared model for Android phone and TV detail surfaces.
 *
 * Detail loaders continue to resolve cached movies/series/episodes. This layer normalizes the
 * resolved data into one render contract: hero art, overview/facts, watch and recovery actions,
 * explicit availability states, and bounded image prefetch keys for the visible detail surface.
 */
data class DetailPageModel(
    val stableKey: String,
    val kind: DetailPageKind,
    val route: MediaRouteArgs?,
    val title: String,
    val subtitle: String?,
    val overview: String?,
    val hero: DetailHero,
    val metadata: List<DetailMetadataItem>,
    val facts: List<DetailFactItem>,
    val watchState: DetailWatchState?,
    val actions: List<DetailPageAction>,
    val recovery: DetailRecoveryState,
    val rails: List<DetailRail>,
    val emptyState: DetailEmptyState? = null,
    val imagePrefetch: DetailImagePrefetchPlan,
) {
    val imageKeys: Set<ImageRequestKey> get() = imagePrefetch.keys

    fun rail(kind: DetailRailKind): DetailRail? = rails.firstOrNull { it.kind == kind }

    fun actionsOf(kind: DetailPageActionKind): List<DetailPageAction> =
        (actions + recovery.actions).filter { it.kind == kind }
}

enum class DetailPageKind {
    Movie,
    Series,
    Season,
    Episode,
    MissingDetail,
}

data class DetailHero(
    val background: DetailPageArt,
    val foreground: DetailPageArt?,
) {
    val imageKeys: List<ImageRequestKey> = listOfNotNull(background.requestKey, foreground?.requestKey)
}

data class DetailPageArt(
    val role: DetailArtRole,
    val label: String,
    val mediaArt: MediaArtObject?,
    val imageState: DetailImageState,
) {
    val requestKey: ImageRequestKey? get() = mediaArt?.requestKey
}

enum class DetailArtRole {
    Poster,
    Backdrop,
    Still,
    Profile,
    None,
}

sealed interface DetailImageState {
    val label: String
    val screenshotLabels: List<String>
    val staleOffline: Boolean

    data class Ready(
        override val label: String,
        override val staleOffline: Boolean,
        val offlineMessage: String? = null,
    ) : DetailImageState {
        override val screenshotLabels: List<String> = buildList {
            add(if (staleOffline) "Stale/offline" else "Manifest image")
            offlineMessage?.let { add("Offline: $it") }
        }
    }

    data class Pending(
        override val label: String,
        override val staleOffline: Boolean,
        val retryAfterMillis: Long? = null,
        val message: String = "Image manifest lookup pending.",
    ) : DetailImageState {
        override val screenshotLabels: List<String> = buildList {
            add("Pending")
            if (staleOffline) add("Stale/offline")
        }
    }

    data class Failed(
        override val label: String,
        override val staleOffline: Boolean,
        val reason: String,
        val retryable: Boolean,
    ) : DetailImageState {
        override val screenshotLabels: List<String> = buildList {
            add("Failed")
            if (staleOffline) add("Stale/offline")
        }
    }

    data class NoArt(
        override val label: String,
        val reason: String,
    ) : DetailImageState {
        override val staleOffline: Boolean = false
        override val screenshotLabels: List<String> = listOf("Missing artwork")
    }
}

data class DetailMetadataItem(
    val label: String,
    val tone: DetailTone = DetailTone.Neutral,
    val kind: DetailMetadataKind = DetailMetadataKind.Descriptive,
)

enum class DetailMetadataKind {
    Descriptive,
    WatchState,
    AudienceRating,
    Recovery,
}

data class DetailFactItem(
    val label: String,
    val value: String,
    val tone: DetailTone = DetailTone.Neutral,
)

enum class DetailTone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    Muted,
}

data class DetailWatchState(
    val scopeKey: String,
    val label: String,
    val state: DetailWatchStateKind,
    val progress: Float,
    val pendingMutation: Boolean,
    val message: String,
) {
    val watched: Boolean get() = state == DetailWatchStateKind.Watched
}

enum class DetailWatchStateKind {
    Unknown,
    Unwatched,
    InProgress,
    Watched,
    Unavailable,
}

data class DetailPageAction(
    val kind: DetailPageActionKind,
    val label: String,
    val role: DetailActionRole,
    val enabled: Boolean = true,
    val disabledReason: String? = null,
    val playbackContract: PlaybackRouteContract? = null,
    val targetId: String? = null,
    val targetWatched: Boolean? = null,
) {
    init {
        require(enabled || !disabledReason.isNullOrBlank()) {
            "Disabled detail actions must explain why they are unavailable"
        }
    }
}

enum class DetailPageActionKind {
    Resume,
    Play,
    StartOver,
    ClearProgress,
    MarkWatched,
    MarkUnwatched,
    RetryWatch,
    RetryCache,
    ClearSelectedCache,
    ClearAllCache,
    ChangeServer,
    ResetConnection,
    Diagnostics,
    Back,
}

enum class DetailActionRole {
    Primary,
    Secondary,
    Retry,
    Cache,
    DestructiveReset,
    Diagnostics,
    Back,
}

data class DetailRecoveryState(
    val freshness: DetailFreshnessNotice?,
    val actions: List<DetailPageAction>,
)

data class DetailFreshnessNotice(
    val kind: DetailFreshnessKind,
    val title: String,
    val message: String,
)

enum class DetailFreshnessKind {
    Fresh,
    Empty,
    Syncing,
    StaleOffline,
    RecoverableError,
}

data class DetailEmptyState(
    val title: String,
    val message: String,
)

data class DetailRail(
    val stableKey: String,
    val kind: DetailRailKind,
    val title: String,
    val state: DetailRailState,
    val cardKind: DetailRailCardKind,
    val activationPolicy: DetailRailActivationPolicy,
    val items: List<DetailRailItem>,
    val emptyMessage: String? = null,
    val unavailableMessage: String? = null,
) {
    val isAvailable: Boolean get() = state == DetailRailState.Available
}

enum class DetailRailKind {
    Seasons,
    Episodes,
    SiblingEpisodes,
    Cast,
    GuestCast,
    Crew,
    Recommendations,
    Similar,
}

enum class DetailRailState {
    Available,
    Empty,
    Unavailable,
}

enum class DetailRailCardKind {
    Poster,
    Still,
    Profile,
    Text,
}

enum class DetailRailActivationPolicy {
    Disabled,
    Navigate,
    Play,
}

data class DetailRailItem(
    val stableId: String,
    val title: String,
    val subtitle: String?,
    val badge: String?,
    val progress: Float?,
    val art: DetailPageArt,
    val route: MediaRouteArgs? = null,
    val playbackContract: PlaybackRouteContract? = null,
)

data class DetailImagePrefetchPolicy(
    val visibleRailItemWindow: Int = DEFAULT_VISIBLE_RAIL_ITEM_WINDOW,
    val maxImageKeys: Int = DEFAULT_MAX_IMAGE_KEYS,
) {
    init {
        require(visibleRailItemWindow >= 0) { "Visible rail item window must be non-negative" }
        require(maxImageKeys >= 0) { "Maximum image key count must be non-negative" }
    }

    companion object {
        const val DEFAULT_VISIBLE_RAIL_ITEM_WINDOW: Int = 12
        const val DEFAULT_MAX_IMAGE_KEYS: Int = 48
    }
}

data class DetailImagePrefetchPlan(
    val keys: Set<ImageRequestKey>,
    val visibleRailItemWindow: Int,
    val maxImageKeys: Int,
)

object DetailPageMapper {
    fun toPage(
        result: DetailLoadResult,
        watchState: WatchRepositoryState = WatchRepositoryState(),
        libraryFreshness: LibraryFreshness? = null,
        imageResolutions: Map<ImageRequestKey, ImageResolution> = emptyMap(),
        networkActionsEnabled: Boolean = true,
        networkActionMessage: String? = null,
        prefetchPolicy: DetailImagePrefetchPolicy = DetailImagePrefetchPolicy(),
    ): DetailPageModel = when (result) {
        is DetailLoadResult.Movie -> moviePage(
            result = result,
            watchState = watchState,
            libraryFreshness = libraryFreshness,
            imageResolutions = imageResolutions,
            networkActionsEnabled = networkActionsEnabled,
            networkActionMessage = networkActionMessage,
            prefetchPolicy = prefetchPolicy,
        )
        is DetailLoadResult.Series -> seriesPage(
            result = result,
            watchState = watchState,
            libraryFreshness = libraryFreshness,
            imageResolutions = imageResolutions,
            networkActionsEnabled = networkActionsEnabled,
            networkActionMessage = networkActionMessage,
            prefetchPolicy = prefetchPolicy,
        )
        is DetailLoadResult.Season -> seasonPage(
            route = result.route,
            series = result.series,
            season = result.season,
            episodes = result.episodes,
            watchState = watchState,
            libraryFreshness = libraryFreshness,
            imageResolutions = imageResolutions,
            networkActionsEnabled = networkActionsEnabled,
            networkActionMessage = networkActionMessage,
            prefetchPolicy = prefetchPolicy,
        )
        is DetailLoadResult.Episode -> episodePage(
            result = result,
            watchState = watchState,
            libraryFreshness = libraryFreshness,
            imageResolutions = imageResolutions,
            networkActionsEnabled = networkActionsEnabled,
            networkActionMessage = networkActionMessage,
            prefetchPolicy = prefetchPolicy,
        )
        is DetailLoadResult.Missing -> missingPage(
            result = result,
            libraryFreshness = libraryFreshness,
            prefetchPolicy = prefetchPolicy,
        )
    }

    fun seasonPage(
        route: MediaRouteArgs,
        series: SeriesDetail?,
        season: SeasonDetail,
        episodes: List<EpisodeDetail>,
        watchState: WatchRepositoryState = WatchRepositoryState(),
        libraryFreshness: LibraryFreshness? = null,
        imageResolutions: Map<ImageRequestKey, ImageResolution> = emptyMap(),
        networkActionsEnabled: Boolean = true,
        networkActionMessage: String? = null,
        prefetchPolicy: DetailImagePrefetchPolicy = DetailImagePrefetchPolicy(),
    ): DetailPageModel {
        val seriesStatus = watchState.seriesStatus(series?.tmdbId)
        val seasonStatus = seriesStatus?.seasons?.get(season.seasonNumber)
        val pageKey = "season:${season.id}"
        val images = DetailImageSet(
            poster = season.images.poster ?: series?.images?.poster,
            backdrop = series?.images?.backdrop ?: season.images.backdrop,
            still = season.images.still,
            posterFallbackPath = season.images.posterFallbackPath ?: series?.images?.posterFallbackPath,
            backdropFallbackPath = series?.images?.backdropFallbackPath ?: season.images.backdropFallbackPath,
            stillFallbackPath = season.images.stillFallbackPath,
        )
        val hero = heroFor(pageKey, season.title, images, imageResolutions)
        val resumeEpisode = episodes.firstOrNull { episode ->
            val progress = watchState.mediaProgress(episode.id)
            progress != null && !progress.isCompleted && progress.positionSeconds > 0.0 && episode.playbackTargetId != null
        }
        val firstPlayableEpisode = episodes.firstOrNull { it.playbackTargetId != null }
        val resumeContract = resumeEpisode?.let { episode ->
            DetailRouteContracts.episodeResume(episode, watchState.mediaProgress(episode.id), route)
        }
        val startOverContract = firstPlayableEpisode?.let { DetailRouteContracts.episodeStartOver(it, route) }
        val rails = listOf(
            episodesRail(
                stableKey = "$pageKey:episodes",
                title = "Episodes",
                route = route,
                episodes = episodes,
                availability = if (episodes.isEmpty()) {
                    EpisodesAvailability.Unavailable("No episodes are cached for this season. Retry episodes to refresh the current series bundle.")
                } else {
                    EpisodesAvailability.Available(episodes.size)
                },
                watchState = watchState,
                seriesStatus = seriesStatus,
                imageResolutions = imageResolutions,
            ),
        )
        val actions = buildList {
            addAll(
                playbackActions(
                    resumeContract = resumeContract,
                    startOverContract = startOverContract,
                    watched = seasonStatus?.isCompleted == true,
                    playLabel = "Play season",
                    unavailableCopy = "Playback is unavailable because this season cache does not include a playable episode file.",
                    networkActionsEnabled = networkActionsEnabled,
                    networkActionMessage = networkActionMessage,
                ),
            )
            seasonStatus?.let { status ->
                add(
                    watchToggleAction(
                        watched = status.isCompleted,
                        pending = seriesStatus?.pendingMutation == true,
                        networkActionsEnabled = networkActionsEnabled,
                        networkActionMessage = networkActionMessage,
                        targetId = series?.tmdbId?.toString() ?: season.id,
                    ),
                )
            }
            if (series?.tmdbId != null && seasonStatus == null) {
                add(retryWatchAction(networkActionsEnabled, networkActionMessage))
            }
        }
        return page(
            stableKey = pageKey,
            kind = DetailPageKind.Season,
            route = route,
            title = season.title,
            subtitle = series?.title,
            overview = season.overview,
            hero = hero,
            metadata = seasonMetadata(season, seasonStatus),
            facts = seasonFacts(season),
            watchState = seasonWatchState(season, seasonStatus, series?.tmdbId, watchState.lastError),
            actions = actions,
            recovery = recoveryState(route, libraryFreshness),
            rails = rails,
            prefetchPolicy = prefetchPolicy,
        )
    }

    fun imageKeys(
        result: DetailLoadResult?,
        prefetchPolicy: DetailImagePrefetchPolicy = DetailImagePrefetchPolicy(),
    ): Set<ImageRequestKey> = result?.let { toPage(it, prefetchPolicy = prefetchPolicy).imageKeys }.orEmpty()

    private fun moviePage(
        result: DetailLoadResult.Movie,
        watchState: WatchRepositoryState,
        libraryFreshness: LibraryFreshness?,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
        prefetchPolicy: DetailImagePrefetchPolicy,
    ): DetailPageModel {
        val movie = result.detail
        val progress = watchState.mediaProgress(movie.id)
        val resumeContract = DetailRouteContracts.movieResume(movie, progress, result.route)
        val startOverContract = DetailRouteContracts.movieStartOver(movie, result.route)
        val hero = heroFor("movie:${movie.id}", movie.title, movie.images, imageResolutions)
        return page(
            stableKey = "movie:${movie.id}",
            kind = DetailPageKind.Movie,
            route = result.route,
            title = movie.title,
            subtitle = movie.tagline,
            overview = movie.overview,
            hero = hero,
            metadata = movieMetadata(movie, progress),
            facts = movieFacts(movie),
            watchState = mediaWatchState(movie.id, "Movie watch state", progress, watchState.lastError),
            actions = buildList {
                addAll(
                    playbackActions(
                        resumeContract = resumeContract,
                        startOverContract = startOverContract,
                        watched = progress?.isCompleted == true,
                        playLabel = if (progress?.isCompleted == true) "Play again" else "Play",
                        unavailableCopy = "Playback is unavailable because this movie does not have a playable file in the cache.",
                        networkActionsEnabled = networkActionsEnabled,
                        networkActionMessage = networkActionMessage,
                    ),
                )
                addAll(
                    mediaWatchActions(
                        mediaId = movie.id,
                        progress = progress,
                        watched = progress?.isCompleted == true,
                        pending = progress?.pendingMutation == true,
                        networkActionsEnabled = networkActionsEnabled,
                        networkActionMessage = networkActionMessage,
                    ),
                )
            },
            recovery = recoveryState(result.route, libraryFreshness),
            rails = listOf(
                castRail("movie:${movie.id}:cast", "Cast", DetailRailKind.Cast, movie.cast, imageResolutions),
                crewRail("movie:${movie.id}:crew", movie.crew, imageResolutions),
                relatedRail("movie:${movie.id}:recommendations", "Recommendations", DetailRailKind.Recommendations, movie.recommendations, imageResolutions),
                relatedRail("movie:${movie.id}:similar", "Similar", DetailRailKind.Similar, movie.similar, imageResolutions),
            ),
            prefetchPolicy = prefetchPolicy,
        )
    }

    private fun seriesPage(
        result: DetailLoadResult.Series,
        watchState: WatchRepositoryState,
        libraryFreshness: LibraryFreshness?,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
        prefetchPolicy: DetailImagePrefetchPolicy,
    ): DetailPageModel {
        val detail = result.detail
        val series = detail.series
        val seriesStatus = watchState.seriesStatus(series.tmdbId)
        val nextEpisode = seriesStatus?.nextEpisode ?: series.tmdbId?.let { watchState.nextEpisodes[it] }
        val nextContract = DetailRouteContracts.seriesNext(detail, nextEpisode, result.route)
        val startOverContract = DetailRouteContracts.seriesStartOver(detail, result.route)
        val hero = heroFor("series:${series.id}", series.title, series.images, imageResolutions)
        return page(
            stableKey = "series:${series.id}",
            kind = DetailPageKind.Series,
            route = result.route,
            title = series.title,
            subtitle = series.tagline,
            overview = series.overview,
            hero = hero,
            metadata = seriesMetadata(series, seriesStatus),
            facts = seriesFacts(series),
            watchState = seriesWatchState(series, seriesStatus, watchState.lastError),
            actions = buildList {
                addAll(
                    playbackActions(
                        resumeContract = nextContract.takeIf { nextEpisode?.reason.equals("resume_in_progress", ignoreCase = true) },
                        startOverContract = if (nextEpisode?.reason.equals("resume_in_progress", ignoreCase = true)) {
                            startOverContract
                        } else {
                            nextContract ?: startOverContract
                        },
                        watched = seriesStatus?.isCompleted == true,
                        playLabel = when {
                            nextEpisode?.reason.equals("resume_in_progress", ignoreCase = true) -> "Resume next"
                            nextEpisode != null -> "Play next"
                            else -> "Play series"
                        },
                        unavailableCopy = "Playback is unavailable because this series cache does not include a playable episode file.",
                        networkActionsEnabled = networkActionsEnabled,
                        networkActionMessage = networkActionMessage,
                    ),
                )
                if (series.tmdbId != null) {
                    seriesStatus?.let { status ->
                        add(
                            watchToggleAction(
                                watched = status.isCompleted,
                                pending = status.pendingMutation,
                                networkActionsEnabled = networkActionsEnabled,
                                networkActionMessage = networkActionMessage,
                                targetId = series.tmdbId.toString(),
                            ),
                        )
                    } ?: add(retryWatchAction(networkActionsEnabled, networkActionMessage))
                }
            },
            recovery = recoveryState(result.route, libraryFreshness),
            rails = listOf(
                seasonsRail("series:${series.id}:seasons", result.route, detail.seasons, seriesStatus, imageResolutions),
                episodesRail(
                    stableKey = "series:${series.id}:episodes",
                    title = "Episodes",
                    route = result.route,
                    episodes = detail.episodes,
                    availability = detail.episodesAvailability,
                    watchState = watchState,
                    seriesStatus = seriesStatus,
                    imageResolutions = imageResolutions,
                ),
                castRail("series:${series.id}:cast", "Cast", DetailRailKind.Cast, series.cast, imageResolutions),
                guestCastRail("series:${series.id}:guest-cast", detail.episodes, imageResolutions),
                crewRail("series:${series.id}:crew", series.crew, imageResolutions),
                relatedRail("series:${series.id}:recommendations", "Recommendations", DetailRailKind.Recommendations, series.recommendations, imageResolutions),
                relatedRail("series:${series.id}:similar", "Similar", DetailRailKind.Similar, series.similar, imageResolutions),
            ),
            prefetchPolicy = prefetchPolicy,
        )
    }

    private fun episodePage(
        result: DetailLoadResult.Episode,
        watchState: WatchRepositoryState,
        libraryFreshness: LibraryFreshness?,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
        prefetchPolicy: DetailImagePrefetchPolicy,
    ): DetailPageModel {
        val episode = result.detail
        val progress = watchState.mediaProgress(episode.id)
        val seriesStatus = watchState.series.values.firstOrNull { status ->
            status.tmdbSeriesId == episode.tmdbSeriesId
        } ?: result.parentSeries?.tmdbId?.let(watchState::seriesStatus)
        val episodeStatus = seriesStatus?.episodeStatus(episode.seasonNumber, episode.episodeNumber)
        val resumeContract = DetailRouteContracts.episodeResume(episode, progress, result.route)
        val startOverContract = DetailRouteContracts.episodeStartOver(episode, result.route)
        val hero = episodeHeroFor(
            pageKey = "episode:${episode.id}",
            title = episode.title,
            parentSeries = result.parentSeries,
            episodeImages = episode.images,
            imageResolutions = imageResolutions,
        )
        return page(
            stableKey = "episode:${episode.id}",
            kind = DetailPageKind.Episode,
            route = result.route,
            title = episode.title,
            subtitle = result.parentSeries?.title?.let { "$it • S${episode.seasonNumber} E${episode.episodeNumber}" }
                ?: "S${episode.seasonNumber} E${episode.episodeNumber}",
            overview = episode.overview,
            hero = hero,
            metadata = episodeMetadata(episode, progress, episodeStatus),
            facts = episodeFacts(episode),
            watchState = episodeWatchState(episode, progress, episodeStatus, watchState.lastError),
            actions = buildList {
                addAll(
                    playbackActions(
                        resumeContract = resumeContract,
                        startOverContract = startOverContract,
                        watched = progress?.isCompleted == true || episodeStatus?.state == WatchEpisodeState.Completed,
                        playLabel = "Play episode",
                        unavailableCopy = "Playback is unavailable because this episode does not have a playable file in the cache.",
                        networkActionsEnabled = networkActionsEnabled,
                        networkActionMessage = networkActionMessage,
                    ),
                )
                addAll(
                    mediaWatchActions(
                        mediaId = episode.id,
                        progress = progress,
                        watched = progress?.isCompleted == true || episodeStatus?.state == WatchEpisodeState.Completed,
                        pending = progress?.pendingMutation == true,
                        networkActionsEnabled = networkActionsEnabled,
                        networkActionMessage = networkActionMessage,
                    ),
                )
            },
            recovery = recoveryState(result.route, libraryFreshness),
            rails = listOf(
                siblingEpisodesRail(
                    stableKey = "episode:${episode.id}:siblings",
                    currentEpisode = episode,
                    route = result.route,
                    siblings = result.siblingEpisodes,
                    watchState = watchState,
                    seriesStatus = seriesStatus,
                    imageResolutions = imageResolutions,
                ),
                castRail("episode:${episode.id}:series-cast", "Series cast", DetailRailKind.Cast, result.parentSeries?.cast.orEmpty(), imageResolutions),
                castRail("episode:${episode.id}:guest-stars", "Guest stars", DetailRailKind.GuestCast, episode.guestStars, imageResolutions),
                crewRail("episode:${episode.id}:crew", episode.crew, imageResolutions),
            ),
            prefetchPolicy = prefetchPolicy,
        )
    }

    private fun missingPage(
        result: DetailLoadResult.Missing,
        libraryFreshness: LibraryFreshness?,
        prefetchPolicy: DetailImagePrefetchPolicy,
    ): DetailPageModel {
        val noArt = noArt("No detail artwork", "Detail artwork is unavailable because the detail record is missing.")
        return page(
            stableKey = "missing:${result.route.stableKey}",
            kind = DetailPageKind.MissingDetail,
            route = result.route,
            title = result.title,
            subtitle = result.route.mediaType.displayName,
            overview = result.message,
            hero = DetailHero(background = noArt, foreground = null),
            metadata = listOf(
                DetailMetadataItem(result.route.mediaType.displayName),
                DetailMetadataItem("Recoverable", tone = DetailTone.Warning, kind = DetailMetadataKind.Recovery),
            ),
            facts = emptyList(),
            watchState = null,
            actions = emptyList(),
            recovery = recoveryState(result.route, libraryFreshness),
            rails = emptyList(),
            emptyState = DetailEmptyState(result.title, result.message),
            prefetchPolicy = prefetchPolicy,
        )
    }

    private fun page(
        stableKey: String,
        kind: DetailPageKind,
        route: MediaRouteArgs?,
        title: String,
        subtitle: String?,
        overview: String?,
        hero: DetailHero,
        metadata: List<DetailMetadataItem>,
        facts: List<DetailFactItem>,
        watchState: DetailWatchState?,
        actions: List<DetailPageAction>,
        recovery: DetailRecoveryState,
        rails: List<DetailRail>,
        emptyState: DetailEmptyState? = null,
        prefetchPolicy: DetailImagePrefetchPolicy,
    ): DetailPageModel = DetailPageModel(
        stableKey = stableKey,
        kind = kind,
        route = route,
        title = title,
        subtitle = subtitle,
        overview = overview,
        hero = hero,
        metadata = metadata,
        facts = facts,
        watchState = watchState,
        actions = actions,
        recovery = recovery,
        rails = rails,
        emptyState = emptyState,
        imagePrefetch = prefetchPlan(hero, rails, prefetchPolicy),
    )

    private fun prefetchPlan(
        hero: DetailHero,
        rails: List<DetailRail>,
        policy: DetailImagePrefetchPolicy,
    ): DetailImagePrefetchPlan {
        val ordered = LinkedHashSet<ImageRequestKey>()
        hero.imageKeys.forEach { key ->
            if (ordered.size < policy.maxImageKeys) ordered.add(key)
        }
        rails.forEach { rail ->
            rail.items.take(policy.visibleRailItemWindow).forEach { item ->
                val key = item.art.requestKey ?: return@forEach
                if (ordered.size < policy.maxImageKeys) ordered.add(key)
            }
        }
        return DetailImagePrefetchPlan(
            keys = ordered,
            visibleRailItemWindow = policy.visibleRailItemWindow,
            maxImageKeys = policy.maxImageKeys,
        )
    }

    private fun recoveryState(
        route: MediaRouteArgs,
        libraryFreshness: LibraryFreshness?,
    ): DetailRecoveryState {
        val visibility = DetailCache.recoveryActions(route)
        return DetailRecoveryState(
            freshness = libraryFreshness?.toDetailFreshnessNotice(),
            actions = buildList {
                add(DetailPageAction(DetailPageActionKind.Back, "Back", DetailActionRole.Back))
                if (visibility.retryCacheSync) {
                    add(DetailPageAction(DetailPageActionKind.RetryCache, "Retry cache sync", DetailActionRole.Retry))
                }
                if (visibility.clearSelectedCache) {
                    add(
                        DetailPageAction(
                            kind = DetailPageActionKind.ClearSelectedCache,
                            label = "Clear selected cache",
                            role = DetailActionRole.Cache,
                            targetId = route.libraryId,
                        ),
                    )
                }
                add(DetailPageAction(DetailPageActionKind.ClearAllCache, "Clear all cache", DetailActionRole.DestructiveReset))
                if (visibility.changeServer) {
                    add(DetailPageAction(DetailPageActionKind.ChangeServer, "Change server", DetailActionRole.Secondary))
                }
                if (visibility.resetConnection) {
                    add(DetailPageAction(DetailPageActionKind.ResetConnection, "Reset connection", DetailActionRole.DestructiveReset))
                }
                add(DetailPageAction(DetailPageActionKind.Diagnostics, "Diagnostics / Export diagnostics", DetailActionRole.Diagnostics))
            },
        )
    }

    private fun LibraryFreshness.toDetailFreshnessNotice(): DetailFreshnessNotice = when (this) {
        LibraryFreshness.Empty -> DetailFreshnessNotice(
            kind = DetailFreshnessKind.Empty,
            title = "Library cache is empty",
            message = "${cacheHealthSummary()} Retry cache sync or change server/reset connection to recover details.",
        )
        LibraryFreshness.Syncing -> DetailFreshnessNotice(
            kind = DetailFreshnessKind.Syncing,
            title = "Syncing library cache",
            message = "${cacheHealthSummary()} Cached details stay mounted while the detail cache refreshes.",
        )
        is LibraryFreshness.Fresh -> DetailFreshnessNotice(
            kind = DetailFreshnessKind.Fresh,
            title = "Library cache is fresh",
            message = "${cacheHealthSummary()} Available for this server and user.",
        )
        is LibraryFreshness.StaleOffline -> DetailFreshnessNotice(
            kind = DetailFreshnessKind.StaleOffline,
            title = "Stale/offline library cache",
            message = "${cacheHealthSummary()} Reason: $message",
        )
        is LibraryFreshness.SeriesCacheIncomplete -> DetailFreshnessNotice(
            kind = DetailFreshnessKind.StaleOffline,
            title = "Series cache is incomplete",
            message = "${cacheHealthSummary()} Retry continues this series cache repair. Reason: $message",
        )
        is LibraryFreshness.CorruptRebuilding -> DetailFreshnessNotice(
            kind = DetailFreshnessKind.RecoverableError,
            title = "Corrupt cache needs rebuild",
            message = "${cacheHealthSummary()} $message",
        )
        is LibraryFreshness.ErrorRetryable -> DetailFreshnessNotice(
            kind = DetailFreshnessKind.RecoverableError,
            title = "Library sync failed",
            message = "${cacheHealthSummary()} $message",
        )
    }

    private fun playbackActions(
        resumeContract: PlaybackRouteContract?,
        startOverContract: PlaybackRouteContract?,
        watched: Boolean,
        playLabel: String,
        unavailableCopy: String,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
    ): List<DetailPageAction> = buildList {
        val disabledReason = networkActionMessage ?: "Reconnect before starting playback."
        when {
            resumeContract != null -> {
                add(
                    playbackAction(
                        kind = DetailPageActionKind.Resume,
                        label = "Resume",
                        contract = resumeContract,
                        networkActionsEnabled = networkActionsEnabled,
                        disabledReason = disabledReason,
                    ),
                )
                startOverContract?.let { contract ->
                    add(
                        playbackAction(
                            kind = DetailPageActionKind.StartOver,
                            label = "Start over",
                            contract = contract,
                            networkActionsEnabled = networkActionsEnabled,
                            disabledReason = disabledReason,
                            role = DetailActionRole.Secondary,
                        ),
                    )
                }
            }
            startOverContract != null -> add(
                playbackAction(
                    kind = DetailPageActionKind.Play,
                    label = if (watched) "Play again" else playLabel,
                    contract = startOverContract,
                    networkActionsEnabled = networkActionsEnabled,
                    disabledReason = disabledReason,
                ),
            )
            else -> add(
                DetailPageAction(
                    kind = DetailPageActionKind.Play,
                    label = playLabel,
                    role = DetailActionRole.Primary,
                    enabled = false,
                    disabledReason = unavailableCopy,
                ),
            )
        }
    }

    private fun playbackAction(
        kind: DetailPageActionKind,
        label: String,
        contract: PlaybackRouteContract,
        networkActionsEnabled: Boolean,
        disabledReason: String,
        role: DetailActionRole = DetailActionRole.Primary,
    ): DetailPageAction = DetailPageAction(
        kind = kind,
        label = label,
        role = role,
        enabled = networkActionsEnabled,
        disabledReason = if (networkActionsEnabled) null else disabledReason,
        playbackContract = contract,
        targetId = contract.logicalMediaId,
    )

    private fun mediaWatchActions(
        mediaId: String,
        progress: WatchMediaProgress?,
        watched: Boolean,
        pending: Boolean,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
    ): List<DetailPageAction> = buildList {
        if (progress == null) add(retryWatchAction(networkActionsEnabled, networkActionMessage))
        if (progress?.hasServerState == true) {
            add(
                DetailPageAction(
                    kind = DetailPageActionKind.ClearProgress,
                    label = "Clear progress",
                    role = DetailActionRole.Cache,
                    enabled = networkActionsEnabled && !pending,
                    disabledReason = disabledReason(networkActionsEnabled, pending, networkActionMessage),
                    targetId = mediaId,
                ),
            )
        }
        add(
            watchToggleAction(
                watched = watched,
                pending = pending,
                networkActionsEnabled = networkActionsEnabled,
                networkActionMessage = networkActionMessage,
                targetId = mediaId,
            ),
        )
    }

    private fun watchToggleAction(
        watched: Boolean,
        pending: Boolean,
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
        targetId: String,
    ): DetailPageAction = DetailPageAction(
        kind = if (watched) DetailPageActionKind.MarkUnwatched else DetailPageActionKind.MarkWatched,
        label = when {
            pending -> "Updating…"
            watched -> "Mark unwatched"
            else -> "Mark watched"
        },
        role = if (watched) DetailActionRole.Secondary else DetailActionRole.Primary,
        enabled = networkActionsEnabled && !pending,
        disabledReason = disabledReason(networkActionsEnabled, pending, networkActionMessage),
        targetId = targetId,
        targetWatched = !watched,
    )

    private fun retryWatchAction(
        networkActionsEnabled: Boolean,
        networkActionMessage: String?,
    ): DetailPageAction = DetailPageAction(
        kind = DetailPageActionKind.RetryWatch,
        label = "Retry watch state",
        role = DetailActionRole.Retry,
        enabled = networkActionsEnabled,
        disabledReason = if (networkActionsEnabled) null else networkActionMessage ?: "Reconnect before retrying watch state.",
    )

    private fun disabledReason(
        networkActionsEnabled: Boolean,
        pending: Boolean,
        networkActionMessage: String?,
    ): String? = when {
        !networkActionsEnabled -> networkActionMessage ?: "Reconnect before updating watch state."
        pending -> "Watch-state update is already in progress."
        else -> null
    }

    private fun heroFor(
        pageKey: String,
        title: String,
        images: DetailImageSet,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailHero {
        val backgroundKey = images.backdrop ?: images.still ?: images.poster
        val backgroundFallback = images.fallbackPathFor(backgroundKey)
        val background = artFor(
            key = backgroundKey,
            fallbackPath = backgroundFallback,
            label = "$title hero art",
            surfaceKey = pageKey,
            itemKey = "hero",
            imageResolutions = imageResolutions,
            grounding = MediaArtGrounding.Flat,
        )
        val foregroundKey = images.poster?.takeIf { it != backgroundKey }
        val foreground = foregroundKey?.let { key ->
            artFor(
                key = key,
                fallbackPath = images.fallbackPathFor(key),
                label = "$title poster",
                surfaceKey = pageKey,
                itemKey = "poster",
                imageResolutions = imageResolutions,
                grounding = MediaArtGrounding.TheaterPlateContactShadow,
            )
        }
        return DetailHero(background = background, foreground = foreground)
    }

    private fun episodeHeroFor(
        pageKey: String,
        title: String,
        parentSeries: SeriesDetail?,
        episodeImages: DetailImageSet,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailHero {
        val backgroundKey = parentSeries?.images?.backdrop
        val background = if (backgroundKey != null) {
            artFor(
                key = backgroundKey,
                fallbackPath = parentSeries?.images?.backdropFallbackPath,
                label = "${parentSeries?.title ?: title} backdrop",
                surfaceKey = pageKey,
                itemKey = "hero",
                imageResolutions = imageResolutions,
                grounding = MediaArtGrounding.Flat,
            )
        } else {
            noArt(
                label = "$title backdrop",
                reason = "Episode backdrop is unavailable; Theater Plate uses a generated fallback while the episode still remains foreground.",
            )
        }
        val foregroundKey = episodeImages.still ?: parentSeries?.images?.poster
        val foreground = foregroundKey?.let { key ->
            val stillForeground = key == episodeImages.still
            artFor(
                key = key,
                fallbackPath = if (stillForeground) episodeImages.stillFallbackPath else parentSeries?.images?.posterFallbackPath,
                label = if (stillForeground) "$title still" else "${parentSeries?.title ?: title} poster",
                surfaceKey = pageKey,
                itemKey = if (stillForeground) "still" else "poster",
                imageResolutions = imageResolutions,
                grounding = MediaArtGrounding.TheaterPlateContactShadow,
            )
        }
        return DetailHero(background = background, foreground = foreground)
    }

    private fun artFor(
        key: ImageRequestKey?,
        fallbackPath: String?,
        label: String,
        surfaceKey: String,
        itemKey: String,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
        grounding: MediaArtGrounding = MediaArtGrounding.CardObject,
    ): DetailPageArt {
        if (key == null) return noArt(label, "$label is not present in the cached detail contract.")
        val mediaArt = MediaArtObject.forCategory(
            category = key.category,
            request = MediaArtRequest(key = key, publicFallbackPath = fallbackPath),
            fallbackLabel = "$label unavailable",
            targetIdentity = MediaArtTargetIdentity(
                surfaceKey = surfaceKey,
                itemKey = itemKey,
                semanticLabel = label,
            ),
            grounding = grounding,
        )
        return DetailPageArt(
            role = key.category.toDetailArtRole(),
            label = label,
            mediaArt = mediaArt,
            imageState = imageStateFor(mediaArt, imageResolutions[key]),
        )
    }

    private fun noArt(label: String, reason: String): DetailPageArt = DetailPageArt(
        role = DetailArtRole.None,
        label = label,
        mediaArt = null,
        imageState = DetailImageState.NoArt(label = "missing", reason = reason),
    )

    private fun imageStateFor(
        art: MediaArtObject,
        resolution: ImageResolution?,
    ): DetailImageState = when (resolution) {
        is ImageResolution.Ready -> DetailImageState.Ready(
            label = resolution.label,
            staleOffline = resolution.stale,
            offlineMessage = resolution.offlineMessage,
        )
        is ImageResolution.Pending -> DetailImageState.Pending(
            label = resolution.label,
            staleOffline = resolution.stale,
            retryAfterMillis = resolution.retryAfterMillis,
            message = "Image pending. Retry after ${resolution.retryAfterMillis} ms.",
        )
        is ImageResolution.Failed -> DetailImageState.Failed(
            label = resolution.label,
            staleOffline = resolution.stale,
            reason = resolution.reason,
            retryable = resolution.retryable,
        )
        is ImageResolution.Placeholder -> DetailImageState.NoArt(
            label = resolution.label,
            reason = resolution.reason,
        )
        null -> DetailImageState.Pending(
            label = "queued",
            staleOffline = false,
            message = "${art.fallbackLabel} while manifest lookup is queued.",
        )
    }

    private fun seasonsRail(
        stableKey: String,
        route: MediaRouteArgs,
        seasons: List<SeasonDetail>,
        seriesStatus: WatchSeriesStatus?,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailRail {
        if (seasons.isEmpty()) {
            return DetailRail(
                stableKey = stableKey,
                kind = DetailRailKind.Seasons,
                title = "Seasons",
                state = DetailRailState.Empty,
                cardKind = DetailRailCardKind.Poster,
                activationPolicy = DetailRailActivationPolicy.Navigate,
                items = emptyList(),
                emptyMessage = "No seasons are cached for this series bundle yet.",
            )
        }
        return DetailRail(
            stableKey = stableKey,
            kind = DetailRailKind.Seasons,
            title = "Seasons",
            state = DetailRailState.Available,
            cardKind = DetailRailCardKind.Poster,
            activationPolicy = DetailRailActivationPolicy.Navigate,
            items = seasons.map { season ->
                val status = seriesStatus?.seasons?.get(season.seasonNumber)
                DetailRailItem(
                    stableId = season.id,
                    title = season.title,
                    subtitle = seasonSubtitle(season, status),
                    badge = status?.let(::seasonBadge),
                    progress = status?.let { if (it.total > 0) it.watched.toFloat() / it.total.toFloat() else 0f },
                    art = artFor(
                        key = season.images.poster,
                        fallbackPath = season.images.posterFallbackPath,
                        label = "${season.title} poster",
                        surfaceKey = stableKey,
                        itemKey = season.id,
                        imageResolutions = imageResolutions,
                    ),
                    route = MediaRouteArgs(
                        mediaType = BrowseMediaType.Season,
                        mediaId = season.id,
                        libraryId = route.libraryId,
                        sourceSurface = route.sourceSurface,
                    ),
                )
            },
        )
    }

    private fun episodesRail(
        stableKey: String,
        title: String,
        route: MediaRouteArgs,
        episodes: List<EpisodeDetail>,
        availability: EpisodesAvailability,
        watchState: WatchRepositoryState,
        seriesStatus: WatchSeriesStatus?,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailRail {
        if (availability is EpisodesAvailability.Unavailable) {
            return DetailRail(
                stableKey = stableKey,
                kind = DetailRailKind.Episodes,
                title = title,
                state = DetailRailState.Unavailable,
                cardKind = DetailRailCardKind.Still,
                activationPolicy = DetailRailActivationPolicy.Play,
                items = emptyList(),
                unavailableMessage = availability.message,
            )
        }
        if (episodes.isEmpty()) {
            return DetailRail(
                stableKey = stableKey,
                kind = DetailRailKind.Episodes,
                title = title,
                state = DetailRailState.Empty,
                cardKind = DetailRailCardKind.Still,
                activationPolicy = DetailRailActivationPolicy.Play,
                items = emptyList(),
                emptyMessage = "No episodes are cached for this detail surface.",
            )
        }
        return DetailRail(
            stableKey = stableKey,
            kind = DetailRailKind.Episodes,
            title = title,
            state = DetailRailState.Available,
            cardKind = DetailRailCardKind.Still,
            activationPolicy = DetailRailActivationPolicy.Play,
            items = episodes.map { episode ->
                episodeRailItem(stableKey, route, episode, watchState, seriesStatus, imageResolutions)
            },
        )
    }

    private fun siblingEpisodesRail(
        stableKey: String,
        currentEpisode: EpisodeDetail,
        route: MediaRouteArgs,
        siblings: List<EpisodeDetail>,
        watchState: WatchRepositoryState,
        seriesStatus: WatchSeriesStatus?,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailRail {
        val seasonSiblings = siblings.filter { it.seasonNumber == currentEpisode.seasonNumber }
        if (seasonSiblings.isEmpty()) {
            return DetailRail(
                stableKey = stableKey,
                kind = DetailRailKind.SiblingEpisodes,
                title = "More from season ${currentEpisode.seasonNumber}",
                state = DetailRailState.Unavailable,
                cardKind = DetailRailCardKind.Still,
                activationPolicy = DetailRailActivationPolicy.Play,
                items = emptyList(),
                unavailableMessage = "Sibling episode data is unavailable in the cached series bundle.",
            )
        }
        return DetailRail(
            stableKey = stableKey,
            kind = DetailRailKind.SiblingEpisodes,
            title = "More from season ${currentEpisode.seasonNumber}",
            state = DetailRailState.Available,
            cardKind = DetailRailCardKind.Still,
            activationPolicy = DetailRailActivationPolicy.Play,
            items = seasonSiblings.map { episode ->
                episodeRailItem(stableKey, route, episode, watchState, seriesStatus, imageResolutions)
            },
        )
    }

    private fun episodeRailItem(
        stableKey: String,
        sourceRoute: MediaRouteArgs,
        episode: EpisodeDetail,
        watchState: WatchRepositoryState,
        seriesStatus: WatchSeriesStatus?,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailRailItem {
        val progress = watchState.mediaProgress(episode.id)
        val episodeStatus = seriesStatus?.episodeStatus(episode.seasonNumber, episode.episodeNumber)
        val ratio = progress?.progressRatio?.takeIf { it > 0f } ?: episodeStatus?.progress
        val watched = progress?.isCompleted == true || episodeStatus?.state == WatchEpisodeState.Completed
        val route = MediaRouteArgs(
            mediaType = BrowseMediaType.Episode,
            mediaId = episode.id,
            libraryId = episode.libraryId,
            sourceSurface = sourceRoute.sourceSurface,
        )
        return DetailRailItem(
            stableId = episode.id,
            title = "S${episode.seasonNumber} E${episode.episodeNumber}: ${episode.title}",
            subtitle = episode.overview ?: episode.airDate,
            badge = when {
                watched -> "Watched"
                ratio != null && ratio > 0f -> "${(ratio * 100f).toInt()}% watched"
                else -> null
            },
            progress = ratio,
            art = artFor(
                key = episode.images.still ?: episode.images.poster,
                fallbackPath = episode.images.stillFallbackPath ?: episode.images.posterFallbackPath,
                label = "${episode.title} still",
                surfaceKey = stableKey,
                itemKey = episode.id,
                imageResolutions = imageResolutions,
            ),
            route = route,
            playbackContract = DetailRouteContracts.episodeResume(episode, progress, sourceRoute)
                ?: DetailRouteContracts.episodeStartOver(episode, sourceRoute),
        )
    }

    private fun castRail(
        stableKey: String,
        title: String,
        kind: DetailRailKind,
        cast: List<DetailCastCredit>,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailRail {
        if (cast.isEmpty()) {
            return DetailRail(
                stableKey = stableKey,
                kind = kind,
                title = title,
                state = DetailRailState.Unavailable,
                cardKind = DetailRailCardKind.Profile,
                activationPolicy = DetailRailActivationPolicy.Disabled,
                items = emptyList(),
                unavailableMessage = "$title/profile data is unavailable in this cached detail contract.",
            )
        }
        return DetailRail(
            stableKey = stableKey,
            kind = kind,
            title = title,
            state = DetailRailState.Available,
            cardKind = DetailRailCardKind.Profile,
            activationPolicy = DetailRailActivationPolicy.Disabled,
            items = cast.map { credit ->
                DetailRailItem(
                    stableId = credit.personId ?: credit.personTmdbId?.toString() ?: credit.creditId ?: credit.name,
                    title = credit.name,
                    subtitle = credit.character,
                    badge = credit.knownForDepartment,
                    progress = null,
                    art = artFor(
                        key = credit.profileImages.profile,
                        fallbackPath = credit.profileImages.profileFallbackPath,
                        label = "${credit.name} profile",
                        surfaceKey = stableKey,
                        itemKey = credit.personId ?: credit.personTmdbId?.toString() ?: credit.name,
                        imageResolutions = imageResolutions,
                    ),
                )
            },
        )
    }

    private fun guestCastRail(
        stableKey: String,
        episodes: List<EpisodeDetail>,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailRail {
        val guestStars = episodes.flatMap { episode ->
            episode.guestStars.map { credit -> episode to credit }
        }
        if (guestStars.isEmpty()) {
            return DetailRail(
                stableKey = stableKey,
                kind = DetailRailKind.GuestCast,
                title = "Episode guests",
                state = DetailRailState.Unavailable,
                cardKind = DetailRailCardKind.Profile,
                activationPolicy = DetailRailActivationPolicy.Disabled,
                items = emptyList(),
                unavailableMessage = "Episode guest/profile data is unavailable in this cached detail contract.",
            )
        }
        return DetailRail(
            stableKey = stableKey,
            kind = DetailRailKind.GuestCast,
            title = "Episode guests",
            state = DetailRailState.Available,
            cardKind = DetailRailCardKind.Profile,
            activationPolicy = DetailRailActivationPolicy.Disabled,
            items = guestStars.map { (episode, credit) ->
                DetailRailItem(
                    stableId = listOfNotNull(episode.id, credit.personId ?: credit.personTmdbId?.toString() ?: credit.name).joinToString(":"),
                    title = credit.name,
                    subtitle = listOfNotNull(credit.character, "S${episode.seasonNumber} E${episode.episodeNumber}").joinToString(" • "),
                    badge = credit.knownForDepartment,
                    progress = null,
                    art = artFor(
                        key = credit.profileImages.profile,
                        fallbackPath = credit.profileImages.profileFallbackPath,
                        label = "${credit.name} profile",
                        surfaceKey = stableKey,
                        itemKey = "${episode.id}:${credit.personId ?: credit.personTmdbId?.toString() ?: credit.name}",
                        imageResolutions = imageResolutions,
                    ),
                )
            },
        )
    }

    private fun crewRail(
        stableKey: String,
        crew: List<DetailCrewCredit>,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailRail {
        if (crew.isEmpty()) {
            return DetailRail(
                stableKey = stableKey,
                kind = DetailRailKind.Crew,
                title = "Crew",
                state = DetailRailState.Unavailable,
                cardKind = DetailRailCardKind.Profile,
                activationPolicy = DetailRailActivationPolicy.Disabled,
                items = emptyList(),
                unavailableMessage = "Crew/profile data is unavailable in this cached detail contract.",
            )
        }
        return DetailRail(
            stableKey = stableKey,
            kind = DetailRailKind.Crew,
            title = "Crew",
            state = DetailRailState.Available,
            cardKind = DetailRailCardKind.Profile,
            activationPolicy = DetailRailActivationPolicy.Disabled,
            items = crew.map { credit ->
                DetailRailItem(
                    stableId = credit.personId ?: credit.personTmdbId?.toString() ?: credit.creditId ?: "${credit.department}:${credit.name}",
                    title = credit.name,
                    subtitle = credit.job,
                    badge = credit.department,
                    progress = null,
                    art = artFor(
                        key = credit.profileImages.profile,
                        fallbackPath = credit.profileImages.profileFallbackPath,
                        label = "${credit.name} profile",
                        surfaceKey = stableKey,
                        itemKey = credit.personId ?: credit.personTmdbId?.toString() ?: credit.name,
                        imageResolutions = imageResolutions,
                    ),
                )
            },
        )
    }

    private fun relatedRail(
        stableKey: String,
        title: String,
        kind: DetailRailKind,
        refs: List<DetailRelatedMediaRef>,
        @Suppress("UNUSED_PARAMETER") imageResolutions: Map<ImageRequestKey, ImageResolution>,
    ): DetailRail {
        if (refs.isEmpty()) {
            return DetailRail(
                stableKey = stableKey,
                kind = kind,
                title = title,
                state = DetailRailState.Unavailable,
                cardKind = DetailRailCardKind.Text,
                activationPolicy = DetailRailActivationPolicy.Disabled,
                items = emptyList(),
                unavailableMessage = "$title data is unavailable in this cached detail contract.",
            )
        }
        return DetailRail(
            stableKey = stableKey,
            kind = kind,
            title = title,
            state = DetailRailState.Available,
            cardKind = DetailRailCardKind.Text,
            activationPolicy = DetailRailActivationPolicy.Disabled,
            items = refs.map { ref ->
                val label = ref.title ?: ref.tmdbId?.let { "TMDB $it" } ?: "Untitled related title"
                DetailRailItem(
                    stableId = ref.tmdbId?.toString() ?: label,
                    title = label,
                    subtitle = ref.tmdbId?.let { "TMDB $it" },
                    badge = null,
                    progress = null,
                    art = noArt("$label artwork", "Related-media refs do not include visible artwork yet."),
                )
            },
        )
    }

    private fun movieMetadata(movie: MovieDetail, progress: WatchMediaProgress?): List<DetailMetadataItem> = buildList {
        movie.releaseDate?.take(4)?.let { add(DetailMetadataItem(it)) }
        movie.runtimeMinutes?.let { add(DetailMetadataItem("$it min")) }
        movie.contentRating?.let { add(DetailMetadataItem(it)) }
        movie.voteAverage?.let { add(DetailMetadataItem("★ ${"%.1f".format(it)}", DetailTone.Warning, DetailMetadataKind.AudienceRating)) }
        progress?.let { add(mediaWatchMetadata(it)) }
    }

    private fun movieFacts(movie: MovieDetail): List<DetailFactItem> = buildList {
        movie.status?.let { add(DetailFactItem("Status", it)) }
        movie.releaseDate?.let { add(DetailFactItem("Release date", it)) }
        movie.fileName?.let { add(DetailFactItem("Target file", it, DetailTone.Muted)) }
        movie.tmdbId?.let { add(DetailFactItem("TMDB", it.toString(), DetailTone.Muted)) }
    }

    private fun seriesMetadata(series: SeriesDetail, status: WatchSeriesStatus?): List<DetailMetadataItem> = buildList {
        series.firstAirDate?.take(4)?.let { add(DetailMetadataItem(it)) }
        (series.availableSeasons ?: series.numberOfSeasons)?.let { add(DetailMetadataItem("$it season(s)")) }
        (series.availableEpisodes ?: series.numberOfEpisodes)?.let { add(DetailMetadataItem("$it episode(s)")) }
        series.contentRating?.let { add(DetailMetadataItem(it)) }
        series.voteAverage?.let { add(DetailMetadataItem("★ ${"%.1f".format(it)}", DetailTone.Warning, DetailMetadataKind.AudienceRating)) }
        status?.let { add(seriesWatchMetadata(it)) }
    }

    private fun seriesFacts(series: SeriesDetail): List<DetailFactItem> = buildList {
        series.status?.let { add(DetailFactItem("Status", it)) }
        series.firstAirDate?.let { add(DetailFactItem("First air date", it)) }
        series.lastAirDate?.let { add(DetailFactItem("Last air date", it)) }
        add(DetailFactItem("Production", if (series.inProduction) "In production" else "Ended", DetailTone.Muted))
        series.tmdbId?.let { add(DetailFactItem("TMDB", it.toString(), DetailTone.Muted)) }
    }

    private fun seasonMetadata(season: SeasonDetail, status: WatchSeasonStatus?): List<DetailMetadataItem> = buildList {
        season.airDate?.take(4)?.let { add(DetailMetadataItem(it)) }
        season.episodeCount?.let { add(DetailMetadataItem("$it episode(s)")) }
        season.runtimeMinutes?.let { add(DetailMetadataItem("$it min")) }
        status?.let { add(DetailMetadataItem(seasonBadge(it), if (it.isCompleted) DetailTone.Success else DetailTone.Accent, DetailMetadataKind.WatchState)) }
    }

    private fun seasonFacts(season: SeasonDetail): List<DetailFactItem> = buildList {
        add(DetailFactItem("Season", season.seasonNumber.toString()))
        season.airDate?.let { add(DetailFactItem("Air date", it)) }
        season.episodeCount?.let { add(DetailFactItem("Episodes", it.toString())) }
    }

    private fun episodeMetadata(
        episode: EpisodeDetail,
        progress: WatchMediaProgress?,
        status: WatchEpisodeStatus?,
    ): List<DetailMetadataItem> = buildList {
        episode.airDate?.take(4)?.let { add(DetailMetadataItem(it)) }
        episode.runtimeMinutes?.let { add(DetailMetadataItem("$it min")) }
        add(DetailMetadataItem("Season ${episode.seasonNumber}"))
        add(DetailMetadataItem("Episode ${episode.episodeNumber}"))
        progress?.let { add(mediaWatchMetadata(it)) } ?: status?.let { add(episodeWatchMetadata(it)) }
    }

    private fun episodeFacts(episode: EpisodeDetail): List<DetailFactItem> = buildList {
        add(DetailFactItem("Episode key", episode.episodeKey))
        episode.airDate?.let { add(DetailFactItem("Air date", it)) }
        episode.fileName?.let { add(DetailFactItem("Target file", it, DetailTone.Muted)) }
        episode.tmdbSeriesId?.let { add(DetailFactItem("TMDB series", it.toString(), DetailTone.Muted)) }
    }

    private fun mediaWatchState(
        scopeKey: String,
        label: String,
        progress: WatchMediaProgress?,
        lastError: String? = null,
    ): DetailWatchState = when {
        progress == null -> DetailWatchState(
            scopeKey = scopeKey,
            label = label,
            state = DetailWatchStateKind.Unknown,
            progress = 0f,
            pendingMutation = false,
            message = watchRefreshMessage(
                lastError = lastError,
                fallback = "Watch state has not loaded yet. Retry watch state keeps this detail recoverable.",
            ),
        )
        progress.isCompleted -> DetailWatchState(
            scopeKey = scopeKey,
            label = label,
            state = DetailWatchStateKind.Watched,
            progress = 1f,
            pendingMutation = progress.pendingMutation,
            message = "Completed. Start over or mark unwatched if this state is wrong.",
        )
        progress.progressRatio > 0f -> DetailWatchState(
            scopeKey = scopeKey,
            label = label,
            state = DetailWatchStateKind.InProgress,
            progress = progress.progressRatio,
            pendingMutation = progress.pendingMutation,
            message = "Resume from ${formatSeconds(progress.positionSeconds)} (${(progress.progressRatio * 100f).toInt()}% watched) or start over.",
        )
        else -> DetailWatchState(
            scopeKey = scopeKey,
            label = label,
            state = DetailWatchStateKind.Unwatched,
            progress = 0f,
            pendingMutation = progress.pendingMutation,
            message = "Unwatched. Play starts from the beginning.",
        )
    }

    private fun seriesWatchState(
        series: SeriesDetail,
        status: WatchSeriesStatus?,
        lastError: String? = null,
    ): DetailWatchState = when {
        series.tmdbId == null -> DetailWatchState(
            scopeKey = series.id,
            label = "Series watch state",
            state = DetailWatchStateKind.Unavailable,
            progress = 0f,
            pendingMutation = false,
            message = "Series watch state requires a TMDB series id.",
        )
        status == null -> DetailWatchState(
            scopeKey = series.tmdbId.toString(),
            label = "Series watch state",
            state = DetailWatchStateKind.Unknown,
            progress = 0f,
            pendingMutation = false,
            message = watchRefreshMessage(
                lastError = lastError,
                fallback = "Retry to load series watch state and next episode.",
            ),
        )
        status.isCompleted -> DetailWatchState(
            scopeKey = series.tmdbId.toString(),
            label = "Series watch state",
            state = DetailWatchStateKind.Watched,
            progress = 1f,
            pendingMutation = status.pendingMutation,
            message = "${status.watched} of ${status.totalEpisodes} watched.",
        )
        status.progressRatio > 0f -> DetailWatchState(
            scopeKey = series.tmdbId.toString(),
            label = "Series watch state",
            state = DetailWatchStateKind.InProgress,
            progress = status.progressRatio,
            pendingMutation = status.pendingMutation,
            message = "${status.watched} of ${status.totalEpisodes} watched; ${status.inProgress} in progress.",
        )
        else -> DetailWatchState(
            scopeKey = series.tmdbId.toString(),
            label = "Series watch state",
            state = DetailWatchStateKind.Unwatched,
            progress = 0f,
            pendingMutation = status.pendingMutation,
            message = "No episodes watched yet.",
        )
    }

    private fun seasonWatchState(
        season: SeasonDetail,
        status: WatchSeasonStatus?,
        tmdbSeriesId: Long?,
        lastError: String? = null,
    ): DetailWatchState? = when {
        status != null -> DetailWatchState(
            scopeKey = "season:${season.id}",
            label = "Season watch state",
            state = when {
                status.isCompleted -> DetailWatchStateKind.Watched
                status.watched > 0 || status.inProgress > 0 -> DetailWatchStateKind.InProgress
                else -> DetailWatchStateKind.Unwatched
            },
            progress = if (status.total > 0) (status.watched.toFloat() / status.total.toFloat()).coerceIn(0f, 1f) else 0f,
            pendingMutation = false,
            message = "${status.watched} of ${status.total} watched; ${status.inProgress} in progress.",
        )
        tmdbSeriesId != null || !lastError.isNullOrBlank() -> DetailWatchState(
            scopeKey = "season:${season.id}",
            label = "Season watch state",
            state = DetailWatchStateKind.Unknown,
            progress = 0f,
            pendingMutation = false,
            message = watchRefreshMessage(
                lastError = lastError,
                fallback = "Retry to load season watch state.",
            ),
        )
        else -> null
    }

    private fun episodeWatchState(
        episode: EpisodeDetail,
        progress: WatchMediaProgress?,
        status: WatchEpisodeStatus?,
        lastError: String? = null,
    ): DetailWatchState {
        val mediaState = mediaWatchState(episode.id, "Episode watch state", progress, lastError)
        if (progress != null || status == null) return mediaState
        return DetailWatchState(
            scopeKey = episode.id,
            label = "Episode watch state",
            state = when (status.state) {
                WatchEpisodeState.Completed -> DetailWatchStateKind.Watched
                WatchEpisodeState.InProgress -> DetailWatchStateKind.InProgress
                WatchEpisodeState.Unwatched -> DetailWatchStateKind.Unwatched
            },
            progress = status.progress,
            pendingMutation = false,
            message = when (status.state) {
                WatchEpisodeState.Completed -> "Completed according to series watch state."
                WatchEpisodeState.InProgress -> "In progress according to series watch state."
                WatchEpisodeState.Unwatched -> "Unwatched according to series watch state."
            },
        )
    }

    private fun watchRefreshMessage(lastError: String?, fallback: String): String = lastError
        ?.takeIf { it.isNotBlank() }
        ?.let { "Watch state refresh failed: $it. Retry watch state keeps playback actions visible." }
        ?: fallback

    private fun mediaWatchMetadata(progress: WatchMediaProgress): DetailMetadataItem = when {
        progress.isCompleted -> DetailMetadataItem("Watched", DetailTone.Success, DetailMetadataKind.WatchState)
        progress.progressRatio > 0f -> DetailMetadataItem("${(progress.progressRatio * 100f).toInt()}% watched", DetailTone.Accent, DetailMetadataKind.WatchState)
        else -> DetailMetadataItem("Unwatched", DetailTone.Muted, DetailMetadataKind.WatchState)
    }

    private fun seriesWatchMetadata(status: WatchSeriesStatus): DetailMetadataItem = when {
        status.isCompleted -> DetailMetadataItem("Watched", DetailTone.Success, DetailMetadataKind.WatchState)
        status.progressRatio > 0f -> DetailMetadataItem("${(status.progressRatio * 100f).toInt()}% watched", DetailTone.Accent, DetailMetadataKind.WatchState)
        else -> DetailMetadataItem("Unwatched", DetailTone.Muted, DetailMetadataKind.WatchState)
    }

    private fun episodeWatchMetadata(status: WatchEpisodeStatus): DetailMetadataItem = when (status.state) {
        WatchEpisodeState.Completed -> DetailMetadataItem("Watched", DetailTone.Success, DetailMetadataKind.WatchState)
        WatchEpisodeState.InProgress -> DetailMetadataItem("${(status.progress * 100f).toInt()}% watched", DetailTone.Accent, DetailMetadataKind.WatchState)
        WatchEpisodeState.Unwatched -> DetailMetadataItem("Unwatched", DetailTone.Muted, DetailMetadataKind.WatchState)
    }

    private fun seasonSubtitle(season: SeasonDetail, status: WatchSeasonStatus?): String = buildList {
        (season.episodeCount ?: status?.total)?.let { add("$it episode(s)") }
        season.airDate?.take(4)?.let { add(it) }
        season.runtimeMinutes?.let { add("$it min") }
    }.joinToString(" • ").ifBlank { "Season ${season.seasonNumber}" }

    private fun seasonBadge(status: WatchSeasonStatus): String = when {
        status.isCompleted -> "Watched"
        status.inProgress > 0 -> "${status.inProgress} in progress"
        status.watched > 0 -> "${status.watched}/${status.total} watched"
        else -> "Unwatched"
    }

    private fun DetailImageSet.fallbackPathFor(key: ImageRequestKey?): String? = when (key) {
        backdrop -> backdropFallbackPath
        poster -> posterFallbackPath
        still -> stillFallbackPath
        else -> null
    }

    private fun BrowseImageCategory.toDetailArtRole(): DetailArtRole = when (this) {
        BrowseImageCategory.Poster -> DetailArtRole.Poster
        BrowseImageCategory.Backdrop -> DetailArtRole.Backdrop
        BrowseImageCategory.Profile -> DetailArtRole.Profile
        BrowseImageCategory.Episode -> DetailArtRole.Still
    }

    private fun formatSeconds(seconds: Double): String {
        val total = seconds.toInt().coerceAtLeast(0)
        val minutes = total / 60
        val remaining = total % 60
        return "$minutes:${remaining.toString().padStart(2, '0')}"
    }
}
