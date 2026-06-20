package com.ferrex.android.core.browse

import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.library.CachedMovieLibrary
import com.ferrex.android.core.library.CachedSeriesLibrary
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.MovieLibraryAccessor
import com.ferrex.android.core.library.RetryClassification
import com.ferrex.android.core.library.SeriesLibraryAccessor
import com.ferrex.android.core.library.toJavaUuidOrNull
import com.ferrex.android.core.library.toUuidString

enum class BrowseMediaType(val routeValue: String, val displayName: String) {
    Movie("movie", "Movie"),
    Series("series", "Series"),
    Season("season", "Season"),
    Episode("episode", "Episode"),
    Unknown("unknown", "Media"),
    ;

    companion object {
        fun fromApi(value: String?): BrowseMediaType = when (value?.trim()?.lowercase()) {
            "movie" -> Movie
            "series" -> Series
            "season" -> Season
            "episode" -> Episode
            else -> Unknown
        }
    }
}

enum class BrowseSourceSurface(val routeValue: String) {
    HomeContinueWatching("home_continue_watching"),
    HomeShelf("home_shelf"),
    LibraryGrid("library_grid"),
    Search("search"),
}

data class MediaRouteArgs(
    val mediaType: BrowseMediaType,
    val mediaId: String,
    val libraryId: String?,
    val sourceSurface: BrowseSourceSurface,
) {
    val stableKey: String = listOfNotNull(mediaType.routeValue, mediaId, libraryId, sourceSurface.routeValue).joinToString(":")

    fun toRouteString(): String = buildString {
        append("media/")
        append(mediaType.routeValue)
        append('/')
        append(mediaId)
        append("?source=")
        append(sourceSurface.routeValue)
        libraryId?.let {
            append("&libraryId=")
            append(it)
        }
    }
}

data class LibraryMediaCard(
    val stableKey: String,
    val title: String,
    val subtitle: String,
    val libraryName: String,
    val route: MediaRouteArgs,
    val imageKey: ImageRequestKey?,
    val publicFallbackPath: String?,
    val releaseDate: String?,
    val secondarySortMillis: Long? = null,
)

data class HomeShelf(
    val title: String,
    val subtitle: String,
    val previewLimit: Int,
    val fullItemCount: Int,
    val items: List<LibraryMediaCard>,
) {
    val limitCopy: String = if (fullItemCount > items.size) {
        "Shelf preview limit ${items.size} of $fullItemCount. Open the Library tab for every cached item."
    } else {
        "Showing all $fullItemCount cached item(s); no shelf cap applied."
    }
}

data class LibraryStatusCopy(
    val title: String,
    val detail: String,
    val isStale: Boolean = false,
    val isRecoverableError: Boolean = false,
)

data class LibraryRecoveryActionVisibility(
    val retry: Boolean,
    val clearSelectedCache: Boolean,
    val changeServer: Boolean,
    val resetConnection: Boolean,
)

enum class HomeLibraryTab(val label: String) {
    Movies("Movies"),
    Series("Series"),
}

enum class MovieSortMode(
    val label: String,
    val endpointSort: String,
    val endpointOrder: String,
) {
    TitleAsc("Title A-Z", "title", "asc"),
    ReleaseDateDesc("Release date", "release_date", "desc"),
    RecentlyAddedDesc("Recently added", "date_added", "desc"),
    RatingDesc("Rating", "rating", "desc"),
}

enum class MovieFilterMode(val label: String) {
    All("All movies"),
    HighRated("Rating 7+"),
}

data class IndexedMovieCards(
    val cards: List<LibraryMediaCard>,
    val invalidIndexCount: Int,
    val appendedMissingCount: Int,
)

object LibraryBrowseModels {
    const val DEFAULT_HOME_SHELF_PREVIEW_LIMIT = 12

    fun movieGridCards(movieLibrary: CachedMovieLibrary): List<LibraryMediaCard> =
        movieGridCards(movieLibrary.library, movieLibrary.accessor)

