package com.ferrex.android.core.image

import com.ferrex.android.core.library.LibrarySyncFailure
import com.ferrex.android.core.library.LibrarySyncResult
import com.ferrex.android.core.library.RetryClassification
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.library.toJavaUuidOrNull
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

interface ImageCacheClearer {
    fun clearSelectedImages(scope: ServerCacheScope, keys: Collection<ImageRequestKey>)
    fun clearAllImages(scope: ServerCacheScope)
}

class ImageRepository(
    private val transport: ImageManifestTransport,
    private val cache: ImageDiskCache,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
    private val clockMillis: () -> Long = { System.currentTimeMillis() },
) : ImageCacheClearer {
    suspend fun resolveImages(
        scope: ServerCacheScope,
        requestedKeys: Collection<ImageRequestKey>,
    ): Map<ImageRequestKey, ImageResolution> = withContext(ioDispatcher) {
        val keys = requestedKeys.distinct()
        val invalid = keys.filter { it.iid.toJavaUuidOrNull() == null }
            .associateWith { key -> ImageResolution.Placeholder(key, "Image iid is not a valid UUID") as ImageResolution }
        val valid = keys.filter { it.iid.toJavaUuidOrNull() != null }
        invalid + refreshManifest(scope, valid)
    }

    /**
     * Refresh only the visible pending/failed images. Ready images stay cached and
     * no OkHttp thread sleeps while waiting for server-side cache fills.
     */
    suspend fun retryPendingOrFailed(
        scope: ServerCacheScope,
        visibleKeys: Collection<ImageRequestKey>,
    ): Map<ImageRequestKey, ImageResolution> = withContext(ioDispatcher) {
        val distinct = visibleKeys.distinct()
        val retryKeys = distinct.filter { key ->
            when (val cached = cache.readManifestEntry(scope, key)) {
                is ManifestCacheRead.Valid -> cached.record.status !is ManifestImageStatus.Ready
                is ManifestCacheRead.Corrupt -> true
                ManifestCacheRead.Missing -> false
            }
        }
        if (retryKeys.isEmpty()) {
            return@withContext distinct.associateWith { cachedOrPlaceholder(scope, it) }
        }
        refreshManifest(scope, retryKeys)
    }

    override fun clearSelectedImages(scope: ServerCacheScope, keys: Collection<ImageRequestKey>) {
        cache.clearManifestEntries(scope, keys)
    }

    override fun clearAllImages(scope: ServerCacheScope) {
        cache.clearAll(scope)
    }

    private suspend fun refreshManifest(
        scope: ServerCacheScope,
        keys: Collection<ImageRequestKey>,
    ): Map<ImageRequestKey, ImageResolution> {
        val distinct = keys.distinct()
        if (distinct.isEmpty()) return emptyMap()
        return when (val manifest = transport.fetchManifest(distinct)) {
            is LibrarySyncResult.Success -> mergeManifest(scope, distinct, manifest.value)
            is LibrarySyncResult.Failure -> staleOrFailure(scope, distinct, manifest.error)
        }
    }

    private fun mergeManifest(
        scope: ServerCacheScope,
        requestedKeys: Collection<ImageRequestKey>,
        records: List<ImageManifestRecord>,
    ): Map<ImageRequestKey, ImageResolution> {
        val byKey = records.associateBy { it.key }
        return requestedKeys.associateWith { key ->
            val record = byKey[key]
                ?: return@associateWith ImageResolution.Failed(
                    key = key,
                    reason = "Image manifest did not include ${key.category.wireName} ${key.iid}",
                    retryable = true,
                )
            cache.writeManifestEntry(scope, record)
            record.toResolution(scope, stale = false, offlineMessage = null)
        }
    }

    private fun staleOrFailure(
        scope: ServerCacheScope,
        keys: Collection<ImageRequestKey>,
        failure: LibrarySyncFailure,
    ): Map<ImageRequestKey, ImageResolution> {
        cache.markStaleOffline(scope, failure.message)
        return keys.associateWith { key ->
            when (val cached = cache.readManifestEntry(scope, key)) {
                is ManifestCacheRead.Valid -> cached.record.toResolution(scope, stale = true, offlineMessage = failure.message)
                is ManifestCacheRead.Corrupt -> ImageResolution.Failed(
                    key = key,
                    reason = "Corrupt image manifest cache was quarantined: ${cached.message}",
                    retryable = true,
                    stale = true,
                )
                ManifestCacheRead.Missing -> ImageResolution.Failed(
                    key = key,
                    reason = "Image manifest unavailable: ${failure.message}",
                    retryable = failure.classification == RetryClassification.Retryable,
                )
            }
        }
    }

    private fun cachedOrPlaceholder(scope: ServerCacheScope, key: ImageRequestKey): ImageResolution =
        when (val cached = cache.readManifestEntry(scope, key)) {
            is ManifestCacheRead.Valid -> cached.record.toResolution(scope, stale = false, offlineMessage = null)
            is ManifestCacheRead.Corrupt -> ImageResolution.Failed(
                key = key,
                reason = "Corrupt image manifest cache was quarantined: ${cached.message}",
                retryable = true,
            )
            ManifestCacheRead.Missing -> ImageResolution.Placeholder(key, "No manifest status has been requested for this image yet")
        }

    private fun ImageManifestRecord.toResolution(
        scope: ServerCacheScope,
        stale: Boolean,
        offlineMessage: String?,
    ): ImageResolution = when (val manifestStatus = status) {
        is ManifestImageStatus.Ready -> ImageResolution.Ready(
            key = key,
            url = "${scope.canonicalServerUrl}${OkHttpImageManifestTransport.Routes.blob(manifestStatus.token)}",
            token = manifestStatus.token,
            stale = stale,
            offlineMessage = offlineMessage,
        )
        is ManifestImageStatus.Pending -> ImageResolution.Pending(
            key = key,
            retryAfterMillis = manifestStatus.retryAfterMillis,
            retryAtMillis = clockMillis() + manifestStatus.retryAfterMillis,
            stale = stale,
            offlineMessage = offlineMessage,
        )
        is ManifestImageStatus.Failed -> ImageResolution.Failed(
            key = key,
            reason = manifestStatus.reason,
            retryable = true,
            stale = stale,
        )
    }
}
