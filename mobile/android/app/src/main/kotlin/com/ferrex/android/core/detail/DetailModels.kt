package com.ferrex.android.core.detail

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.library.CachedMovieLibrary
import com.ferrex.android.core.library.CachedSeriesLibrary
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.SeriesBundleAccessor
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.watch.WatchMediaProgress
import com.ferrex.android.core.watch.WatchNextEpisode
import com.ferrex.android.core.watch.WatchRepositoryState
import ferrex.details.EnhancedMovieDetails
import ferrex.details.EnhancedSeriesDetails
import ferrex.details.EpisodeDetails
import ferrex.details.SeasonDetails
import ferrex.files.MediaFile
import ferrex.media.EpisodeReference
import ferrex.media.MovieReference
import ferrex.media.SeasonReference
import ferrex.media.SeriesReference

sealed interface DetailLoadResult {
    val route: MediaRouteArgs

    data class Movie(
        override val route: MediaRouteArgs,
        val detail: MovieDetail,
    ) : DetailLoadResult

    data class Series(
        override val route: MediaRouteArgs,
        val detail: SeriesBundleDetail,
    ) : DetailLoadResult

    data class Episode(
        override val route: MediaRouteArgs,
        val detail: EpisodeDetail,
        val parentSeries: SeriesDetail?,
    ) : DetailLoadResult

    data class Missing(
        override val route: MediaRouteArgs,
        val title: String,
        val message: String,
        val selectedLibraryId: String?,
    ) : DetailLoadResult
}

data class DetailRecoveryActionVisibility(
    val back: Boolean = true,
    val retryCacheSync: Boolean = true,
    val clearSelectedCache: Boolean,
    val changeServer: Boolean = true,
    val resetConnection: Boolean = true,
)

data class DetailImageSet(
    val poster: ImageRequestKey? = null,
    val backdrop: ImageRequestKey? = null,
    val still: ImageRequestKey? = null,
    val posterFallbackPath: String? = null,
    val backdropFallbackPath: String? = null,
    val stillFallbackPath: String? = null,
) {
    val keys: Set<ImageRequestKey> = setOfNotNull(poster, backdrop, still)
}

data class MovieDetail(
    val id: String,
    val libraryId: String,
    val title: String,
    val overview: String?,
    val releaseDate: String?,
    val runtimeMinutes: Int?,
    val voteAverage: Float?,
    val voteCount: Int?,
    val contentRating: String?,
    val genres: List<String>,
    val tagline: String?,
    val status: String?,
    val tmdbId: Long?,
    val fileId: String?,
    val fileName: String?,
    val images: DetailImageSet,
)

data class SeriesDetail(
    val id: String,
    val libraryId: String,
    val title: String,
    val overview: String?,
    val firstAirDate: String?,
    val lastAirDate: String?,
    val availableSeasons: Int?,
    val availableEpisodes: Int?,
    val numberOfSeasons: Int?,
    val numberOfEpisodes: Int?,
    val voteAverage: Float?,
    val voteCount: Int?,
    val contentRating: String?,
    val genres: List<String>,
    val tagline: String?,
    val status: String?,
    val inProduction: Boolean,
    val tmdbId: Long?,
    val images: DetailImageSet,
)

data class SeasonDetail(
    val id: String,
    val seasonNumber: Int,
    val title: String,
    val overview: String?,
    val airDate: String?,
    val episodeCount: Int?,
    val runtimeMinutes: Int?,
    val images: DetailImageSet,
)

data class EpisodeDetail(
    val id: String,
    val libraryId: String,
    val seriesId: String,
    val seasonNumber: Int,
    val episodeNumber: Int,
    val title: String,
    val overview: String?,
    val airDate: String?,
    val runtimeMinutes: Int?,
    val tmdbSeriesId: Long?,
    val fileId: String?,
    val fileName: String?,
    val images: DetailImageSet,
) {
    val episodeKey: String = "S$seasonNumber:E$episodeNumber"
}

data class SeriesBundleDetail(
    val series: SeriesDetail,
    val seasons: List<SeasonDetail>,
    val episodesBySeason: Map<Int, List<EpisodeDetail>>,
    val episodesAvailability: EpisodesAvailability,
) {
    val episodes: List<EpisodeDetail> = episodesBySeason.values.flatten()
    val firstPlayableEpisode: EpisodeDetail? = episodes.firstOrNull { it.playbackTargetId != null }
}

