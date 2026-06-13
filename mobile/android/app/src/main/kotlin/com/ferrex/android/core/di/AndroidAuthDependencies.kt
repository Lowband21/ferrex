package com.ferrex.android.core.di

import android.content.Context
import com.ferrex.android.BuildConfig
import com.ferrex.android.core.api.AuthInterceptor
import com.ferrex.android.core.api.FerrexApiClient
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.api.TokenRefreshAuthenticator
import com.ferrex.android.core.auth.AuthManager
import com.ferrex.android.core.auth.EncryptedAuthStorage
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageDiskCache
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.OkHttpImageManifestTransport
import com.ferrex.android.core.library.LibraryDiskCache
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.OkHttpLibrarySyncTransport
import com.ferrex.android.core.library.ServerCacheScope
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit

class AndroidAuthDependencies(
    context: Context,
    deviceName: String,
) {
    val serverConfig = ServerConfig()
    val authInterceptor = AuthInterceptor()
    val tokenRefreshAuthenticator = TokenRefreshAuthenticator(serverConfig, authInterceptor)

    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS)
        .writeTimeout(20, TimeUnit.SECONDS)
        .addInterceptor(authInterceptor)
        .authenticator(tokenRefreshAuthenticator)
        .build()

    private val storage = EncryptedAuthStorage(context)
    private val apiClient = FerrexApiClient(httpClient, serverConfig)
    private val libraryCache = LibraryDiskCache.fromContext(context)
    private val imageCache = ImageDiskCache.fromContext(context)
    private val libraryTransport = OkHttpLibrarySyncTransport(httpClient, serverConfig)
    private val imageTransport = OkHttpImageManifestTransport(httpClient, serverConfig)

    val imageRepository = ImageRepository(
        transport = imageTransport,
        cache = imageCache,
    )

    val imagePipeline = FerrexImagePipeline(
        context = context,
        authenticatedHttpClient = httpClient,
        imageDiskCache = imageCache,
    )

    val libraryRepository = LibraryRepository(
        transport = libraryTransport,
        cache = libraryCache,
        imageCacheClearer = imageRepository,
    )

    val authManager = AuthManager(
        api = apiClient,
        storage = storage,
        serverConfig = serverConfig,
        authInterceptor = authInterceptor,
        tokenRefreshAuthenticator = tokenRefreshAuthenticator,
        deviceName = deviceName,
        appVersion = BuildConfig.VERSION_NAME,
        onResetConnectionCacheClear = { serverUrl, userId ->
            val scope = ServerCacheScope.from(serverUrl, userId)
            libraryCache.clearAllForScope(scope)
            imageRepository.clearAllImages(scope)
        },
    )
}