    fun movieGridCards(library: LibraryInfo, accessor: MovieLibraryAccessor): List<LibraryMediaCard> = buildList {
        for (index in 0 until accessor.movieCount) {
            val movie = accessor.movieAt(index) ?: continue
            val details = movie.details
            val mediaId = movie.id.toUuidString()
            val libraryId = runCatching { movie.libraryId.toUuidString() }.getOrDefault(library.id)
            val releaseDate = details?.releaseDate?.takeIf { it.isNotBlank() }
            val imageKey = details?.primaryPosterIid?.toUuidString()?.validUuidOrNull()?.let {
                ImageRequestKey(it, BrowseImageCategory.Poster)
            }
            add(
                LibraryMediaCard(
                    stableKey = "movie:$libraryId:$mediaId",
                    title = movie.title,
                    subtitle = movieSubtitle(releaseDate, details?.runtime?.toInt()),
                    libraryName = library.name,
                    route = MediaRouteArgs(
                        mediaType = BrowseMediaType.Movie,
                        mediaId = mediaId,
                        libraryId = libraryId,
                        sourceSurface = BrowseSourceSurface.LibraryGrid,
                    ),
                    imageKey = imageKey,
                    publicFallbackPath = details?.posterPath,
                    releaseDate = releaseDate,
                ),
            )
        }
    }

    fun seriesGridCards(seriesLibrary: CachedSeriesLibrary): List<LibraryMediaCard> =
        seriesGridCards(seriesLibrary.library, seriesLibrary.accessor)

    fun seriesGridCards(library: LibraryInfo, accessor: SeriesLibraryAccessor): List<LibraryMediaCard> = buildList {
        for (index in 0 until accessor.seriesReferenceCount) {
            val series = accessor.seriesAt(index) ?: continue
            val details = series.details
            val mediaId = series.id.toUuidString()
            val libraryId = runCatching { series.libraryId.toUuidString() }.getOrDefault(library.id)
            val firstAirDate = details?.firstAirDate?.takeIf { it.isNotBlank() }
            val imageKey = details?.primaryPosterIid?.toUuidString()?.validUuidOrNull()?.let {
                ImageRequestKey(it, BrowseImageCategory.Poster)
            }
            add(
                LibraryMediaCard(
                    stableKey = "series:$libraryId:$mediaId",
                    title = series.title,
                    subtitle = seriesSubtitle(firstAirDate, details?.availableEpisodes?.toInt()),
                    libraryName = library.name,
                    route = MediaRouteArgs(
                        mediaType = BrowseMediaType.Series,
                        mediaId = mediaId,
                        libraryId = libraryId,
                        sourceSurface = BrowseSourceSurface.LibraryGrid,
                    ),
                    imageKey = imageKey,
                    publicFallbackPath = details?.posterPath,
                    releaseDate = firstAirDate,
                    secondarySortMillis = series.discoveredAt?.millis ?: series.createdAt?.millis,
                ),
            )
        }
    }

    fun homeShelves(
        movieLibraries: List<CachedMovieLibrary>,
        seriesLibraries: List<CachedSeriesLibrary>,
        previewLimit: Int = DEFAULT_HOME_SHELF_PREVIEW_LIMIT,
    ): List<HomeShelf> {
        val movieCards = movieLibraries.flatMap(::movieGridCards)
        val seriesCards = seriesLibraries.flatMap(::seriesGridCards)
        return buildList {
            movieCards.sortedByDescending { it.releaseDate.orEmpty() }.takeIf { it.isNotEmpty() }?.let { sorted ->
                add(
                    HomeShelf(
                        title = "Recently released movies",
                        subtitle = "Local shelf from cached complete movie batches; not backend discovery.",
                        previewLimit = previewLimit,
                        fullItemCount = sorted.size,
                        items = sorted.take(previewLimit).map { it.forSurface(BrowseSourceSurface.HomeShelf) },
                    ),
                )
            }
            seriesCards.sortedWith(
                compareByDescending<LibraryMediaCard> { it.releaseDate.orEmpty() }
                    .thenByDescending { it.secondarySortMillis ?: 0L },
            ).takeIf { it.isNotEmpty() }?.let { sorted ->
                add(
                    HomeShelf(
                        title = "Recently aired series",
                        subtitle = "Local shelf from cached complete series bundles; not backend discovery.",
                        previewLimit = previewLimit,
                        fullItemCount = sorted.size,
                        items = sorted.take(previewLimit).map { it.forSurface(BrowseSourceSurface.HomeShelf) },
                    ),
                )
            }
        }
    }