sealed interface EpisodesAvailability {
    data class Available(val episodeCount: Int) : EpisodesAvailability
    data class Unavailable(val message: String, val retryLabel: String = "Retry episodes") : EpisodesAvailability
}

val MovieDetail.playbackTargetId: String?
    get() = fileId ?: id

val EpisodeDetail.playbackTargetId: String?
    get() = fileId ?: id

object DetailRouteContracts {
    fun continuePlayback(result: DetailLoadResult, watchState: WatchRepositoryState): PlaybackRouteContract? = when (result) {
        is DetailLoadResult.Movie -> movieResume(result.detail, watchState.mediaProgress(result.detail.id), result.route)
            ?: movieServerResume(result.detail, result.route)
        is DetailLoadResult.Episode -> episodeResume(result.detail, watchState.mediaProgress(result.detail.id), result.route)
            ?: episodeServerResume(result.detail, result.route)
        is DetailLoadResult.Series -> seriesNext(
            series = result.detail,
            nextEpisode = result.detail.series.tmdbId?.let { watchState.seriesStatus(it)?.nextEpisode ?: watchState.nextEpisodes[it] },
            sourceRoute = result.route,
        ) ?: seriesStartOver(result.detail, result.route)
        is DetailLoadResult.Missing -> null
    }

    fun movieResume(movie: MovieDetail, progress: WatchMediaProgress?, sourceRoute: MediaRouteArgs): PlaybackRouteContract? {
        if (progress == null || progress.isCompleted || progress.positionSeconds <= 0.0) return null
        return movie.playbackTargetId?.let { target ->
            PlaybackRouteContract(
                targetMediaId = target,
                logicalMediaId = movie.id,
                mediaType = BrowseMediaType.Movie,
                startPositionSeconds = progress.positionSeconds,
                startOver = false,
                sourceDetailRoute = sourceRoute.toRouteString(),
            )
        }
    }

    fun movieStartOver(movie: MovieDetail, sourceRoute: MediaRouteArgs): PlaybackRouteContract? = movie.playbackTargetId?.let { target ->
        PlaybackRouteContract(
            targetMediaId = target,
            logicalMediaId = movie.id,
            mediaType = BrowseMediaType.Movie,
            startPositionSeconds = null,
            startOver = true,
            sourceDetailRoute = sourceRoute.toRouteString(),
        )
    }

    private fun movieServerResume(movie: MovieDetail, sourceRoute: MediaRouteArgs): PlaybackRouteContract? = movie.playbackTargetId?.let { target ->
        PlaybackRouteContract(
            targetMediaId = target,
            logicalMediaId = movie.id,
            mediaType = BrowseMediaType.Movie,
            startPositionSeconds = null,
            startOver = false,
            sourceDetailRoute = sourceRoute.toRouteString(),
        )
    }

    fun episodeResume(episode: EpisodeDetail, progress: WatchMediaProgress?, sourceRoute: MediaRouteArgs): PlaybackRouteContract? {
        if (progress == null || progress.isCompleted || progress.positionSeconds <= 0.0) return null
        return episode.playbackTargetId?.let { target ->
            PlaybackRouteContract(
                targetMediaId = target,
                logicalMediaId = episode.id,
                mediaType = BrowseMediaType.Episode,
                startPositionSeconds = progress.positionSeconds,
                startOver = false,
                sourceDetailRoute = sourceRoute.toRouteString(),
            )
        }
    }

    fun episodeStartOver(episode: EpisodeDetail, sourceRoute: MediaRouteArgs): PlaybackRouteContract? = episode.playbackTargetId?.let { target ->
        PlaybackRouteContract(
            targetMediaId = target,
            logicalMediaId = episode.id,
            mediaType = BrowseMediaType.Episode,
            startPositionSeconds = null,
            startOver = true,
            sourceDetailRoute = sourceRoute.toRouteString(),
        )
    }

