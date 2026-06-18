package com.ferrex.android.core.detail

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.RetryClassification
import com.ferrex.android.core.watch.WatchEpisodeState
import com.ferrex.android.core.watch.WatchEpisodeStatus
import com.ferrex.android.core.watch.WatchMediaProgress
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.core.watch.WatchSeasonStatus
import com.ferrex.android.core.watch.WatchSeriesStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DetailPageModelsTest {
    @Test
    fun moviePageNormalizesHeroActionsRailsAndPrefetchKeys() {
        val movie = movieDetail(
            progressFile = "movie-file",
            cast = listOf(castCredit("actor-one", "Actor One", "Captain", profile = key("actor-profile", BrowseImageCategory.Profile))),
            crew = listOf(crewCredit("crew-one", "Director One", "Director", profile = key("crew-profile", BrowseImageCategory.Profile))),
            recommendations = listOf(DetailRelatedMediaRef(tmdbId = 9001, title = "Recommended Movie")),
            similar = listOf(DetailRelatedMediaRef(tmdbId = 9002, title = "Similar Movie")),
        )
        val route = route(BrowseMediaType.Movie, movie.id, movie.libraryId)
        val page = DetailPageMapper.toPage(
            DetailLoadResult.Movie(route, movie),
            watchState = WatchRepositoryState(
                media = mapOf(movie.id to WatchMediaProgress(movie.id, positionSeconds = 120.0, durationSeconds = 1_200.0)),
            ),
        )

        assertEquals(DetailPageKind.Movie, page.kind)
        assertEquals(key("backdrop", BrowseImageCategory.Backdrop), page.hero.background.requestKey)
        assertEquals(key("poster", BrowseImageCategory.Poster), page.hero.foreground?.requestKey)
        assertTrue(page.actions.any { it.kind == DetailPageActionKind.Resume && it.playbackContract?.startPositionSeconds == 120.0 })
        assertTrue(page.actions.any { it.kind == DetailPageActionKind.StartOver })
        assertTrue(page.actions.any { it.kind == DetailPageActionKind.ClearProgress })
        assertTrue(page.actions.any { it.kind == DetailPageActionKind.MarkWatched && it.targetWatched == true })
        assertEquals(DetailRailState.Available, page.rail(DetailRailKind.Cast)?.state)
        assertEquals(DetailRailState.Available, page.rail(DetailRailKind.Crew)?.state)
        assertEquals(DetailRailState.Available, page.rail(DetailRailKind.Recommendations)?.state)
        assertTrue(page.imageKeys.contains(key("backdrop", BrowseImageCategory.Backdrop)))
        assertTrue(page.imageKeys.contains(key("poster", BrowseImageCategory.Poster)))
        assertTrue(page.imageKeys.contains(key("actor-profile", BrowseImageCategory.Profile)))
        assertTrue(page.imageKeys.contains(key("crew-profile", BrowseImageCategory.Profile)))
    }

    @Test
    fun imageStatesRepresentPendingFailedStaleAndNoArt() {
        val movie = movieDetail(progressFile = "movie-file")
        val page = DetailPageMapper.toPage(
            DetailLoadResult.Movie(route(BrowseMediaType.Movie, movie.id, movie.libraryId), movie),
            imageResolutions = mapOf(
                key("backdrop", BrowseImageCategory.Backdrop) to ImageResolution.Pending(
                    key = key("backdrop", BrowseImageCategory.Backdrop),
                    retryAfterMillis = 5_000,
                    retryAtMillis = 10_000,
                    stale = true,
                    offlineMessage = "server unreachable",
                ),
                key("poster", BrowseImageCategory.Poster) to ImageResolution.Failed(
                    key = key("poster", BrowseImageCategory.Poster),
                    reason = "decode failed",
                    retryable = true,
                ),
            ),
            libraryFreshness = LibraryFreshness.StaleOffline("server unreachable", itemCount = 4, lastSyncedAtMillis = null),
        )

        val backgroundState = page.hero.background.imageState
        val foregroundState = page.hero.foreground?.imageState
        assertTrue(backgroundState is DetailImageState.Pending)
        assertTrue(backgroundState.staleOffline)
        assertTrue(foregroundState is DetailImageState.Failed)
        assertEquals(DetailFreshnessKind.StaleOffline, page.recovery.freshness?.kind)

        val noArtPage = DetailPageMapper.toPage(
            DetailLoadResult.Movie(route(BrowseMediaType.Movie, "no-art", "library"), movie.copy(id = "no-art", images = DetailImageSet())),
        )
        assertTrue(noArtPage.hero.background.imageState is DetailImageState.NoArt)
        assertNull(noArtPage.hero.foreground)
    }

    @Test
    fun contractGatedRailsExposeUnavailableInsteadOfFalseEmptyStates() {
        val series = seriesDetail()
        val bundle = SeriesBundleDetail(
            series = series,
            seasons = emptyList(),
            episodesBySeason = emptyMap(),
            episodesAvailability = EpisodesAvailability.Unavailable("Episodes unavailable for this series bundle."),
        )
        val page = DetailPageMapper.toPage(
            DetailLoadResult.Series(route(BrowseMediaType.Series, series.id, series.libraryId), bundle),
            libraryFreshness = LibraryFreshness.ErrorRetryable("network down", RetryClassification.Retryable),
        )

        assertEquals(DetailRailState.Empty, page.rail(DetailRailKind.Seasons)?.state)
        assertEquals(DetailRailState.Unavailable, page.rail(DetailRailKind.Episodes)?.state)
        assertEquals(DetailRailState.Unavailable, page.rail(DetailRailKind.Cast)?.state)
        assertEquals(DetailRailState.Unavailable, page.rail(DetailRailKind.Recommendations)?.state)
        assertEquals(DetailFreshnessKind.RecoverableError, page.recovery.freshness?.kind)
        assertTrue(page.actions.any { it.kind == DetailPageActionKind.RetryWatch })
        assertTrue(page.recovery.actions.any { it.kind == DetailPageActionKind.RetryCache })
        assertTrue(page.recovery.actions.any { it.kind == DetailPageActionKind.ClearSelectedCache })
        assertTrue(page.recovery.actions.any { it.kind == DetailPageActionKind.ChangeServer })
        assertTrue(page.recovery.actions.any { it.kind == DetailPageActionKind.ResetConnection })
        assertTrue(page.recovery.actions.any { it.kind == DetailPageActionKind.Diagnostics })
    }

    @Test
    fun seriesPrefetchIncludesHeroAndBoundedVisibleRailImages() {
        val episodes = (1..20).map { index ->
            episodeDetail(
                id = "episode-$index",
                episodeNumber = index,
                images = DetailImageSet(still = key("still-$index", BrowseImageCategory.Episode)),
            )
        }
        val series = seriesDetail()
        val page = DetailPageMapper.toPage(
            DetailLoadResult.Series(
                route = route(BrowseMediaType.Series, series.id, series.libraryId),
                detail = SeriesBundleDetail(
                    series = series,
                    seasons = emptyList(),
                    episodesBySeason = episodes.groupBy { it.seasonNumber },
                    episodesAvailability = EpisodesAvailability.Available(episodes.size),
                ),
            ),
            prefetchPolicy = DetailImagePrefetchPolicy(visibleRailItemWindow = 5, maxImageKeys = 8),
        )

        assertTrue(page.imageKeys.contains(key("poster", BrowseImageCategory.Poster)))
        assertTrue(page.imageKeys.contains(key("backdrop", BrowseImageCategory.Backdrop)))
        (1..5).forEach { index ->
            assertTrue(page.imageKeys.contains(key("still-$index", BrowseImageCategory.Episode)))
        }
        assertFalse(page.imageKeys.contains(key("still-6", BrowseImageCategory.Episode)))
        assertTrue(page.imageKeys.size <= 8)
        assertEquals(5, page.imagePrefetch.visibleRailItemWindow)
    }

    @Test
    fun seasonPageBuildsSeasonSurfaceWithEpisodeRailAndWatchActions() {
        val series = seriesDetail()
        val season = SeasonDetail(
            id = "season-1",
            seasonNumber = 1,
            title = "Season 1",
            overview = "season overview",
            airDate = "2024-02-01",
            episodeCount = 2,
            runtimeMinutes = null,
            images = DetailImageSet(poster = key("season-poster", BrowseImageCategory.Poster)),
        )
        val episodes = listOf(
            episodeDetail(id = "episode-1", episodeNumber = 1, images = DetailImageSet(still = key("season-still-1", BrowseImageCategory.Episode))),
            episodeDetail(id = "episode-2", episodeNumber = 2, images = DetailImageSet(still = key("season-still-2", BrowseImageCategory.Episode))),
        )
        val page = DetailPageMapper.seasonPage(
            route = route(BrowseMediaType.Series, series.id, series.libraryId),
            series = series,
            season = season,
            episodes = episodes,
            watchState = WatchRepositoryState(
                series = mapOf(
                    1234L to WatchSeriesStatus(
                        tmdbSeriesId = 1234,
                        totalEpisodes = 2,
                        watched = 2,
                        inProgress = 0,
                        seasons = mapOf(
                            1 to WatchSeasonStatus(
                                seasonNumber = 1,
                                total = 2,
                                watched = 2,
                                inProgress = 0,
                                isCompleted = true,
                                episodes = emptyMap(),
                            ),
                        ),
                        nextEpisode = null,
                    ),
                ),
            ),
        )

        assertEquals(DetailPageKind.Season, page.kind)
        assertEquals(DetailWatchStateKind.Watched, page.watchState?.state)
        assertEquals(DetailRailState.Available, page.rail(DetailRailKind.Episodes)?.state)
        assertTrue(page.actions.any { it.kind == DetailPageActionKind.MarkUnwatched && it.targetWatched == false })
        assertTrue(page.imageKeys.contains(key("season-poster", BrowseImageCategory.Poster)))
        assertTrue(page.imageKeys.contains(key("season-still-1", BrowseImageCategory.Episode)))
    }

    @Test
    fun episodePageMapsSiblingEpisodeAndGuestProfileRails() {
        val parent = seriesDetail(
            cast = listOf(castCredit("series-actor", "Series Actor", "Lead", profile = key("series-profile", BrowseImageCategory.Profile))),
        )
        val current = episodeDetail(
            id = "episode-2",
            episodeNumber = 2,
            images = DetailImageSet(still = key("still-2", BrowseImageCategory.Episode)),
            guestStars = listOf(castCredit("guest-one", "Guest One", "Guest", profile = key("guest-profile", BrowseImageCategory.Profile))),
        )
        val sibling = episodeDetail(
            id = "episode-1",
            episodeNumber = 1,
            images = DetailImageSet(still = key("still-1", BrowseImageCategory.Episode)),
        )
        val page = DetailPageMapper.toPage(
            DetailLoadResult.Episode(
                route = route(BrowseMediaType.Episode, current.id, current.libraryId),
                detail = current,
                parentSeries = parent,
                siblingEpisodes = listOf(sibling, current),
            ),
            watchState = WatchRepositoryState(
                series = mapOf(
                    1234L to WatchSeriesStatus(
                        tmdbSeriesId = 1234,
                        totalEpisodes = 2,
                        watched = 1,
                        inProgress = 1,
                        seasons = mapOf(
                            1 to WatchSeasonStatus(
                                seasonNumber = 1,
                                total = 2,
                                watched = 1,
                                inProgress = 1,
                                isCompleted = false,
                                episodes = mapOf(
                                    1 to WatchEpisodeStatus(WatchEpisodeState.Completed),
                                    2 to WatchEpisodeStatus(WatchEpisodeState.InProgress, progress = 0.5f),
                                ),
                            ),
                        ),
                        nextEpisode = null,
                    ),
                ),
            ),
        )

        assertEquals(DetailRailState.Available, page.rail(DetailRailKind.SiblingEpisodes)?.state)
        assertEquals(2, page.rail(DetailRailKind.SiblingEpisodes)?.items?.size)
        assertEquals(DetailRailState.Available, page.rail(DetailRailKind.Cast)?.state)
        assertEquals(DetailRailState.Available, page.rail(DetailRailKind.GuestCast)?.state)
        assertTrue(page.imageKeys.contains(key("series-profile", BrowseImageCategory.Profile)))
        assertTrue(page.imageKeys.contains(key("still-1", BrowseImageCategory.Episode)))
        assertTrue(page.imageKeys.contains(key("still-2", BrowseImageCategory.Episode)))
        assertTrue(page.imageKeys.contains(key("guest-profile", BrowseImageCategory.Profile)))
        assertNotNull(page.watchState)
        assertEquals(DetailWatchStateKind.InProgress, page.watchState?.state)
    }

    private fun movieDetail(
        progressFile: String?,
        cast: List<DetailCastCredit> = emptyList(),
        crew: List<DetailCrewCredit> = emptyList(),
        recommendations: List<DetailRelatedMediaRef> = emptyList(),
        similar: List<DetailRelatedMediaRef> = emptyList(),
    ): MovieDetail = MovieDetail(
        id = "movie",
        libraryId = "library",
        title = "Cache Movie",
        overview = "cached overview",
        releaseDate = "2024-01-02",
        runtimeMinutes = 95,
        voteAverage = 7.5f,
        voteCount = 42,
        contentRating = "PG-13",
        genres = listOf("Drama"),
        tagline = "A cached story",
        status = "Released",
        tmdbId = 100,
        fileId = progressFile,
        fileName = progressFile?.let { "movie.mkv" },
        images = DetailImageSet(
            poster = key("poster", BrowseImageCategory.Poster),
            backdrop = key("backdrop", BrowseImageCategory.Backdrop),
            posterFallbackPath = "/poster.jpg",
            backdropFallbackPath = "/backdrop.jpg",
        ),
        cast = cast,
        crew = crew,
        recommendations = recommendations,
        similar = similar,
    )

    private fun seriesDetail(
        cast: List<DetailCastCredit> = emptyList(),
        crew: List<DetailCrewCredit> = emptyList(),
        recommendations: List<DetailRelatedMediaRef> = emptyList(),
        similar: List<DetailRelatedMediaRef> = emptyList(),
    ): SeriesDetail = SeriesDetail(
        id = "series",
        libraryId = "library",
        title = "Cache Series",
        overview = "series overview",
        firstAirDate = "2024-02-03",
        lastAirDate = null,
        availableSeasons = 1,
        availableEpisodes = 2,
        numberOfSeasons = 1,
        numberOfEpisodes = 2,
        voteAverage = 8.1f,
        voteCount = 88,
        contentRating = "TV-14",
        genres = listOf("Sci-Fi"),
        tagline = "Signal found",
        status = "Returning Series",
        inProduction = true,
        tmdbId = 1234,
        images = DetailImageSet(
            poster = key("poster", BrowseImageCategory.Poster),
            backdrop = key("backdrop", BrowseImageCategory.Backdrop),
        ),
        cast = cast,
        crew = crew,
        recommendations = recommendations,
        similar = similar,
    )

    private fun episodeDetail(
        id: String = "episode-1",
        episodeNumber: Int = 1,
        images: DetailImageSet = DetailImageSet(still = key("still", BrowseImageCategory.Episode)),
        guestStars: List<DetailCastCredit> = emptyList(),
    ): EpisodeDetail = EpisodeDetail(
        id = id,
        libraryId = "library",
        seriesId = "series",
        seasonNumber = 1,
        episodeNumber = episodeNumber,
        title = "Episode $episodeNumber",
        overview = "episode overview $episodeNumber",
        airDate = "2024-02-${episodeNumber.toString().padStart(2, '0')}",
        runtimeMinutes = 42,
        tmdbSeriesId = 1234,
        fileId = "file-$id",
        fileName = "$id.mkv",
        images = images,
        guestStars = guestStars,
    )

    private fun castCredit(
        personId: String,
        name: String,
        character: String,
        profile: ImageRequestKey?,
    ): DetailCastCredit = DetailCastCredit(
        personTmdbId = null,
        personId = personId,
        creditId = null,
        castId = null,
        name = name,
        originalName = null,
        character = character,
        order = 0,
        gender = null,
        knownForDepartment = "Acting",
        profileImages = DetailProfileImageSet(profile = profile),
    )

    private fun crewCredit(
        personId: String,
        name: String,
        job: String,
        profile: ImageRequestKey?,
    ): DetailCrewCredit = DetailCrewCredit(
        personTmdbId = null,
        personId = personId,
        creditId = null,
        name = name,
        job = job,
        department = job,
        gender = null,
        knownForDepartment = job,
        originalName = null,
        profileImages = DetailProfileImageSet(profile = profile),
    )

    private fun route(type: BrowseMediaType, id: String, libraryId: String): MediaRouteArgs = MediaRouteArgs(
        mediaType = type,
        mediaId = id,
        libraryId = libraryId,
        sourceSurface = BrowseSourceSurface.LibraryGrid,
    )

    private fun key(id: String, category: BrowseImageCategory): ImageRequestKey = ImageRequestKey(id, category)
}
