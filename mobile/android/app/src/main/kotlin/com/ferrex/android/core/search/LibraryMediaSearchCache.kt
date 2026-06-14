package com.ferrex.android.core.search

import com.ferrex.android.core.library.CachedMediaReference
import com.ferrex.android.core.library.CachedMediaResyncSummary
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.ServerCacheScope

class LibraryMediaSearchCache(
    private val libraryRepository: LibraryRepository,
) : MediaSearchCache {
    override fun resolve(scope: ServerCacheScope, id: SearchMediaId): CachedMediaReference? =
        libraryRepository.resolveCachedMedia(scope, id.toCachedLookupKey())

    override fun freshness(scope: ServerCacheScope): LibraryFreshness = libraryRepository.searchFreshness(scope)

    override suspend fun resync(scope: ServerCacheScope, id: SearchMediaId): CachedMediaResyncSummary =
        libraryRepository.resyncCachedMediaForSearch(scope, id.toCachedLookupKey())
}
