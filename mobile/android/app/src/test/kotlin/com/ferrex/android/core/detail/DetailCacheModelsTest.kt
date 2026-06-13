package com.ferrex.android.core.detail

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.library.CachedMovieLibrary
import com.ferrex.android.core.library.CachedSeriesLibrary
import com.ferrex.android.core.library.LibraryFlatBuffers
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.LibraryKind
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.MovieLibraryAccessor
import com.ferrex.android.core.library.SeriesLibraryAccessor
import com.ferrex.android.core.library.toFlatBufferUuid
import com.ferrex.android.core.watch.WatchMediaProgress
import com.google.flatbuffers.FlatBufferBuilder
import ferrex.details.EnhancedMovieDetails
import ferrex.details.EnhancedSeriesDetails
import ferrex.details.EpisodeDetails
import ferrex.details.SeasonDetails
import ferrex.files.MediaFile
import ferrex.library.MediaBatchData
import ferrex.library.SeriesBundleData
import ferrex.media.EpisodeReference
import ferrex.media.Media
import ferrex.media.MediaVariant
import ferrex.media.MovieReference
import ferrex.media.SeasonReference
import ferrex.media.SeriesReference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

@OptIn(ExperimentalUnsignedTypes::class)
class DetailCacheModelsTest {
    @Test
    fun movieDetailLoadsFromCachedBatchAndBuildsPlaybackContracts() {
        val ids = Ids()
        val library = LibraryInfo(ids.movieLibrary.toString(), "Movies", LibraryKind.Movies)
        val state = LibraryRepositoryState(
            movieLibraries = listOf(CachedMovieLibrary(library, MovieLibraryAccessor(moviePayload(ids)))),
        )
        val route = MediaRouteArgs(BrowseMediaType.Movie, ids.movie.toString(), library.id, BrowseSourceSurface.LibraryGrid)

        val result = DetailCache.resolve(state, route) as DetailLoadResult.Movie

        assertEquals("Cache Movie", result.detail.title)
        assertEquals("cached overview", result.detail.overview)
        assertEquals(ids.movieFile.toString(), result.detail.fileId)
        assertTrue(DetailCache.imageKeys(result).any { it.category == BrowseImageCategory.Poster })
        assertTrue(DetailCache.imageKeys(result).any { it.category == BrowseImageCategory.Backdrop })

        val resume = DetailRouteContracts.movieResume(
            movie = result.detail,
            progress = WatchMediaProgress(result.detail.id, positionSeconds = 120.0, durationSeconds = 1_200.0, percentage = 10.0),
            sourceRoute = route,
        )!!
        val startOver = DetailRouteContracts.movieStartOver(result.detail, route)!!

        assertEquals(ids.movieFile.toString(), resume.targetMediaId)
        assertEquals(ids.movie.toString(), resume.logicalMediaId)
        assertEquals(120.0, resume.startPositionSeconds!!, 0.0)
        assertFalse(resume.startOver)
        assertTrue(startOver.startOver)
    }

    @Test
    fun movieCacheMissProducesRecoverableActions() {
        val route = MediaRouteArgs(BrowseMediaType.Movie, uuid(999).toString(), uuid(1).toString(), BrowseSourceSurface.LibraryGrid)

        val result = DetailCache.resolve(LibraryRepositoryState(), route)
        val actions = DetailCache.recoveryActions(route)

        assertTrue(result is DetailLoadResult.Missing)
        assertTrue(actions.back)
        assertTrue(actions.retryCacheSync)
        assertTrue(actions.clearSelectedCache)
        assertTrue(actions.changeServer)
        assertTrue(actions.resetConnection)
    }

    @Test
    fun seriesDetailUsesCurrentPerSeriesBundleRootForSeasonsAndEpisodes() {
        val ids = Ids()
        val library = LibraryInfo(ids.seriesLibrary.toString(), "Series", LibraryKind.Series)
        val state = LibraryRepositoryState(
            seriesLibraries = listOf(CachedSeriesLibrary(library, SeriesLibraryAccessor(seriesPayload(ids, includeEpisode = true)))),
        )
        val route = MediaRouteArgs(BrowseMediaType.Series, ids.series.toString(), library.id, BrowseSourceSurface.LibraryGrid)

        val result = DetailCache.resolve(state, route) as DetailLoadResult.Series

        assertEquals("Cache Series", result.detail.series.title)
        assertEquals(listOf(1), result.detail.seasons.map { it.seasonNumber })
        assertEquals(1, result.detail.episodes.size)
        assertTrue(result.detail.episodesAvailability is EpisodesAvailability.Available)
        assertEquals(ids.episodeFile.toString(), result.detail.firstPlayableEpisode?.fileId)
        assertTrue(DetailCache.imageKeys(result).any { it.category == BrowseImageCategory.Episode })
    }

