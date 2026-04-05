package com.ferrex.android

import android.app.Application
import coil.ImageLoader
import coil.ImageLoaderFactory
import dagger.hilt.android.HiltAndroidApp
import javax.inject.Inject

/**
 * Application class for Ferrex.
 *
 * Implements [ImageLoaderFactory] so that Coil's `AsyncImage` composable
 * automatically uses our auth-enabled [ImageLoader] (configured with the
 * app's OkHttpClient + AuthInterceptor). Without this, Coil creates a
 * default ImageLoader that can't authenticate with the server.
 */
@HiltAndroidApp
class FerrexApplication : Application(), ImageLoaderFactory {

    @Inject
    lateinit var imageLoader: ImageLoader

    override fun newImageLoader(): ImageLoader = imageLoader
}
