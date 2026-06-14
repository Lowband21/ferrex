package com.ferrex.android.core.library

import com.ferrex.android.core.image.ImageCacheClearer
import com.ferrex.android.core.image.ImageRequestKey
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
    private val imageCacheClearer: ImageCacheClearer? = null,
) {
    private val _state = MutableStateFlow(LibraryRepositoryState())
    val state: StateFlow<LibraryRepositoryState> = _state.asStateFlow()

    fun searchFreshness(scope: ServerCacheScope): LibraryFreshness =
        if (_state.value.scope?.directoryName == scope.directoryName) _state.value.freshness else LibraryFreshness.Empty

    fun resolveCachedMedia(scope: ServerCacheScope, key: CachedMediaLookupKey): CachedMediaReference? {
        resolveCachedMediaFromState(scope, key)?.let { return it }
        val libraries = knownLibrariesForSearch(scope).filter { it.supports(key) }
        for (library in libraries) {
            val resolved = when (key.type) {
                CachedMediaType.Movie -> movieAccessorForSearch(scope, library.id)?.findMovie(key.id)
                CachedMediaType.Series -> seriesAccessorForSearch(scope, library.id)?.findSeries(key.id)
                CachedMediaType.Season -> seriesAccessorForSearch(scope, library.id)?.findSeason(key.id)
                CachedMediaType.Episode -> seriesAccessorForSearch(scope, library.id)?.findEpisode(key.id)
            }
            if (resolved != null) return resolved
        }
        return null
    }

    suspend fun resyncCachedMediaForSearch(
        scope: ServerCacheScope,
        key: CachedMediaLookupKey,
        maxLibraries: Int = DEFAULT_SEARCH_RESYNC_LIBRARY_LIMIT,
    ): CachedMediaResyncSummary = withContext(ioDispatcher) {
        var libraries = knownLibrariesForSearch(scope)
        if (libraries.isEmpty()) {
            refreshLibraries(scope)
            libraries = knownLibrariesForSearch(scope)
        }
        val matchingLibraries = libraries.filter { it.supports(key) }
        val candidates = matchingLibraries.take(maxLibraries.coerceAtLeast(0))
        candidates.forEach { library ->
            when (library.kind) {
                LibraryKind.Movies -> syncMovieLibrary(scope, library, libraries)
                LibraryKind.Series -> syncSeriesLibrary(scope, library, libraries)
                LibraryKind.Unknown -> Unit
            }
        }
        CachedMediaResyncSummary(
            attemptedLibraryIds = candidates.map { it.id },
            bounded = candidates.size < matchingLibraries.size,
        )
    }

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
        val imageKeys = selectedImageKeys(libraryId)
        cache.clearSelectedLibrary(scope, libraryId)
        imageCacheClearer?.clearSelectedImages(scope, imageKeys)
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
        imageCacheClearer?.clearAllImages(scope)
        if (_state.value.scope?.directoryName == scope.directoryName) {
            publish(
                LibraryRepositoryState(
                    scope = scope,
                    freshness = LibraryFreshness.Empty,
                ),
            )
        }
    }

    private fun selectedImageKeys(libraryId: String): Set<ImageRequestKey> {
        val state = _state.value
        if (state.selectedLibraryId != libraryId) return emptySet()
        return buildSet {
            state.movieAccessor?.primaryImageKeys()?.let(::addAll)
            state.seriesAccessor?.primaryImageKeys()?.let(::addAll)
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

    private fun resolveCachedMediaFromState(scope: ServerCacheScope, key: CachedMediaLookupKey): CachedMediaReference? {
        val state = _state.value
        if (state.scope?.directoryName != scope.directoryName) return null
        return when (key.type) {
            CachedMediaType.Movie -> state.movieAccessor?.findMovie(key.id)
            CachedMediaType.Series -> state.seriesAccessor?.findSeries(key.id)
            CachedMediaType.Season -> state.seriesAccessor?.findSeason(key.id)
            CachedMediaType.Episode -> state.seriesAccessor?.findEpisode(key.id)
        }
    }

    private fun knownLibrariesForSearch(scope: ServerCacheScope): List<LibraryInfo> {
        val state = _state.value
        if (state.scope?.directoryName == scope.directoryName && state.libraries.isNotEmpty()) return state.libraries
        return runCatching {
            cache.readLibraryList(scope)?.let { LibraryFlatBuffers.parseLibraryList(it.bytes) }.orEmpty()
        }.getOrDefault(emptyList())
    }

    private fun movieAccessorForSearch(scope: ServerCacheScope, libraryId: String): MovieLibraryAccessor? =
        when (val load = runCatching { loadCachedMovieAccessor(scope, libraryId) }.getOrNull()) {
            is CacheLoad.Success -> load.accessor
            is CacheLoad.Corrupt -> load.accessor
            CacheLoad.Empty, null -> null
        }

    private fun seriesAccessorForSearch(scope: ServerCacheScope, libraryId: String): SeriesLibraryAccessor? =
        when (val load = runCatching { loadCachedSeriesAccessor(scope, libraryId) }.getOrNull()) {
            is CacheLoad.Success -> load.accessor
            is CacheLoad.Corrupt -> load.accessor
            CacheLoad.Empty, null -> null
        }

    private fun LibraryInfo.supports(key: CachedMediaLookupKey): Boolean = when (key.type) {
        CachedMediaType.Movie -> kind == LibraryKind.Movies
        CachedMediaType.Series,
        CachedMediaType.Season,
        CachedMediaType.Episode -> kind == LibraryKind.Series
    }

    private fun publish(state: LibraryRepositoryState): LibraryRepositoryState {
        val enriched = if (state.scope != null && state.freshness != LibraryFreshness.Syncing) {
            state.withCachedDatasets()
        } else {
            state
        }
        _state.value = enriched
        return enriched
    }

    private fun LibraryRepositoryState.withCachedDatasets(): LibraryRepositoryState {
        val scoped = scope ?: return copy(movieLibraries = emptyList(), seriesLibraries = emptyList())
        val cachedMovies = libraries.filter { it.kind == LibraryKind.Movies }.mapNotNull { library ->
            when (val load = loadCachedMovieAccessor(scoped, library.id)) {
                is CacheLoad.Success -> CachedMovieLibrary(library, load.accessor)
                is CacheLoad.Corrupt -> load.accessor?.let { CachedMovieLibrary(library, it) }
                CacheLoad.Empty -> null
            }
        }
        val cachedSeries = libraries.filter { it.kind == LibraryKind.Series }.mapNotNull { library ->
            when (val load = loadCachedSeriesAccessor(scoped, library.id)) {
                is CacheLoad.Success -> CachedSeriesLibrary(library, load.accessor)
                is CacheLoad.Corrupt -> load.accessor?.let { CachedSeriesLibrary(library, it) }
                CacheLoad.Empty -> null
            }
        }
        return copy(
            movieLibraries = cachedMovies,
            seriesLibraries = cachedSeries,
        )
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

    companion object {
        const val DEFAULT_SEARCH_RESYNC_LIBRARY_LIMIT = 4
    }
}
