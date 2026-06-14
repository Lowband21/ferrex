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
    val freshness: LibraryFreshness = LibraryFreshness.Empty,
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