    @Test
    fun seriesWithoutParsedEpisodesShowsRetryableUnavailableStateNotIndefiniteLoading() {
        val ids = Ids()
        val library = LibraryInfo(ids.seriesLibrary.toString(), "Series", LibraryKind.Series)
        val state = LibraryRepositoryState(
            seriesLibraries = listOf(CachedSeriesLibrary(library, SeriesLibraryAccessor(seriesPayload(ids, includeEpisode = false)))),
        )
        val route = MediaRouteArgs(BrowseMediaType.Series, ids.series.toString(), library.id, BrowseSourceSurface.LibraryGrid)

        val result = DetailCache.resolve(state, route) as DetailLoadResult.Series
        val unavailable = result.detail.episodesAvailability as EpisodesAvailability.Unavailable

        assertTrue(unavailable.message.contains("Episodes unavailable"))
        assertFalse(unavailable.message.contains("loading", ignoreCase = true))
        assertEquals("Retry episodes", unavailable.retryLabel)
    }

    @Test
    fun recoveryActionsHideSelectedCacheClearWhenRouteHasNoLibrary() {
        val route = MediaRouteArgs(BrowseMediaType.Episode, uuid(77).toString(), null, BrowseSourceSurface.HomeContinueWatching)

        val actions = DetailCache.recoveryActions(route)

        assertTrue(actions.back)
        assertTrue(actions.retryCacheSync)
        assertFalse(actions.clearSelectedCache)
    }

    private inner class Ids {
        val movieLibrary: UUID = uuid(1)
        val seriesLibrary: UUID = uuid(2)
        val movie: UUID = uuid(10)
        val movieFile: UUID = uuid(11)
        val series: UUID = uuid(20)
        val season: UUID = uuid(21)
        val episode: UUID = uuid(22)
        val episodeFile: UUID = uuid(23)
        val poster: UUID = uuid(30)
        val backdrop: UUID = uuid(31)
        val still: UUID = uuid(32)
    }

    private fun moviePayload(ids: Ids): List<com.ferrex.android.core.library.ParsedMovieBatch> {
        val builder = FlatBufferBuilder(1024)
        val details = movieDetails(builder, ids)
        val file = mediaFile(builder, ids.movieFile, "movie.mkv")
        val title = builder.createString("Cache Movie")
        MovieReference.startMovieReference(builder)
        MovieReference.addBatchId(builder, 1u)
        MovieReference.addTmdbId(builder, 100UL)
        MovieReference.addTitle(builder, title)
        MovieReference.addDetails(builder, details)
        MovieReference.addFile(builder, file)
        MovieReference.addLibraryId(builder, ids.movieLibrary.toFlatBufferUuid(builder))
        MovieReference.addId(builder, ids.movie.toFlatBufferUuid(builder))
        val movie = MovieReference.endMovieReference(builder)
        val media = Media.createMedia(builder, MediaVariant.MovieReference, movie)
        val items = MediaBatchData.createItemsVector(builder, intArrayOf(media))
        val root = MediaBatchData.createMediaBatchData(builder, 1u, 1UL, items)
        builder.finish(root)
        return LibraryFlatBuffers.parseMoviePayload(builder.sizedByteArray().wrap(), expectedBatchId = 1).getOrThrow()
    }

    private fun seriesPayload(ids: Ids, includeEpisode: Boolean): List<com.ferrex.android.core.library.ParsedSeriesBundle> {
        val builder = FlatBufferBuilder(2048)
        val media = buildList {
            add(seriesReference(builder, ids))
            add(seasonReference(builder, ids))
            if (includeEpisode) add(episodeReference(builder, ids))
        }.toIntArray()
        val items = SeriesBundleData.createItemsVector(builder, media)
        SeriesBundleData.startSeriesBundleData(builder)
        SeriesBundleData.addVersion(builder, 7UL)
        SeriesBundleData.addItems(builder, items)
        SeriesBundleData.addSeriesId(builder, ids.series.toFlatBufferUuid(builder))
        val root = SeriesBundleData.endSeriesBundleData(builder)
        builder.finish(root)
        return LibraryFlatBuffers.parseSeriesPayload(builder.sizedByteArray().wrap(), expectedSeriesId = ids.series.toString()).getOrThrow()
    }

    private fun movieDetails(builder: FlatBufferBuilder, ids: Ids): Int {
        val title = builder.createString("Cache Movie")
        val overview = builder.createString("cached overview")
        val release = builder.createString("2024-01-02")
        EnhancedMovieDetails.startEnhancedMovieDetails(builder)
        EnhancedMovieDetails.addTitle(builder, title)
        EnhancedMovieDetails.addOverview(builder, overview)
        EnhancedMovieDetails.addReleaseDate(builder, release)
        EnhancedMovieDetails.addRuntime(builder, 95u)
        EnhancedMovieDetails.addVoteAverage(builder, 7.5f)
        EnhancedMovieDetails.addPrimaryPosterIid(builder, ids.poster.toFlatBufferUuid(builder))
        EnhancedMovieDetails.addPrimaryBackdropIid(builder, ids.backdrop.toFlatBufferUuid(builder))
        return EnhancedMovieDetails.endEnhancedMovieDetails(builder)
    }