    private fun episodeServerResume(episode: EpisodeDetail, sourceRoute: MediaRouteArgs): PlaybackRouteContract? = episode.playbackTargetId?.let { target ->
        PlaybackRouteContract(
            targetMediaId = target,
            logicalMediaId = episode.id,
            mediaType = BrowseMediaType.Episode,
            startPositionSeconds = null,
            startOver = false,
            sourceDetailRoute = sourceRoute.toRouteString(),
        )
    }

    fun seriesNext(series: SeriesBundleDetail, nextEpisode: WatchNextEpisode?, sourceRoute: MediaRouteArgs): PlaybackRouteContract? {
        val episode = nextEpisode?.playableMediaId?.let { playable ->
            series.episodes.firstOrNull { it.id == playable || it.fileId == playable }
        } ?: nextEpisode?.key?.let { key ->
            series.episodes.firstOrNull {
                it.seasonNumber == key.seasonNumber && it.episodeNumber == key.episodeNumber
            }
        } ?: series.firstPlayableEpisode
        val shouldResumeServerProgress = nextEpisode?.reason.equals("resume_in_progress", ignoreCase = true)
        return if (shouldResumeServerProgress) {
            episode?.playbackTargetId?.let { target ->
                PlaybackRouteContract(
                    targetMediaId = target,
                    logicalMediaId = episode.id,
                    mediaType = BrowseMediaType.Episode,
                    startPositionSeconds = null,
                    startOver = false,
                    sourceDetailRoute = sourceRoute.toRouteString(),
                )
            }
        } else {
            episode?.let { episodeStartOver(it, sourceRoute) }
        }
    }

    fun seriesStartOver(series: SeriesBundleDetail, sourceRoute: MediaRouteArgs): PlaybackRouteContract? =
        series.firstPlayableEpisode?.let { episodeStartOver(it, sourceRoute) }
}

object DetailCache {
    fun resolve(state: LibraryRepositoryState?, route: MediaRouteArgs): DetailLoadResult {
        if (state == null) return missing(route, "Library cache has not loaded yet.")
        return when (route.mediaType) {
            BrowseMediaType.Movie -> resolveMovie(state, route)
            BrowseMediaType.Series -> resolveSeries(state, route)
            BrowseMediaType.Episode -> resolveEpisode(state, route)
            BrowseMediaType.Unknown -> missing(route, "This media type is not supported by the detail cache.")
        }
    }

    fun recoveryActions(route: MediaRouteArgs): DetailRecoveryActionVisibility = DetailRecoveryActionVisibility(
        clearSelectedCache = route.libraryId != null,
    )

    fun imageKeys(result: DetailLoadResult?): Set<ImageRequestKey> = when (result) {
        is DetailLoadResult.Movie -> result.detail.images.keys
        is DetailLoadResult.Series -> buildSet {
            addAll(result.detail.series.images.keys)
            result.detail.seasons.flatMapTo(this) { it.images.keys }
            result.detail.episodes.flatMapTo(this) { it.images.keys }
        }
        is DetailLoadResult.Episode -> buildSet {
            addAll(result.detail.images.keys)
            result.parentSeries?.images?.keys?.let(::addAll)
        }
        is DetailLoadResult.Missing,
        null -> emptySet()
    }

    private fun resolveMovie(state: LibraryRepositoryState, route: MediaRouteArgs): DetailLoadResult {
        matchingMovieLibraries(state, route.libraryId).forEach { cached ->
            val movie = cached.accessor.movieById(route.mediaId) ?: return@forEach
            return DetailLoadResult.Movie(route, movie.toDetail(cached.library.name))
        }
        return missing(route, "Movie not found in the selected repository cache. Retry cache sync or recover the connection.")
    }

    private fun resolveSeries(state: LibraryRepositoryState, route: MediaRouteArgs): DetailLoadResult {
        matchingSeriesLibraries(state, route.libraryId).forEach { cached ->
            val series = cached.accessor.seriesById(route.mediaId) ?: return@forEach
            val bundle = cached.accessor.bundleForSeries(route.mediaId)
            return DetailLoadResult.Series(route, bundle.toDetail(series))
        }
        return missing(route, "Series not found in the selected repository cache. Retry cache sync or recover the connection.")
    }

