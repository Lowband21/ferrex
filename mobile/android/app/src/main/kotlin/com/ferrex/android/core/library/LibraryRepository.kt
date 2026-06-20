package com.ferrex.android.core.library

import com.ferrex.android.core.image.ImageCacheClearer
import com.ferrex.android.core.image.ImageRequestKey
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

/**
 * Server-scoped library repository foundation for Android/TV browse/search/detail.
 *
 * Movie libraries use `/api/v1/libraries/{id}/movie-batches:sync` followed by
 * per-batch `/movie-batches:fetch` requests. Series libraries use
 * `/api/v1/libraries/{id}/series-bundles:sync` followed by bounded multi-ID
 * `/series-bundles:fetch` chunks. The current server only exposes complete
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

    private val libraryJobLock = Any()
    private val activeLibraryJobs = mutableMapOf<LibrarySyncJobKey, Deferred<LibraryRepositoryState>>()

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
    ): LibraryRepositoryState = coalescedLibraryJob(LibrarySyncJobKey(scope.directoryName, library.id, library.kind)) {
        syncMovieLibraryUncoalesced(scope, library, knownLibraries)
    }

    private suspend fun syncMovieLibraryUncoalesced(
        scope: ServerCacheScope,
        library: LibraryInfo,
        knownLibraries: List<LibraryInfo>,
    ): LibraryRepositoryState = withContext(ioDispatcher) {
        val cachedVersions = cache.cachedMovieBatchVersions(scope, library.id)
        val initialLoad = loadCachedMovieAccessor(scope, library.id)
        val existingAccessor = when (initialLoad) {
            is CacheLoad.Success -> initialLoad.accessor
            is CacheLoad.Corrupt -> initialLoad.accessor
            CacheLoad.Empty -> null
        }
        publish(
            _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = existingAccessor,
                seriesAccessor = null,
                freshness = LibraryFreshness.Syncing,
            ),
        )

        when (val sync = transport.syncMovieBatches(library.id, cachedVersions)) {
            is LibrarySyncResult.Failure -> return@withContext publish(
                if (initialLoad is CacheLoad.Corrupt) {
                    _state.value.copy(
                        scope = scope,
                        libraries = knownLibraries.ifEmpty { listOf(library) },
                        selectedLibraryId = library.id,
                        movieAccessor = initialLoad.accessor,
                        seriesAccessor = null,
                        freshness = LibraryFreshness.CorruptRebuilding(initialLoad.message, initialLoad.quarantinedFiles),
                    )
                } else {
                    cachedMovieState(scope, library, sync.error, knownLibraries)
                },
            )
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
    ): LibraryRepositoryState = coalescedLibraryJob(LibrarySyncJobKey(scope.directoryName, library.id, library.kind)) {
        syncSeriesLibraryUncoalesced(scope, library, knownLibraries)
    }

    private suspend fun syncSeriesLibraryUncoalesced(
        scope: ServerCacheScope,
        library: LibraryInfo,
        knownLibraries: List<LibraryInfo>,
    ): LibraryRepositoryState = withContext(ioDispatcher) {
        val cachedVersions = cache.cachedSeriesBundleVersions(scope, library.id)
        val initialLoad = loadCachedSeriesAccessor(scope, library.id)
        val existingAccessor = when (initialLoad) {
            is CacheLoad.Success -> initialLoad.accessor
            is CacheLoad.Corrupt -> initialLoad.accessor
            CacheLoad.Empty -> null
        }
        publish(
            _state.value.copy(
                scope = scope,
                libraries = knownLibraries.ifEmpty { listOf(library) },
                selectedLibraryId = library.id,
                movieAccessor = null,
                seriesAccessor = existingAccessor,
                freshness = LibraryFreshness.Syncing,
            ),
        )

        when (val sync = transport.syncSeriesBundles(library.id, cachedVersions)) {
            is LibrarySyncResult.Failure -> return@withContext publish(
                if (initialLoad is CacheLoad.Corrupt) {
                    _state.value.copy(
                        scope = scope,
                        libraries = knownLibraries.ifEmpty { listOf(library) },
                        selectedLibraryId = library.id,
                        movieAccessor = null,
                        seriesAccessor = initialLoad.accessor,
                        freshness = LibraryFreshness.CorruptRebuilding(initialLoad.message, initialLoad.quarantinedFiles),
                    )
                } else {
                    cachedSeriesState(scope, library, sync.error, knownLibraries)
                },
            )
            is LibrarySyncResult.Success -> {
                val plan = sync.value
                val staleIds = plan.staleSeriesIds.distinct().sorted()
                val staleIdSet = staleIds.toSet()
                val deletedIds = plan.deletedSeriesIds.distinct().sorted()
                val expectedIds = expectedSeriesBundleIds(
                    cachedIds = cachedVersions.keys,
                    deletedIds = deletedIds,
                    staleIds = staleIds,
                )
                cache.deleteSeriesBundles(scope, library.id, deletedIds)
                val fetchedSeriesIds = mutableSetOf<String>()

                var remainingIds = pendingSeriesBundleIds(
                    scope = scope,
                    libraryId = library.id,
                    expectedIds = expectedIds,
                    staleIds = staleIdSet,
                    serverVersions = plan.serverVersions,
                    fetchedIds = fetchedSeriesIds,
                )
                if (remainingIds.isNotEmpty()) {
                    publish(
                        seriesIncompleteState(
                            scope = scope,
                            library = library,
                            knownLibraries = knownLibraries,
                            expectedIds = expectedIds,
                            remainingIds = remainingIds,
                            message = "Series cache sync is incomplete.",
                            classification = RetryClassification.Retryable,
                        ),
                    )
                }

                for (chunk in remainingIds.chunked(SERIES_BUNDLE_FETCH_CHUNK_SIZE)) {
                    when (val fetch = transport.fetchSeriesBundles(library.id, chunk)) {
                        is LibrarySyncResult.Failure -> return@withContext publish(
                            seriesIncompleteState(
                                scope = scope,
                                library = library,
                                knownLibraries = knownLibraries,
                                expectedIds = expectedIds,
                                remainingIds = remainingIds,
                                message = fetch.error.message,
                                classification = fetch.error.classification,
                                failedBundleCount = chunk.size,
                            ),
                        )
                        is LibrarySyncResult.Success -> {
                            val parsed = validateFetchedSeriesBundles(fetch.value, chunk)
                            if (parsed.isFailure) {
                                val failure = LibrarySyncFailure.Parse(parsed.exceptionOrNull()?.message ?: "Invalid series bundle payload")
                                return@withContext publish(
                                    seriesIncompleteState(
                                        scope = scope,
                                        library = library,
                                        knownLibraries = knownLibraries,
                                        expectedIds = expectedIds,
                                        remainingIds = remainingIds,
                                        message = failure.message,
                                        classification = failure.classification,
                                        failedBundleCount = chunk.size,
                                    ),
                                )
                            }
                            val bundles = parsed.getOrThrow()
                            for (bundle in bundles) {
                                val version = plan.serverVersions[bundle.seriesId] ?: bundle.version
                                cache.writeSeriesBundle(scope, library.id, bundle.seriesId, version, fetch.value)
                            }
                            val fetchedIds = bundles.map { it.seriesId }
                            fetchedSeriesIds += fetchedIds
                            remainingIds = (remainingIds - fetchedIds.toSet()).sorted()
                        }
                    }
                    if (remainingIds.isNotEmpty()) {
                        publish(
                            seriesIncompleteState(
                                scope = scope,
                                library = library,
                                knownLibraries = knownLibraries,
                                expectedIds = expectedIds,
                                remainingIds = remainingIds,
                                message = "Series cache sync is incomplete.",
                                classification = RetryClassification.Retryable,
                            ),
                        )
                    }
                }

                val finalRemainingIds = pendingSeriesBundleIds(
                    scope = scope,
                    libraryId = library.id,
                    expectedIds = expectedIds,
                    staleIds = staleIdSet,
                    serverVersions = plan.serverVersions,
                    fetchedIds = fetchedSeriesIds,
                )
                if (finalRemainingIds.isNotEmpty()) {
                    publish(
                        seriesIncompleteState(
                            scope = scope,
                            library = library,
                            knownLibraries = knownLibraries,
                            expectedIds = expectedIds,
                            remainingIds = finalRemainingIds,
                            message = "Series cache sync is incomplete.",
                            classification = RetryClassification.Retryable,
                        ),
                    )
                } else {
                    publish(seriesFreshState(scope, library, knownLibraries, expectedIds))
                }
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

    private fun seriesFreshState(
        scope: ServerCacheScope,
        library: LibraryInfo,
        knownLibraries: List<LibraryInfo>,
        expectedIds: Set<String>? = null,
    ): LibraryRepositoryState {
        return when (val load = loadCachedSeriesAccessor(scope, library.id)) {
            is CacheLoad.Success -> {
                val missing = expectedIds?.minus(load.accessor.seriesIds.toSet()).orEmpty().sorted()
                if (missing.isNotEmpty()) {
                    return seriesIncompleteState(
                        scope = scope,
                        library = library,
                        knownLibraries = knownLibraries,
                        expectedIds = expectedIds.orEmpty(),
                        remainingIds = missing,
                        message = "Series cache sync is incomplete.",
                        classification = RetryClassification.Retryable,
                    )
                }
                _state.value.copy(
                    scope = scope,
                    libraries = knownLibraries.ifEmpty { listOf(library) },
                    selectedLibraryId = library.id,
                    movieAccessor = null,
                    seriesAccessor = load.accessor,
                    freshness = if (expectedIds != null && expectedIds.isNotEmpty()) {
                        LibraryFreshness.Fresh(load.accessor.itemCount, System.currentTimeMillis())
                    } else if (load.accessor.itemCount == 0) {
                        LibraryFreshness.Empty
                    } else {
                        LibraryFreshness.Fresh(load.accessor.itemCount, System.currentTimeMillis())
                    },
                )
            }
            is CacheLoad.Empty -> {
                val remaining = expectedIds.orEmpty().sorted()
                if (remaining.isNotEmpty()) {
                    seriesIncompleteState(
                        scope = scope,
                        library = library,
                        knownLibraries = knownLibraries,
                        expectedIds = expectedIds.orEmpty(),
                        remainingIds = remaining,
                        message = "Series cache sync is incomplete.",
                        classification = RetryClassification.Retryable,
                    )
                } else {
                    _state.value.copy(
                        scope = scope,
                        libraries = knownLibraries.ifEmpty { listOf(library) },
                        selectedLibraryId = library.id,
                        movieAccessor = null,
                        seriesAccessor = null,
                        freshness = LibraryFreshness.Empty,
                    )
                }
            }
            is CacheLoad.Corrupt -> {
                val parseableIds = load.accessor?.seriesIds.orEmpty().toSet()
                val missing = expectedIds?.minus(parseableIds).orEmpty().sorted()
                if (missing.isNotEmpty()) {
                    seriesIncompleteState(
                        scope = scope,
                        library = library,
                        knownLibraries = knownLibraries,
                        expectedIds = expectedIds.orEmpty(),
                        remainingIds = missing,
                        message = load.message,
                        classification = RetryClassification.Retryable,
                        failedBundleCount = load.quarantinedFiles,
                    )
                } else {
                    _state.value.copy(
                        scope = scope,
                        libraries = knownLibraries.ifEmpty { listOf(library) },
                        selectedLibraryId = library.id,
                        movieAccessor = null,
                        seriesAccessor = load.accessor,
                        freshness = LibraryFreshness.CorruptRebuilding(load.message, load.quarantinedFiles),
                    )
                }
            }
        }
    }

    private fun seriesIncompleteState(
        scope: ServerCacheScope,
        library: LibraryInfo,
        knownLibraries: List<LibraryInfo>,
        expectedIds: Set<String>,
        remainingIds: List<String>,
        message: String,
        classification: RetryClassification,
        failedBundleCount: Int = 0,
    ): LibraryRepositoryState {
        val load = loadCachedSeriesAccessor(scope, library.id)
        val accessor = when (load) {
            is CacheLoad.Success -> load.accessor
            is CacheLoad.Corrupt -> load.accessor
            CacheLoad.Empty -> null
        }
        val remaining = remainingIds.distinct().sorted()
        val completed = (expectedIds.size - remaining.size).coerceIn(0, expectedIds.size)
        return _state.value.copy(
            scope = scope,
            libraries = knownLibraries.ifEmpty { listOf(library) },
            selectedLibraryId = library.id,
            movieAccessor = null,
            seriesAccessor = accessor,
            freshness = LibraryFreshness.SeriesCacheIncomplete(
                message = message,
                completedBundles = completed,
                expectedBundles = expectedIds.size,
                remainingBundleIds = remaining,
                itemCount = accessor?.itemCount ?: 0,
                classification = classification,
                failedBundleCount = failedBundleCount.coerceIn(0, remaining.size),
            ),
        )
    }

    private fun expectedSeriesBundleIds(
        cachedIds: Set<String>,
        deletedIds: Collection<String>,
        staleIds: Collection<String>,
    ): Set<String> = ((cachedIds - deletedIds.toSet()) + staleIds).toSortedSet()

    private fun pendingSeriesBundleIds(
        scope: ServerCacheScope,
        libraryId: String,
        expectedIds: Set<String>,
        staleIds: Set<String>,
        serverVersions: Map<String, Long>,
        fetchedIds: Set<String>,
    ): List<String> {
        if (expectedIds.isEmpty()) return emptyList()
        val completeness = seriesCacheCompleteness(scope, libraryId, expectedIds)
        val currentVersions = cache.cachedSeriesBundleVersions(scope, libraryId)
        return expectedIds.filter { seriesId ->
            val missing = seriesId in completeness.missingIds
            val stale = if (seriesId !in staleIds) {
                false
            } else {
                val expectedVersion = serverVersions[seriesId]
                if (expectedVersion != null) currentVersions[seriesId] != expectedVersion else seriesId !in fetchedIds
            }
            missing || stale
        }.sorted()
    }

    private fun seriesCacheCompleteness(
        scope: ServerCacheScope,
        libraryId: String,
        expectedIds: Set<String>,
    ): SeriesCacheCompleteness {
        val load = loadCachedSeriesAccessor(scope, libraryId)
        val accessor = when (load) {
            is CacheLoad.Success -> load.accessor
            is CacheLoad.Corrupt -> load.accessor
            CacheLoad.Empty -> null
        }
        val parseableIds = accessor?.seriesIds.orEmpty().toSet()
        return SeriesCacheCompleteness(
            missingIds = (expectedIds - parseableIds).toSortedSet(),
        )
    }

    private fun validateFetchedSeriesBundles(bytes: ByteArray, requestedIds: List<String>): Result<List<ParsedSeriesBundle>> = runCatching {
        val requested = requestedIds.toSet()
        check(requested.isNotEmpty()) { "Series bundle fetch requested no bundle IDs" }
        val bundles = LibraryFlatBuffers.parseSeriesPayload(bytes.wrapFlatBuffer()).getOrThrow()
        val returnedIds = bundles.map { it.seriesId }
        val returned = returnedIds.toSet()
        check(returnedIds.size == returned.size) { "Series bundle response contained duplicate bundle IDs" }
        val missing = requested - returned
        val unexpected = returned - requested
        check(missing.isEmpty() && unexpected.isEmpty()) {
            buildString {
                append("Series bundle response did not match requested IDs")
                if (missing.isNotEmpty()) append("; missing=").append(missing.sorted().joinToString(","))
                if (unexpected.isNotEmpty()) append("; unexpected=").append(unexpected.sorted().joinToString(","))
            }
        }
        bundles.filter { it.seriesId in requested }.sortedBy { it.seriesId }
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

    private suspend fun coalescedLibraryJob(
        key: LibrarySyncJobKey,
        block: suspend () -> LibraryRepositoryState,
    ): LibraryRepositoryState = coroutineScope {
        var shouldStart = false
        val deferred = synchronized(libraryJobLock) {
            activeLibraryJobs[key]?.takeUnless { it.isCompleted } ?: async(start = CoroutineStart.LAZY) {
                block()
            }.also { job ->
                shouldStart = true
                activeLibraryJobs[key] = job
                job.invokeOnCompletion {
                    synchronized(libraryJobLock) {
                        if (activeLibraryJobs[key] === job) activeLibraryJobs.remove(key)
                    }
                }
            }
        }
        if (shouldStart) deferred.start()
        deferred.await()
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

    private data class LibrarySyncJobKey(
        val scopeDirectoryName: String,
        val libraryId: String,
        val kind: LibraryKind,
    )

    private data class SeriesCacheCompleteness(
        val missingIds: Set<String>,
    )

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
        const val SERIES_BUNDLE_FETCH_CHUNK_SIZE = 16
    }
}