    private fun seriesDetails(builder: FlatBufferBuilder, ids: Ids): Int {
        val name = builder.createString("Cache Series")
        val overview = builder.createString("series overview")
        EnhancedSeriesDetails.startEnhancedSeriesDetails(builder)
        EnhancedSeriesDetails.addName(builder, name)
        EnhancedSeriesDetails.addOverview(builder, overview)
        EnhancedSeriesDetails.addAvailableSeasons(builder, 1.toUShort())
        EnhancedSeriesDetails.addAvailableEpisodes(builder, 1.toUShort())
        EnhancedSeriesDetails.addPrimaryPosterIid(builder, ids.poster.toFlatBufferUuid(builder))
        EnhancedSeriesDetails.addPrimaryBackdropIid(builder, ids.backdrop.toFlatBufferUuid(builder))
        return EnhancedSeriesDetails.endEnhancedSeriesDetails(builder)
    }

    private fun seasonDetails(builder: FlatBufferBuilder): Int {
        val name = builder.createString("Season 1")
        SeasonDetails.startSeasonDetails(builder)
        SeasonDetails.addName(builder, name)
        SeasonDetails.addSeasonNumber(builder, 1.toUShort())
        SeasonDetails.addEpisodeCount(builder, 1.toUShort())
        return SeasonDetails.endSeasonDetails(builder)
    }

    private fun episodeDetails(builder: FlatBufferBuilder, ids: Ids): Int {
        val name = builder.createString("Pilot")
        EpisodeDetails.startEpisodeDetails(builder)
        EpisodeDetails.addName(builder, name)
        EpisodeDetails.addSeasonNumber(builder, 1.toUShort())
        EpisodeDetails.addEpisodeNumber(builder, 1.toUShort())
        EpisodeDetails.addRuntime(builder, 42u)
        EpisodeDetails.addPrimaryStillIid(builder, ids.still.toFlatBufferUuid(builder))
        return EpisodeDetails.endEpisodeDetails(builder)
    }

    private fun seriesReference(builder: FlatBufferBuilder, ids: Ids): Int {
        val title = builder.createString("Cache Series")
        val details = seriesDetails(builder, ids)
        SeriesReference.startSeriesReference(builder)
        SeriesReference.addTmdbId(builder, 1234UL)
        SeriesReference.addTitle(builder, title)
        SeriesReference.addDetails(builder, details)
        SeriesReference.addLibraryId(builder, ids.seriesLibrary.toFlatBufferUuid(builder))
        SeriesReference.addId(builder, ids.series.toFlatBufferUuid(builder))
        val series = SeriesReference.endSeriesReference(builder)
        return Media.createMedia(builder, MediaVariant.SeriesReference, series)
    }

    private fun seasonReference(builder: FlatBufferBuilder, ids: Ids): Int {
        val details = seasonDetails(builder)
        SeasonReference.startSeasonReference(builder)
        SeasonReference.addSeasonNumber(builder, 1.toUShort())
        SeasonReference.addTmdbSeriesId(builder, 1234UL)
        SeasonReference.addDetails(builder, details)
        SeasonReference.addLibraryId(builder, ids.seriesLibrary.toFlatBufferUuid(builder))
        SeasonReference.addSeriesId(builder, ids.series.toFlatBufferUuid(builder))
        SeasonReference.addId(builder, ids.season.toFlatBufferUuid(builder))
        val season = SeasonReference.endSeasonReference(builder)
        return Media.createMedia(builder, MediaVariant.SeasonReference, season)
    }

    private fun episodeReference(builder: FlatBufferBuilder, ids: Ids): Int {
        val details = episodeDetails(builder, ids)
        val file = mediaFile(builder, ids.episodeFile, "episode.mkv")
        EpisodeReference.startEpisodeReference(builder)
        EpisodeReference.addSeasonNumber(builder, 1.toUShort())
        EpisodeReference.addEpisodeNumber(builder, 1.toUShort())
        EpisodeReference.addTmdbSeriesId(builder, 1234UL)
        EpisodeReference.addDetails(builder, details)
        EpisodeReference.addFile(builder, file)
        EpisodeReference.addLibraryId(builder, ids.seriesLibrary.toFlatBufferUuid(builder))
        EpisodeReference.addSeasonId(builder, ids.season.toFlatBufferUuid(builder))
        EpisodeReference.addSeriesId(builder, ids.series.toFlatBufferUuid(builder))
        EpisodeReference.addId(builder, ids.episode.toFlatBufferUuid(builder))
        val episode = EpisodeReference.endEpisodeReference(builder)
        return Media.createMedia(builder, MediaVariant.EpisodeReference, episode)
    }

    private fun mediaFile(builder: FlatBufferBuilder, fileId: UUID, filenameValue: String): Int {
        val filename = builder.createString(filenameValue)
        MediaFile.startMediaFile(builder)
        MediaFile.addFilename(builder, filename)
        MediaFile.addId(builder, fileId.toFlatBufferUuid(builder))
        return MediaFile.endMediaFile(builder)
    }

    private fun uuid(seed: Int): UUID = UUID(0x1111000000000000L + seed, 0x8888000000000000UL.toLong() + seed)

    private fun ByteArray.wrap(): ByteBuffer = ByteBuffer.wrap(this).order(ByteOrder.LITTLE_ENDIAN)
}
