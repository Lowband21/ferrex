package com.ferrex.android.core.image

import android.content.Context
import coil.ImageLoader
import coil.disk.DiskCache
import coil.memory.MemoryCache
import com.ferrex.android.core.api.FerrexApiClient
import com.ferrex.android.core.api.ServerConfig
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import okhttp3.OkHttpClient
import javax.inject.Singleton

/**
 * Coil ImageLoader configured for Ferrex's content-addressed blob URLs.
 *
 * Key properties:
 * - Memory cache: 25% of available heap (critical for scroll performance)
 * - Disk cache: 100MB (poster images are typically 10-50KB each)
 * - Content-addressed URLs (/images/blob/{token}) are immutable —
 *   Coil respects Cache-Control: immutable headers, so images are
 *   cached permanently once downloaded.
 */
@Module
@InstallIn(SingletonComponent::class)
object ImagePipelineModule {

    @Provides
    @Singleton
    fun provideImageLoader(
        @ApplicationContext context: Context,
        okHttpClient: OkHttpClient,
    ): ImageLoader {
        return ImageLoader.Builder(context)
            .okHttpClient(okHttpClient)
            .memoryCache {
                MemoryCache.Builder(context)
                    .maxSizePercent(0.25) // 25% of available heap
                    .build()
            }
            .diskCache {
                DiskCache.Builder()
                    .directory(context.cacheDir.resolve("coil_image_cache"))
                    .maxSizeBytes(100L * 1024 * 1024) // 100MB
                    .build()
            }
            .crossfade(true)
            .build()
    }
}

/**
 * Builds image URLs from FlatBuffer media data.
 *
 * Image IIDs (instance UUIDs) from FlatBuffer details fields like
 * `primary_poster_iid` are passed to the server's `/images/iid/{uuid}`
 * endpoint, which resolves the IID to the content-addressed blob and
 * serves the image. Coil caches responses permanently (immutable blobs).
 */
object ImageUrlBuilder {

    /**
     * Build a URL for a content-addressed image blob by its token (64-char hex).
     */
    fun blobUrl(serverConfig: ServerConfig, token: String): String =
        "${serverConfig.serverUrl}/api/v1/images/blob/$token"

    /**
     * Build a URL to serve an image by its IID (image instance UUID).
     * The server resolves the IID → blob token internally.
     */
    fun iidUrl(serverConfig: ServerConfig, iid: String): String =
        "${serverConfig.serverUrl}/api/v1/images/iid/$iid"
}
