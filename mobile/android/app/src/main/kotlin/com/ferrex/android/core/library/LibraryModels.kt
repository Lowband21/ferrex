package com.ferrex.android.core.library

import ferrex.common.LibraryType

data class LibraryInfo(
    val id: String,
    val name: String,
    val kind: LibraryKind,
)

enum class LibraryKind {
    Movies,
    Series,
    Unknown,
}

fun Byte.toLibraryKind(): LibraryKind = when (this) {
    LibraryType.Movies -> LibraryKind.Movies
    LibraryType.Series -> LibraryKind.Series
    else -> LibraryKind.Unknown
}

data class LibraryRepositoryState(
    val scope: ServerCacheScope? = null,
    val libraries: List<LibraryInfo> = emptyList(),
    val selectedLibraryId: String? = null,
    val movieAccessor: MovieLibraryAccessor? = null,
    val seriesAccessor: SeriesLibraryAccessor? = null,
    val movieLibraries: List<CachedMovieLibrary> = emptyList(),
    val seriesLibraries: List<CachedSeriesLibrary> = emptyList(),
    val freshness: LibraryFreshness = LibraryFreshness.Empty,
)

data class CachedMovieLibrary(
    val library: LibraryInfo,
    val accessor: MovieLibraryAccessor,
)

data class CachedSeriesLibrary(
    val library: LibraryInfo,
    val accessor: SeriesLibraryAccessor,
)


enum class CachedMediaType {
    Movie,
    Series,
    Season,
    Episode,
}

data class CachedMediaLookupKey(
    val type: CachedMediaType,
    val id: String,
)

sealed interface CachedMediaReference {
    val id: String
    val libraryId: String
    val title: String
    val imageKey: com.ferrex.android.core.image.ImageRequestKey?
    val publicFallbackPath: String?

    data class Movie(
        override val id: String,
        override val libraryId: String,
        override val title: String,
        override val imageKey: com.ferrex.android.core.image.ImageRequestKey?,
        override val publicFallbackPath: String?,
    ) : CachedMediaReference

    data class Series(
        override val id: String,
        override val libraryId: String,
        override val title: String,
        override val imageKey: com.ferrex.android.core.image.ImageRequestKey?,
        override val publicFallbackPath: String?,
    ) : CachedMediaReference

    data class Season(
        override val id: String,
        override val libraryId: String,
        override val title: String,
        override val imageKey: com.ferrex.android.core.image.ImageRequestKey?,
        override val publicFallbackPath: String?,
        val seriesId: String,
        val seasonNumber: Int,
    ) : CachedMediaReference

    data class Episode(
        override val id: String,
        override val libraryId: String,
        override val title: String,
        override val imageKey: com.ferrex.android.core.image.ImageRequestKey?,
        override val publicFallbackPath: String?,
        val seriesId: String,
        val seasonId: String,
        val seasonNumber: Int,
        val episodeNumber: Int,
    ) : CachedMediaReference
}

data class CachedMediaResyncSummary(
    val attemptedLibraryIds: List<String>,
    val bounded: Boolean,
)

data class LibraryCacheHealthCounts(
    val cachedItems: Int? = null,
    val expectedItems: Int? = null,
    val pendingItems: Int? = null,
    val failedItems: Int? = null,
    val cachedSeriesBundles: Int? = null,
    val expectedSeriesBundles: Int? = null,
    val pendingSeriesBundles: Int? = null,
    val failedSeriesBundles: Int? = null,
    val quarantinedPayloads: Int? = null,
)

sealed interface LibraryFreshness {
    val label: String

    data object Empty : LibraryFreshness {
        override val label: String = "empty"
    }

    data object Syncing : LibraryFreshness {
        override val label: String = "syncing"
    }

    data class Fresh(
        val itemCount: Int,
        val syncedAtMillis: Long,
    ) : LibraryFreshness {
        override val label: String = "fresh"
    }

    data class StaleOffline(
        val message: String,
        val itemCount: Int,
        val lastSyncedAtMillis: Long?,
    ) : LibraryFreshness {
        override val label: String = "stale-offline"
    }

    data class SeriesCacheIncomplete(
        val message: String,
        val completedBundles: Int,
        val expectedBundles: Int,
        val remainingBundleIds: List<String>,
        val itemCount: Int,
        val classification: RetryClassification,
        val failedBundleCount: Int = 0,
    ) : LibraryFreshness {
        override val label: String = "series-cache-incomplete"
    }

