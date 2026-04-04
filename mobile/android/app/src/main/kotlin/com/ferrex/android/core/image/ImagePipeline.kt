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
 * V1 simplification: constructs blob URLs directly from poster IID tokens
 * embedded in media references, bypassing the image manifest endpoint.
 * The manifest-aware prefetch layer is added later.
 */
object BlobUrlBuilder {

    /**
     * Build the full URL for a content-addressed image blob.
     * These URLs are immutable — Coil caches them permanently.
     */
    fun posterUrl(serverConfig: ServerConfig, imageToken: String): String =
        "${serverConfig.serverUrl}/api/v1/images/blob/$imageToken"
}
