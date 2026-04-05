package com.ferrex.android.core.api

import com.ferrex.android.core.image.ImageRetryInterceptor
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit
import javax.inject.Qualifier
import javax.inject.Singleton

/** Qualifier for the streaming-specific OkHttpClient (longer timeouts). */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class StreamingClient

@Module
@InstallIn(SingletonComponent::class)
object NetworkModule {

    /**
     * Primary OkHttpClient for API calls and image loading.
     * 30s read timeout is fine for API requests and image downloads.
     */
    @Provides
    @Singleton
    fun provideOkHttpClient(
        authInterceptor: AuthInterceptor,
        tokenRefreshAuthenticator: TokenRefreshAuthenticator,
    ): OkHttpClient {
        return OkHttpClient.Builder()
            .addInterceptor(authInterceptor)
            .addInterceptor(ImageRetryInterceptor())
            .authenticator(tokenRefreshAuthenticator)
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .writeTimeout(30, TimeUnit.SECONDS)
            .build()
    }

    /**
     * Streaming OkHttpClient for ExoPlayer media playback.
     *
     * Key differences from the primary client:
     * - No read timeout: ExoPlayer buffers ahead then pauses reading;
     *   a 30s read timeout would kill the socket during buffer-full pauses.
     * - No image retry interceptor: not needed for video streams.
     * - Longer connect timeout: large files on slow storage may take time.
     */
    @Provides
    @Singleton
    @StreamingClient
    fun provideStreamingOkHttpClient(
        authInterceptor: AuthInterceptor,
        tokenRefreshAuthenticator: TokenRefreshAuthenticator,
    ): OkHttpClient {
        return OkHttpClient.Builder()
            .addInterceptor(authInterceptor)
            .authenticator(tokenRefreshAuthenticator)
            .connectTimeout(30, TimeUnit.SECONDS)
            .readTimeout(0, TimeUnit.SECONDS)   // No read timeout for streaming
            .writeTimeout(0, TimeUnit.SECONDS)
            .build()
    }
}