    private fun resolveEpisode(state: LibraryRepositoryState, route: MediaRouteArgs): DetailLoadResult {
        matchingSeriesLibraries(state, route.libraryId).forEach { cached ->
            val episode = cached.accessor.episodeById(route.mediaId) ?: return@forEach
            val parentSeriesId = episode.seriesId.toUuidString()
            val parent = cached.accessor.seriesById(parentSeriesId)?.toDetail()
            return DetailLoadResult.Episode(route, episode.toDetail(), parent)
        }
        return missing(route, "Episode not found in cached series bundles. Retry episodes or recover the connection.")
    }

    private fun missing(route: MediaRouteArgs, message: String): DetailLoadResult.Missing = DetailLoadResult.Missing(
        route = route,
        title = "Details unavailable",
        message = message,
        selectedLibraryId = route.libraryId,
    )

    private fun matchingMovieLibraries(state: LibraryRepositoryState, libraryId: String?): List<CachedMovieLibrary> =
        state.movieLibraries.filter { libraryId == null || it.library.id == libraryId }
            .ifEmpty { state.movieLibraries }

    private fun matchingSeriesLibraries(state: LibraryRepositoryState, libraryId: String?): List<CachedSeriesLibrary> =
        state.seriesLibraries.filter { libraryId == null || it.library.id == libraryId }
            .ifEmpty { state.seriesLibraries }
}

private fun SeriesBundleAccessor?.toDetail(series: SeriesReference): SeriesBundleDetail {
    val seriesDetail = series.toDetail()
    val seasonDetails = this?.seasons.orEmpty()
        .map(SeasonReference::toDetail)
        .sortedBy { it.seasonNumber }
    val episodes = this?.episodes.orEmpty()
        .map(EpisodeReference::toDetail)
        .sortedWith(compareBy<EpisodeDetail> { it.seasonNumber }.thenBy { it.episodeNumber })
    val availability = if (episodes.isEmpty()) {
        EpisodesAvailability.Unavailable(
            message = "Episodes unavailable for this series bundle. Retry episodes to refresh the current per-series cache root.",
        )
    } else {
        EpisodesAvailability.Available(episodes.size)
    }
    return SeriesBundleDetail(
        series = seriesDetail,
        seasons = seasonDetails,
        episodesBySeason = episodes.groupBy { it.seasonNumber }.toSortedMap(),
        episodesAvailability = availability,
    )
}

private fun MovieReference.toDetail(@Suppress("UNUSED_PARAMETER") libraryName: String? = null): MovieDetail {
    val details = details
    return MovieDetail(
        id = id.toUuidString(),
        libraryId = libraryId.toUuidString(),
        title = title,
        overview = details?.overview.cleanOrNull(),
        releaseDate = details?.releaseDate.cleanOrNull(),
        runtimeMinutes = details?.runtime?.toInt()?.takeIf { it > 0 },
        voteAverage = details?.voteAverage?.takeIf { it > 0f },
        voteCount = details?.voteCount?.toInt()?.takeIf { it > 0 },
        contentRating = details?.contentRating.cleanOrNull(),
        genres = details?.genreNames().orEmpty(),
        tagline = details?.tagline.cleanOrNull(),
        status = details?.status.cleanOrNull(),
        tmdbId = tmdbId.toLong().takeIf { it > 0L },
        fileId = file?.safeId(),
        fileName = file?.filename,
        images = details.movieImages(),
    )
}

private fun SeriesReference.toDetail(): SeriesDetail {
    val details = details
    return SeriesDetail(
        id = id.toUuidString(),
        libraryId = libraryId.toUuidString(),
        title = title,
        overview = details?.overview.cleanOrNull(),
        firstAirDate = details?.firstAirDate.cleanOrNull(),
        lastAirDate = details?.lastAirDate.cleanOrNull(),
        availableSeasons = details?.availableSeasons?.toInt()?.takeIf { it > 0 },
        availableEpisodes = details?.availableEpisodes?.toInt()?.takeIf { it > 0 },
        numberOfSeasons = details?.numberOfSeasons?.toInt()?.takeIf { it > 0 },
        numberOfEpisodes = details?.numberOfEpisodes?.toInt()?.takeIf { it > 0 },
        voteAverage = details?.voteAverage?.takeIf { it > 0f },
        voteCount = details?.voteCount?.toInt()?.takeIf { it > 0 },
        contentRating = details?.contentRating.cleanOrNull(),
        genres = details?.genreNames().orEmpty(),
        tagline = details?.tagline.cleanOrNull(),
        status = details?.status.cleanOrNull(),
        inProduction = details?.inProduction ?: false,
        tmdbId = tmdbId.toLong().takeIf { it > 0L },
        images = details.seriesImages(),
    )
}

