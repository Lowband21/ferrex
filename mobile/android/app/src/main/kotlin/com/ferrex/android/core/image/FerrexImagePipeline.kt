package com.ferrex.android.core.image

import android.content.Context
import coil.ImageLoader
import coil.disk.DiskCache
import coil.memory.MemoryCache
import com.ferrex.android.core.library.ServerCacheScope
import okhttp3.OkHttpClient
import java.util.concurrent.ConcurrentHashMap

/**
 * Scope-aware Coil pipeline for manifest-resolved immutable blob URLs.
 *
 * The OkHttp client is the same auth-enabled client used by protected API calls.
 * Disk cache directories sit under the server/user scope so selected/all cache
 * recovery actions can remove image metadata and blobs without an OS app-data wipe.
 */
class FerrexImagePipeline(
    private val context: Context,
    private val authenticatedHttpClient: OkHttpClient,
    private val imageDiskCache: ImageDiskCache,
) {
    private val loaders = ConcurrentHashMap<String, ImageLoader>()

    fun imageLoader(scope: ServerCacheScope): ImageLoader = loaders.getOrPut(scope.directoryName) {
        ImageLoader.Builder(context.applicationContext)
            .okHttpClient(authenticatedHttpClient)
            .memoryCache {
                MemoryCache.Builder(context.applicationContext)
                    .maxSizePercent(0.25)
                    .build()
            }
            .diskCache {
                DiskCache.Builder()
                    .directory(imageDiskCache.coilDiskCacheDir(scope))
                    .maxSizeBytes(MAX_DISK_CACHE_BYTES)
                    .build()
            }
            .crossfade(true)
            .build()
    }

    fun clear(scope: ServerCacheScope) {
        loaders.remove(scope.directoryName)?.shutdown()
        imageDiskCache.clearAll(scope)
    }

    companion object {
        const val MAX_DISK_CACHE_BYTES: Long = 100L * 1024L * 1024L
    }
}