    fun applyMovieIndices(
        cards: List<LibraryMediaCard>,
        indices: List<Int>,
        appendMissing: Boolean,
    ): IndexedMovieCards {
        val seen = LinkedHashSet<String>()
        var invalid = 0
        val indexed = buildList {
            indices.distinct().forEach { index ->
                val card = cards.getOrNull(index)
                if (card == null) {
                    invalid += 1
                } else if (seen.add(card.stableKey)) {
                    add(card)
                }
            }
        }
        if (!appendMissing) {
            return IndexedMovieCards(indexed, invalid, appendedMissingCount = 0)
        }
        val missing = cards.filter { it.stableKey !in seen }
        return IndexedMovieCards(indexed + missing, invalid, appendedMissingCount = missing.size)
    }

    fun libraryStatusCopy(freshness: LibraryFreshness): LibraryStatusCopy = when (freshness) {
        LibraryFreshness.Empty -> LibraryStatusCopy(
            title = "Library cache is empty",
            detail = "Retry to sync libraries, or change server/reset connection if this server is wrong.",
        )
        LibraryFreshness.Syncing -> LibraryStatusCopy(
            title = "Syncing library cache",
            detail = "Movies and series remain recoverable while cached payloads update.",
        )
        is LibraryFreshness.Fresh -> LibraryStatusCopy(
            title = "Library cache is fresh",
            detail = "${freshness.itemCount} cached item(s) available for this server and user.",
        )
        is LibraryFreshness.StaleOffline -> LibraryStatusCopy(
            title = "Stale/offline library cache",
            detail = "Showing ${freshness.itemCount} cached item(s): ${freshness.message}",
            isStale = true,
        )
        is LibraryFreshness.SeriesCacheIncomplete -> LibraryStatusCopy(
            title = "Series cache is still syncing",
            detail = "Showing ${freshness.itemCount} cached item(s): ${freshness.message}",
            isStale = true,
        )
        is LibraryFreshness.CorruptRebuilding -> LibraryStatusCopy(
            title = "Cache needs rebuild",
            detail = freshness.message,
            isRecoverableError = true,
        )
        is LibraryFreshness.ErrorRetryable -> LibraryStatusCopy(
            title = if (freshness.classification == RetryClassification.AuthRequired) "Library sync needs sign-in" else "Library sync failed",
            detail = freshness.message,
            isRecoverableError = true,
        )
    }

    fun recoveryActionVisibility(selectedLibraryId: String?): LibraryRecoveryActionVisibility = LibraryRecoveryActionVisibility(
        retry = true,
        clearSelectedCache = selectedLibraryId != null,
        changeServer = true,
        resetConnection = true,
    )

    fun unsupportedSeriesControlsCopy(): String =
        "Series sort and filters are disabled because the current index endpoints only support movie libraries. The full cached series grid remains available."

    private fun LibraryMediaCard.forSurface(surface: BrowseSourceSurface): LibraryMediaCard = copy(
        route = route.copy(sourceSurface = surface),
        stableKey = "${route.mediaType.routeValue}:${route.libraryId}:${route.mediaId}:${surface.routeValue}",
    )

    private fun movieSubtitle(releaseDate: String?, runtimeMinutes: Int?): String = buildList {
        releaseDate?.takeIf { it.length >= 4 }?.let { add(it.take(4)) }
        runtimeMinutes?.takeIf { it > 0 }?.let { add("${it} min") }
    }.ifEmpty { listOf("Movie") }.joinToString(" • ")

    private fun seriesSubtitle(firstAirDate: String?, episodes: Int?): String = buildList {
        firstAirDate?.takeIf { it.length >= 4 }?.let { add(it.take(4)) }
        episodes?.takeIf { it > 0 }?.let { add("$it episode(s)") }
    }.ifEmpty { listOf("Series") }.joinToString(" • ")

    private fun String.validUuidOrNull(): String? = takeIf { it.toJavaUuidOrNull() != null }
}
