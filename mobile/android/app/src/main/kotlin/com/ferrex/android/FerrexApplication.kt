package com.ferrex.android

import android.app.Application
import coil.ImageLoader
import coil.ImageLoaderFactory
import com.ferrex.android.core.diagnostics.CrashHandler
import com.ferrex.android.core.diagnostics.DiagnosticLog
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

    override fun onCreate() {
        super.onCreate()

        // Install the crash handler FIRST — before any other init that could throw.
        CrashHandler.install(this)
        DiagnosticLog.i("App", "Ferrex starting (pid=${android.os.Process.myPid()})")

        // Log heap limits so crash files show the baseline
        val rt = Runtime.getRuntime()
        DiagnosticLog.i("App",
            "Heap: max=${rt.maxMemory() / (1024 * 1024)}MB " +
                "total=${rt.totalMemory() / (1024 * 1024)}MB " +
                "free=${rt.freeMemory() / (1024 * 1024)}MB"
        )
    }

    override fun newImageLoader(): ImageLoader = imageLoader
}
