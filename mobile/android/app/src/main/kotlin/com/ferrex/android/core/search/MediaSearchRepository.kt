package com.ferrex.android.core.search

import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.messageOrFallback
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.library.CachedMediaLookupKey
import com.ferrex.android.core.library.CachedMediaReference
import com.ferrex.android.core.library.CachedMediaResyncSummary
import com.ferrex.android.core.library.CachedMediaType
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.ServerCacheScope

interface MediaSearchCache {
    fun resolve(scope: ServerCacheScope, id: SearchMediaId): CachedMediaReference?
    fun freshness(scope: ServerCacheScope): LibraryFreshness
    suspend fun resync(scope: ServerCacheScope, id: SearchMediaId): CachedMediaResyncSummary
}

class MediaSearchRepository(
    private val transport: MediaSearchTransport,
    private val cache: MediaSearchCache,
) {
    suspend fun search(
        scope: ServerCacheScope,
        text: String,
        limit: Int = SearchLimits.DEFAULT,
    ): MediaSearchOutcome {
        val query = text.trim()
        if (query.isEmpty()) return MediaSearchOutcome.Idle

        return when (val response = transport.queryMedia(query, SearchLimits.normalize(limit))) {
            is ApiResult.Success -> resolveHits(scope, query, response.data)
            is ApiResult.HttpError -> MediaSearchOutcome.Failure(
                query = query,
                kind = SearchFailureKind.Http,
                message = "Server returned ${response.code}: ${response.message}",
                retryable = response.code == 408 || response.code == 409 || response.code == 425 || response.code == 429 || response.code >= 500,
            )
            is ApiResult.NetworkError -> MediaSearchOutcome.Failure(
                query = query,
                kind = SearchFailureKind.NetworkOffline,
                message = response.message.ifBlank { "Network unavailable" },
                retryable = true,
            )
            is ApiResult.ServerError -> MediaSearchOutcome.Failure(
                query = query,
                kind = SearchFailureKind.Server,
                message = response.message,
                retryable = true,
            )
            ApiResult.EmptyBody -> MediaSearchOutcome.Failure(
                query = query,
                kind = SearchFailureKind.InvalidResponse,
                message = "Server returned an empty search response",
                retryable = true,
            )
            is ApiResult.ParseError -> MediaSearchOutcome.Failure(
                query = query,
                kind = SearchFailureKind.InvalidResponse,
                message = response.messageOrFallback("Search response was not understood"),
                retryable = true,
            )
        }
    }

    private suspend fun resolveHits(
        scope: ServerCacheScope,
        query: String,
        hits: List<SearchMediaWithStatus>,
    ): MediaSearchOutcome {
        if (hits.isEmpty()) return MediaSearchOutcome.NoResults(query)

        var resolved = hits.associateWith { hit -> cache.resolve(scope, hit.id) }
        val missingGroups = resolved.filterValues { it == null }
            .keys
            .map { it.id.resyncGroup() }
            .distinct()
        val resyncSummaries = mutableMapOf<CachedMediaType, CachedMediaResyncSummary>()

        for (group in missingGroups) {
            val representative = resolved.keys.firstOrNull { it.id.resyncGroup() == group } ?: continue
            resyncSummaries[group] = cache.resync(scope, representative.id)
        }

        if (resyncSummaries.isNotEmpty()) {
            resolved = hits.associateWith { hit -> cache.resolve(scope, hit.id) }
        }

        val rows = hits.map { hit ->
            resolved[hit]?.let { reference -> reference.toRow(hit.id) }
                ?: hit.id.toCacheMiss(resyncSummaries[hit.id.resyncGroup()])
        }

        return MediaSearchOutcome.Results(
            query = query,
            rows = rows,
            staleCache = cache.freshness(scope).isStaleForSearch,
        )
    }

    private fun SearchMediaId.resyncGroup(): CachedMediaType = when (type) {
        SearchMediaType.Movie -> CachedMediaType.Movie
        SearchMediaType.Series,
        SearchMediaType.Season,
        SearchMediaType.Episode -> CachedMediaType.Series
    }

    private fun CachedMediaReference.toRow(sourceId: SearchMediaId): SearchResultRow.Resolved = when (this) {
        is CachedMediaReference.Movie -> SearchResultRow.Resolved(
            sourceId = sourceId,
            title = title,
            subtitle = "Movie • Library $libraryId",
            libraryId = libraryId,
            imageKey = imageKey,
            publicFallbackPath = publicFallbackPath,
            target = SearchDetailTarget(SearchMediaType.Movie, id, libraryId),
        )
        is CachedMediaReference.Series -> SearchResultRow.Resolved(
            sourceId = sourceId,
            title = title,
            subtitle = "Series • Library $libraryId",
            libraryId = libraryId,
            imageKey = imageKey,
            publicFallbackPath = publicFallbackPath,
            target = SearchDetailTarget(SearchMediaType.Series, id, libraryId),
        )
        is CachedMediaReference.Season -> SearchResultRow.Resolved(
            sourceId = sourceId,
            title = title,
            subtitle = "Season $seasonNumber • Opens series detail",
            libraryId = libraryId,
            imageKey = imageKey,
            publicFallbackPath = publicFallbackPath,
            target = SearchDetailTarget(SearchMediaType.Series, seriesId, libraryId),
        )
        is CachedMediaReference.Episode -> SearchResultRow.Resolved(
            sourceId = sourceId,
            title = title,
            subtitle = "S$seasonNumber E$episodeNumber • Opens series detail",
            libraryId = libraryId,
            imageKey = imageKey,
            publicFallbackPath = publicFallbackPath,
            target = SearchDetailTarget(SearchMediaType.Series, seriesId, libraryId),
        )
    }

    private fun SearchMediaId.toCacheMiss(summary: CachedMediaResyncSummary?): SearchResultRow.CacheMiss {
        val typeLabel = type.routeSegment.replaceFirstChar { it.uppercase() }
        val libraryKind = if (type == SearchMediaType.Movie) "movie" else "series"
        val attempted = summary?.attemptedLibraryIds.orEmpty()
        val resyncCopy = when {
            summary == null -> "No accessible $libraryKind libraries were available for bounded resync."
            attempted.isEmpty() -> "Bounded resync found no accessible $libraryKind library to refresh."
            summary.bounded -> "Retried ${attempted.size} matching $libraryKind library cache(s); more libraries are available, so retry remains bounded."
            else -> "Retried ${attempted.size} matching $libraryKind library cache(s)."
        }
        return SearchResultRow.CacheMiss(
            sourceId = this,
            title = "$typeLabel unavailable in cache",
            message = "$resyncCopy Search results are kept visible so this miss can be repaired instead of silently dropped.",
            retryable = true,
            attemptedLibraryIds = attempted,
        )
    }

    private val LibraryFreshness.isStaleForSearch: Boolean
        get() = this is LibraryFreshness.StaleOffline || this is LibraryFreshness.CorruptRebuilding || this is LibraryFreshness.ErrorRetryable
}