    data class CorruptRebuilding(
        val message: String,
        val quarantinedFiles: Int,
    ) : LibraryFreshness {
        override val label: String = "corrupt-rebuilding"
    }

    data class ErrorRetryable(
        val message: String,
        val classification: RetryClassification,
    ) : LibraryFreshness {
        override val label: String = "error-retryable"
    }
}

enum class RetryClassification {
    Retryable,
    AuthRequired,
    NotFound,
    NotRetryable,
    InvalidResponse,
}

fun LibraryFreshness.cacheHealthCounts(): LibraryCacheHealthCounts = when (this) {
    LibraryFreshness.Empty -> LibraryCacheHealthCounts(cachedItems = 0)
    LibraryFreshness.Syncing -> LibraryCacheHealthCounts()
    is LibraryFreshness.Fresh -> LibraryCacheHealthCounts(
        cachedItems = itemCount,
        expectedItems = itemCount,
        pendingItems = 0,
        failedItems = 0,
    )
    is LibraryFreshness.StaleOffline -> LibraryCacheHealthCounts(cachedItems = itemCount)
    is LibraryFreshness.SeriesCacheIncomplete -> LibraryCacheHealthCounts(
        cachedItems = itemCount,
        cachedSeriesBundles = completedBundles,
        expectedSeriesBundles = expectedBundles,
        pendingSeriesBundles = remainingBundleIds.size,
        failedSeriesBundles = failedBundleCount.coerceAtLeast(0),
    )
    is LibraryFreshness.CorruptRebuilding -> LibraryCacheHealthCounts(
        failedItems = quarantinedFiles,
        quarantinedPayloads = quarantinedFiles,
    )
    is LibraryFreshness.ErrorRetryable -> LibraryCacheHealthCounts(failedItems = 1)
}

fun LibraryFreshness.cacheHealthSummary(): String = when (this) {
    LibraryFreshness.Empty -> "Cached 0 item(s); expected, pending, and failed counts are unknown until sync completes."
    LibraryFreshness.Syncing -> "Syncing selected library cache; cached, expected, pending, and failed counts are updating."
    is LibraryFreshness.Fresh -> "Cached $itemCount/$itemCount expected item(s); pending 0; failed 0."
    is LibraryFreshness.StaleOffline -> "Cached $itemCount item(s); expected, pending, and failed counts are unknown while offline/stale."
    is LibraryFreshness.SeriesCacheIncomplete -> "Cached $itemCount item(s); series bundles cached $completedBundles/$expectedBundles expected; pending ${remainingBundleIds.size}; failed ${failedBundleCount.coerceAtLeast(0)}."
    is LibraryFreshness.CorruptRebuilding -> "Quarantined $quarantinedFiles corrupt payload(s); cached and expected counts may be partial until Retry rebuilds."
    is LibraryFreshness.ErrorRetryable -> "Sync request failed (${classification.name}); cached, expected, and pending counts are unavailable."
}

sealed interface LibrarySyncFailure {
    val message: String
    val classification: RetryClassification

    data class Network(override val message: String) : LibrarySyncFailure {
        override val classification: RetryClassification = RetryClassification.Retryable
    }

    data class Http(val code: Int, override val message: String) : LibrarySyncFailure {
        override val classification: RetryClassification = when (code) {
            401, 403 -> RetryClassification.AuthRequired
            404 -> RetryClassification.NotFound
            408, 409, 425, 429 -> RetryClassification.Retryable
            in 500..599 -> RetryClassification.Retryable
            else -> RetryClassification.NotRetryable
        }
    }

    data class Parse(override val message: String) : LibrarySyncFailure {
        override val classification: RetryClassification = RetryClassification.InvalidResponse
    }

    data object EmptyBody : LibrarySyncFailure {
        override val message: String = "Server returned an empty FlatBuffers response"
        override val classification: RetryClassification = RetryClassification.InvalidResponse
    }
}

sealed interface LibrarySyncResult<out T> {
    data class Success<T>(val value: T) : LibrarySyncResult<T>
    data class Failure(val error: LibrarySyncFailure) : LibrarySyncResult<Nothing>
}

data class MovieBatchSyncPlan(
    val staleBatchIds: List<Int>,
    val deletedBatchIds: List<Int>,
    val serverVersions: Map<Int, Long>,
)

data class SeriesBundleSyncPlan(
    val staleSeriesIds: List<String>,
    val deletedSeriesIds: List<String>,
    val serverVersions: Map<String, Long>,
)