private fun SeasonReference.toDetail(): SeasonDetail {
    val details = details
    val fallbackTitle = "Season ${seasonNumber.toInt()}"
    return SeasonDetail(
        id = id.toUuidString(),
        seasonNumber = seasonNumber.toInt(),
        title = details?.name.cleanOrNull() ?: fallbackTitle,
        overview = details?.overview.cleanOrNull(),
        airDate = details?.airDate.cleanOrNull(),
        episodeCount = details?.episodeCount?.toInt()?.takeIf { it > 0 },
        runtimeMinutes = details?.runtime?.toInt()?.takeIf { it > 0 },
        images = details.seasonImages(),
    )
}

private fun EpisodeReference.toDetail(): EpisodeDetail {
    val details = details
    val season = seasonNumber.toInt()
    val episode = episodeNumber.toInt()
    val fallbackTitle = "S${season} E${episode}"
    return EpisodeDetail(
        id = id.toUuidString(),
        libraryId = libraryId.toUuidString(),
        seriesId = seriesId.toUuidString(),
        seasonNumber = season,
        episodeNumber = episode,
        title = details?.name.cleanOrNull() ?: fallbackTitle,
        overview = details?.overview.cleanOrNull(),
        airDate = details?.airDate.cleanOrNull(),
        runtimeMinutes = details?.runtime?.toInt()?.takeIf { it > 0 },
        tmdbSeriesId = tmdbSeriesId.toLong().takeIf { it > 0L },
        fileId = file?.safeId(),
        fileName = file?.filename,
        images = details.episodeImages(),
    )
}

private fun MediaFile.safeId(): String? = runCatching { id.toUuidString() }.getOrNull()

private fun EnhancedMovieDetails?.movieImages(): DetailImageSet = DetailImageSet(
    poster = this?.primaryPosterIid?.toUuidString()?.let { ImageRequestKey(it, BrowseImageCategory.Poster) },
    backdrop = this?.primaryBackdropIid?.toUuidString()?.let { ImageRequestKey(it, BrowseImageCategory.Backdrop) },
    posterFallbackPath = this?.posterPath.cleanOrNull(),
    backdropFallbackPath = this?.backdropPath.cleanOrNull(),
)

private fun EnhancedSeriesDetails?.seriesImages(): DetailImageSet = DetailImageSet(
    poster = this?.primaryPosterIid?.toUuidString()?.let { ImageRequestKey(it, BrowseImageCategory.Poster) },
    backdrop = this?.primaryBackdropIid?.toUuidString()?.let { ImageRequestKey(it, BrowseImageCategory.Backdrop) },
    posterFallbackPath = this?.posterPath.cleanOrNull(),
    backdropFallbackPath = this?.backdropPath.cleanOrNull(),
)

private fun SeasonDetails?.seasonImages(): DetailImageSet = DetailImageSet(
    poster = this?.primaryPosterIid?.toUuidString()?.let { ImageRequestKey(it, BrowseImageCategory.Poster) },
    posterFallbackPath = this?.posterPath.cleanOrNull(),
)

private fun EpisodeDetails?.episodeImages(): DetailImageSet = DetailImageSet(
    still = this?.primaryStillIid?.toUuidString()?.let { ImageRequestKey(it, BrowseImageCategory.Episode) },
    stillFallbackPath = this?.stillPath.cleanOrNull(),
)

private fun EnhancedMovieDetails.genreNames(): List<String> = buildList {
    for (index in 0 until genresLength) {
        genres(index)?.name?.cleanOrNull()?.let(::add)
    }
}

private fun EnhancedSeriesDetails.genreNames(): List<String> = buildList {
    for (index in 0 until genresLength) {
        genres(index)?.name?.cleanOrNull()?.let(::add)
    }
}

private fun String?.cleanOrNull(): String? = this?.trim()?.takeIf { it.isNotEmpty() }
