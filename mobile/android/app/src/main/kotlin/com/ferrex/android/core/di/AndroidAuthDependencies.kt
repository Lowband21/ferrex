package com.ferrex.android.core.di

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import com.ferrex.android.BuildConfig
import com.ferrex.android.core.api.AuthInterceptor
import com.ferrex.android.core.api.FerrexApiClient
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.api.TokenRefreshAuthenticator
import com.ferrex.android.core.auth.AuthManager
import com.ferrex.android.core.auth.EncryptedAuthStorage
import com.ferrex.android.core.diagnostics.AndroidDiagnosticsCore
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageDiskCache
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.OkHttpImageManifestTransport
import com.ferrex.android.core.image.ScopedImageCacheClearer
import com.ferrex.android.core.browse.OkHttpLibraryIndexTransport
import com.ferrex.android.core.library.LibraryDiskCache
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.OkHttpLibrarySyncTransport
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.playback.OkHttpPlaybackProgressReporter
import com.ferrex.android.core.playback.OkHttpPlaybackTicketTransport
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.WatchStatePlaybackResumeProgressProvider
import com.ferrex.android.core.search.LibraryMediaSearchCache
import com.ferrex.android.core.search.MediaSearchRepository
import com.ferrex.android.core.search.OkHttpMediaSearchTransport
import com.ferrex.android.core.watch.ContinueWatchingRepository
import com.ferrex.android.core.watch.OkHttpContinueWatchingTransport
import com.ferrex.android.core.watch.OkHttpWatchStateTransport
import com.ferrex.android.core.watch.WatchRepository
import com.ferrex.android.core.watch.WatchStateInvalidationBus
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit

class AndroidAuthDependencies(
    context: Context,
    deviceName: String,
) {
    val serverConfig = ServerConfig()
    val authInterceptor = AuthInterceptor()
    val tokenRefreshAuthenticator = TokenRefreshAuthenticator(serverConfig, authInterceptor)

    private val authScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS)
        .writeTimeout(20, TimeUnit.SECONDS)
        .addInterceptor(authInterceptor)
        .authenticator(tokenRefreshAuthenticator)
        .build()

    val streamingHttpClient: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .writeTimeout(30, TimeUnit.SECONDS)
        .retryOnConnectionFailure(true)
        .build()

    private val storage = EncryptedAuthStorage(context)
    private val apiClient = FerrexApiClient(httpClient, serverConfig)
    private val libraryCache = LibraryDiskCache.fromContext(context)
    private val imageCache = ImageDiskCache.fromContext(context)
    private val libraryTransport = OkHttpLibrarySyncTransport(httpClient, serverConfig)
    private val imageTransport = OkHttpImageManifestTransport(httpClient, serverConfig)
    private val searchTransport = OkHttpMediaSearchTransport(httpClient, serverConfig)
    private val continueWatchingTransport = OkHttpContinueWatchingTransport(httpClient, serverConfig)
    private val watchStateTransport = OkHttpWatchStateTransport(httpClient, serverConfig)

    val playbackTicketTransport = OkHttpPlaybackTicketTransport(httpClient, serverConfig)
    val playbackStreamUrlFactory = PlaybackStreamUrlFactory(serverConfig)
    val playbackProgressReporter = OkHttpPlaybackProgressReporter(httpClient, serverConfig)
    val playbackResumeProgressProvider = WatchStatePlaybackResumeProgressProvider(watchStateTransport)

    val libraryIndexTransport = OkHttpLibraryIndexTransport(httpClient, serverConfig)
    val watchStateInvalidationBus = WatchStateInvalidationBus()

    val continueWatchingRepository = ContinueWatchingRepository(
        transport = continueWatchingTransport,
    )

    val watchRepository = WatchRepository(
        transport = watchStateTransport,
        invalidationBus = watchStateInvalidationBus,
    )

    val imageRepository = ImageRepository(
        transport = imageTransport,
        cache = imageCache,
    )

    val imagePipeline = FerrexImagePipeline(
        context = context,
        authenticatedHttpClient = httpClient,
        imageDiskCache = imageCache,
    )

    private val scopedImageCacheClearer = ScopedImageCacheClearer(
        manifestCacheClearer = imageRepository,
        imageLoaderClearer = imagePipeline,
    )

    val libraryRepository = LibraryRepository(
        transport = libraryTransport,
        cache = libraryCache,
        imageCacheClearer = scopedImageCacheClearer,
    )

    val searchRepository = MediaSearchRepository(
        transport = searchTransport,
        cache = LibraryMediaSearchCache(libraryRepository),
    )

    val diagnostics = AndroidDiagnosticsCore(
        context = context,
        storage = storage,
        serverConfig = serverConfig,
        libraryCache = libraryCache,
        imageCache = imageCache,
        libraryRepository = libraryRepository,
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
            scopedImageCacheClearer.clearAllImages(scope)
        },
        reconnectScope = authScope,
    )

    private val connectivityManager = context.getSystemService(ConnectivityManager::class.java)
    private val reconnectNetworkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            authManager.notifyConnectivityAvailable()
        }
    }

    init {
        connectivityManager?.registerDefaultNetworkCallback(reconnectNetworkCallback)
    }
}
