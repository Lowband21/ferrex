package com.ferrex.android.core.library

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

/**
 * Server-scoped library repository foundation for Android/TV browse/search/detail.
 *
 * Movie libraries use `/api/v1/libraries/{id}/movie-batches:sync` followed by
 * per-batch `/movie-batches:fetch` requests. Series libraries use
 * `/api/v1/libraries/{id}/series-bundles:sync` followed by per-series
 * `/series-bundles:fetch` requests. The current server only exposes complete
 * series bundles, not a compact series-list endpoint, so series sync is heavier
 * by design until that endpoint exists.
 */
class LibraryRepository(
    private val transport: LibrarySyncTransport,
    private val cache: LibraryDiskCache,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) {
    private val _state = MutableStateFlow(LibraryRepositoryState())
    val state: StateFlow<LibraryRepositoryState> = _state.asStateFlow()

    suspend fun refreshLibraries(scope: ServerCacheScope, selectedLibraryId: String? = null): LibraryRepositoryState =
        withContext(ioDispatcher) {
            publish(
                _state.value.copy(
                    scope = scope,
                    freshness = LibraryFreshness.Syncing,
                ),
            )

            val libraries = when (val loaded = loadLibraries(scope)) {
                is LibraryLoad.Online -> loaded.libraries
                is LibraryLoad.Cached -> {
                    val state = _state.value.copy(
                        scope = scope,
                        libraries = loaded.libraries,
                        freshness = LibraryFreshness.StaleOffline(
                            message = loaded.failure.message,
                            itemCount = 0,
                            lastSyncedAtMillis = null,
                        ),
                    )
                    cache.markStaleOffline(scope, null, loaded.failure.message)
                    publish(state)
                    loaded.libraries
                }
                is LibraryLoad.Corrupt -> return@withContext publish(
                    _state.value.copy(
                        scope = scope,
                        freshness = LibraryFreshness.CorruptRebuilding(
                            message = loaded.message,
                            quarantinedFiles = loaded.quarantinedFiles,
                        ),
                    ),
                )
                is LibraryLoad.Failed -> return@withContext publish(
                    _state.value.copy(
                        scope = scope,
                        freshness = LibraryFreshness.ErrorRetryable(
                            message = loaded.failure.message,
                            classification = loaded.failure.classification,
                        ),
                    ),
                )
            }

            if (libraries.isEmpty()) {
                return@withContext publish(
                    _state.value.copy(
                        scope = scope,
                        libraries = emptyList(),
                        selectedLibraryId = null,
                        movieAccessor = null,
                        seriesAccessor = null,
                        freshness = LibraryFreshness.Empty,
                    ),
                )
            }

            val selected = libraries.firstOrNull { it.id == selectedLibraryId }
                ?: libraries.firstOrNull { it.id == _state.value.selectedLibraryId }
                ?: libraries.first()

            when (selected.kind) {
                LibraryKind.Movies -> syncMovieLibrary(scope, selected, libraries)
                LibraryKind.Series -> syncSeriesLibrary(scope, selected, libraries)
                LibraryKind.Unknown -> publish(
                    _state.value.copy(
                        scope = scope,
                        libraries = libraries,
                        selectedLibraryId = selected.id,
                        movieAccessor = null,
                        seriesAccessor = null,
                        freshness = LibraryFreshness.Empty,
                    ),
                )
            }
        }

    suspend fun syncMovieLibrary(
        scope: ServerCacheScope,
        library: LibraryInfo,
        knownLibraries: List<LibraryInfo> = _state.value.libraries,
    ): LibraryRepositoryState = withContext(ioDispatcher) {
        publish(
            _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = null,
                seriesAccessor = null,
                freshness = LibraryFreshness.Syncing,
            ),
        )

        val cachedVersions = cache.cachedMovieBatchVersions(scope, library.id)
        when (val sync = transport.syncMovieBatches(library.id, cachedVersions)) {
            is LibrarySyncResult.Failure -> return@withContext publish(cachedMovieState(scope, library, sync.error, knownLibraries))
            is LibrarySyncResult.Success -> {
                cache.deleteMovieBatches(scope, library.id, sync.value.deletedBatchIds)
                for (batchId in sync.value.staleBatchIds.distinct().sorted()) {
                    when (val fetch = transport.fetchMovieBatch(library.id, batchId)) {
                        is LibrarySyncResult.Failure -> return@withContext publish(cachedMovieState(scope, library, fetch.error, knownLibraries))
                        is LibrarySyncResult.Success -> {
                            val parsed = LibraryFlatBuffers.parseMoviePayload(fetch.value.wrapFlatBuffer(), expectedBatchId = batchId)
                            if (parsed.isFailure) {
                                return@withContext publish(
                                    errorState(scope, library, knownLibraries, LibrarySyncFailure.Parse(parsed.exceptionOrNull()?.message ?: "Invalid movie batch payload")),
                                )
                            }
                            val version = sync.value.serverVersions[batchId]
                                ?: parsed.getOrThrow().firstOrNull { it.batchId == batchId }?.version
                                ?: 0L
                            cache.writeMovieBatch(scope, library.id, batchId, version, fetch.value)
                        }
                    }
                }
                publish(movieFreshState(scope, library, knownLibraries))
            }
        }
    }

    suspend fun syncSeriesLibrary(
        scope: ServerCacheScope,
        library: LibraryInfo,
        knownLibraries: List<LibraryInfo> = _state.value.libraries,
    ): LibraryRepositoryState = withContext(ioDispatcher) {
        publish(
            _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = null,
                seriesAccessor = null,
                freshness = LibraryFreshness.Syncing,
            ),
        )

        val cachedVersions = cache.cachedSeriesBundleVersions(scope, library.id)
        when (val sync = transport.syncSeriesBundles(library.id, cachedVersions)) {
            is LibrarySyncResult.Failure -> return@withContext publish(cachedSeriesState(scope, library, sync.error, knownLibraries))
            is LibrarySyncResult.Success -> {
                cache.deleteSeriesBundles(scope, library.id, sync.value.deletedSeriesIds)
                for (seriesId in sync.value.staleSeriesIds.distinct().sorted()) {
                    when (val fetch = transport.fetchSeriesBundle(library.id, seriesId)) {
                        is LibrarySyncResult.Failure -> return@withContext publish(cachedSeriesState(scope, library, fetch.error, knownLibraries))
                        is LibrarySyncResult.Success -> {
                            val parsed = LibraryFlatBuffers.parseSeriesPayload(fetch.value.wrapFlatBuffer(), expectedSeriesId = seriesId)
                            if (parsed.isFailure) {
                                return@withContext publish(
                                    errorState(scope, library, knownLibraries, LibrarySyncFailure.Parse(parsed.exceptionOrNull()?.message ?: "Invalid series bundle payload")),
                                )
                            }
                            val version = sync.value.serverVersions[seriesId]
                                ?: parsed.getOrThrow().firstOrNull { it.seriesId == seriesId }?.version
                                ?: 0L
                            cache.writeSeriesBundle(scope, library.id, seriesId, version, fetch.value)
                        }
                    }
                }
                publish(seriesFreshState(scope, library, knownLibraries))
            }
        }
    }

    fun clearSelectedCache(scope: ServerCacheScope, libraryId: String) {
        cache.clearSelectedLibrary(scope, libraryId)
        if (_state.value.scope?.directoryName == scope.directoryName && _state.value.selectedLibraryId == libraryId) {
            publish(
                _state.value.copy(
                    movieAccessor = null,
                    seriesAccessor = null,
                    freshness = LibraryFreshness.Empty,
                ),
            )
        }
    }

    fun clearAllCache(scope: ServerCacheScope) {
        cache.clearAllForScope(scope)
        if (_state.value.scope?.directoryName == scope.directoryName) {
            publish(
                LibraryRepositoryState(
                    scope = scope,
                    freshness = LibraryFreshness.Empty,
                ),
            )
        }
    }

    private fun movieFreshState(scope: ServerCacheScope, library: LibraryInfo, knownLibraries: List<LibraryInfo>): LibraryRepositoryState {
        return when (val load = loadCachedMovieAccessor(scope, library.id)) {
            is CacheLoad.Success -> _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = load.accessor,
                seriesAccessor = null,
                freshness = if (load.accessor.itemCount == 0) {
                    LibraryFreshness.Empty
                } else {
                    LibraryFreshness.Fresh(load.accessor.itemCount, System.currentTimeMillis())
                },
            )
            is CacheLoad.Empty -> _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = null,
                seriesAccessor = null,
                freshness = LibraryFreshness.Empty,
            )
            is CacheLoad.Corrupt -> _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = load.accessor,
                seriesAccessor = null,
                freshness = LibraryFreshness.CorruptRebuilding(load.message, load.quarantinedFiles),
            )
        }
    }

    private fun seriesFreshState(scope: ServerCacheScope, library: LibraryInfo, knownLibraries: List<LibraryInfo>): LibraryRepositoryState {
        return when (val load = loadCachedSeriesAccessor(scope, library.id)) {
            is CacheLoad.Success -> _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = null,
                seriesAccessor = load.accessor,
                freshness = if (load.accessor.itemCount == 0) {
                    LibraryFreshness.Empty
                } else {
                    LibraryFreshness.Fresh(load.accessor.itemCount, System.currentTimeMillis())
                },
            )
            is CacheLoad.Empty -> _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = null,
                seriesAccessor = null,
                freshness = LibraryFreshness.Empty,
            )
            is CacheLoad.Corrupt -> _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = null,
                seriesAccessor = load.accessor,
                freshness = LibraryFreshness.CorruptRebuilding(load.message, load.quarantinedFiles),
            )
        }
    }

    private fun cachedMovieState(
        scope: ServerCacheScope,
        library: LibraryInfo,
        failure: LibrarySyncFailure,
        knownLibraries: List<LibraryInfo>,
    ): LibraryRepositoryState {
        return when (val load = loadCachedMovieAccessor(scope, library.id)) {
            is CacheLoad.Success -> {
                cache.markStaleOffline(scope, library.id, failure.message)
                _state.value.copy(
                    scope = scope,
                    libraries = knownLibraries.ifEmpty { listOf(library) },
                    selectedLibraryId = library.id,
                    movieAccessor = load.accessor,
                    seriesAccessor = null,
                    freshness = LibraryFreshness.StaleOffline(failure.message, load.accessor.itemCount, null),
                )
            }
            is CacheLoad.Empty -> errorState(scope, library, knownLibraries, failure)
            is CacheLoad.Corrupt -> _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = load.accessor,
                seriesAccessor = null,
                freshness = LibraryFreshness.CorruptRebuilding(load.message, load.quarantinedFiles),
            )
        }
    }

    private fun cachedSeriesState(
        scope: ServerCacheScope,
        library: LibraryInfo,
        failure: LibrarySyncFailure,
        knownLibraries: List<LibraryInfo>,
    ): LibraryRepositoryState {
        return when (val load = loadCachedSeriesAccessor(scope, library.id)) {
            is CacheLoad.Success -> {
                cache.markStaleOffline(scope, library.id, failure.message)
                _state.value.copy(
                    scope = scope,
                    libraries = knownLibraries.ifEmpty { listOf(library) },
                    selectedLibraryId = library.id,
                    movieAccessor = null,
                    seriesAccessor = load.accessor,
                    freshness = LibraryFreshness.StaleOffline(failure.message, load.accessor.itemCount, null),
                )
            }
            is CacheLoad.Empty -> errorState(scope, library, knownLibraries, failure)
            is CacheLoad.Corrupt -> _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = null,
                seriesAccessor = load.accessor,
                freshness = LibraryFreshness.CorruptRebuilding(load.message, load.quarantinedFiles),
            )
        }
    }

    private fun errorState(
        scope: ServerCacheScope,
        library: LibraryInfo,
        knownLibraries: List<LibraryInfo>,
        failure: LibrarySyncFailure,
    ): LibraryRepositoryState = _state.value.copy(
        scope = scope,
        libraries = knownLibraries.ifEmpty { listOf(library) },
        selectedLibraryId = library.id,
        movieAccessor = null,
        seriesAccessor = null,
        freshness = LibraryFreshness.ErrorRetryable(failure.message, failure.classification),
    )

    private fun loadCachedMovieAccessor(scope: ServerCacheScope, libraryId: String): CacheLoad<MovieLibraryAccessor> {
        val batches = mutableListOf<ParsedMovieBatch>()
        var quarantined = 0
        for (payload in cache.readMovieBatchPayloads(scope, libraryId)) {
            val parsed = LibraryFlatBuffers.parseMoviePayload(payload.bytes, expectedBatchId = payload.id)
            if (parsed.isSuccess) {
                batches += parsed.getOrThrow()
            } else {
                quarantined += 1
                cache.quarantineMovieBatch(scope, libraryId, payload.id, parsed.exceptionOrNull()?.message ?: "Unparseable movie batch")
            }
        }
        if (quarantined > 0) {
            val accessor = batches.takeIf { it.isNotEmpty() }?.let(::MovieLibraryAccessor)
            return CacheLoad.Corrupt("$quarantined cached movie batch file(s) were quarantined and can be rebuilt with Retry.", quarantined, accessor)
        }
        if (batches.isEmpty()) return CacheLoad.Empty
        return CacheLoad.Success(MovieLibraryAccessor(batches))
    }

    private fun loadCachedSeriesAccessor(scope: ServerCacheScope, libraryId: String): CacheLoad<SeriesLibraryAccessor> {
        val bundles = mutableListOf<ParsedSeriesBundle>()
        var quarantined = 0
        for (payload in cache.readSeriesBundlePayloads(scope, libraryId)) {
            val parsed = LibraryFlatBuffers.parseSeriesPayload(payload.bytes, expectedSeriesId = payload.id)
            if (parsed.isSuccess) {
                bundles += parsed.getOrThrow()
            } else {
                quarantined += 1
                cache.quarantineSeriesBundle(scope, libraryId, payload.id, parsed.exceptionOrNull()?.message ?: "Unparseable series bundle")
            }
        }
        if (quarantined > 0) {
            val accessor = bundles.takeIf { it.isNotEmpty() }?.let(::SeriesLibraryAccessor)
            return CacheLoad.Corrupt("$quarantined cached series bundle file(s) were quarantined and can be rebuilt with Retry.", quarantined, accessor)
        }
        if (bundles.isEmpty()) return CacheLoad.Empty
        return CacheLoad.Success(SeriesLibraryAccessor(bundles))
    }

    private suspend fun loadLibraries(scope: ServerCacheScope): LibraryLoad {
        return when (val result = transport.fetchLibraries()) {
            is LibrarySyncResult.Success -> runCatching {
                val libraries = LibraryFlatBuffers.parseLibraryList(result.value)
                cache.writeLibraryList(scope, result.value)
                LibraryLoad.Online(libraries)
            }.getOrElse { error ->
                LibraryLoad.Failed(LibrarySyncFailure.Parse(error.message ?: "Invalid library list"))
            }
            is LibrarySyncResult.Failure -> loadCachedLibraries(scope, result.error)
        }
    }

    private fun loadCachedLibraries(scope: ServerCacheScope, failure: LibrarySyncFailure): LibraryLoad {
        val payload = runCatching { cache.readLibraryList(scope) }.getOrNull()
            ?: return LibraryLoad.Failed(failure)
        return runCatching {
            LibraryLoad.Cached(LibraryFlatBuffers.parseLibraryList(payload.bytes), failure)
        }.getOrElse { error ->
            val quarantined = if (cache.quarantineLibraryList(scope, error.message ?: "Invalid cached library list") != null) 1 else 0
            LibraryLoad.Corrupt("Cached library metadata was quarantined and can be rebuilt with Retry.", quarantined)
        }
    }

    private fun publish(state: LibraryRepositoryState): LibraryRepositoryState {
        _state.value = state
        return state
    }

    private fun ByteArray.wrapFlatBuffer() = java.nio.ByteBuffer.wrap(this).order(java.nio.ByteOrder.LITTLE_ENDIAN)

    private sealed interface LibraryLoad {
        data class Online(val libraries: List<LibraryInfo>) : LibraryLoad
        data class Cached(val libraries: List<LibraryInfo>, val failure: LibrarySyncFailure) : LibraryLoad
        data class Failed(val failure: LibrarySyncFailure) : LibraryLoad
        data class Corrupt(val message: String, val quarantinedFiles: Int) : LibraryLoad
    }

    private sealed interface CacheLoad<out T> {
        data class Success<T>(val accessor: T) : CacheLoad<T>
        data object Empty : CacheLoad<Nothing>
        data class Corrupt<T>(val message: String, val quarantinedFiles: Int, val accessor: T?) : CacheLoad<T>
    }
}