fun SearchMediaId.toCachedLookupKey(): CachedMediaLookupKey = CachedMediaLookupKey(
    type = when (type) {
        SearchMediaType.Movie -> CachedMediaType.Movie
        SearchMediaType.Series -> CachedMediaType.Series
        SearchMediaType.Season -> CachedMediaType.Season
        SearchMediaType.Episode -> CachedMediaType.Episode
    },
    id = id,
)

sealed interface MediaSearchOutcome {
    data object Idle : MediaSearchOutcome
    data class NoResults(val query: String) : MediaSearchOutcome
    data class Results(
        val query: String,
        val rows: List<SearchResultRow>,
        val staleCache: Boolean,
    ) : MediaSearchOutcome
    data class Failure(
        val query: String,
        val kind: SearchFailureKind,
        val message: String,
        val retryable: Boolean,
    ) : MediaSearchOutcome
}

enum class SearchFailureKind {
    Http,
    NetworkOffline,
    Server,
    InvalidResponse,
}

sealed interface SearchResultRow {
    val sourceId: SearchMediaId

    data class Resolved(
        override val sourceId: SearchMediaId,
        val title: String,
        val subtitle: String,
        val libraryId: String,
        val imageKey: ImageRequestKey?,
        val publicFallbackPath: String?,
        val target: SearchDetailTarget,
    ) : SearchResultRow

    data class CacheMiss(
        override val sourceId: SearchMediaId,
        val title: String,
        val message: String,
        val retryable: Boolean,
        val attemptedLibraryIds: List<String>,
    ) : SearchResultRow
}

data class SearchDetailTarget(
    val mediaType: SearchMediaType,
    val mediaId: String,
    val libraryId: String?,
)
