package com.ferrex.android.core.diagnostics

import android.app.Application
import android.content.Context
import android.content.pm.PackageInfo
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.Process
import android.view.Display
import android.view.Window
import com.ferrex.android.BuildConfig
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.auth.AuthStorage
import com.ferrex.android.core.image.ImageDiskCache
import com.ferrex.android.core.library.LibraryDiskCache
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.ServerCacheScope
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.system.exitProcess

class AndroidDiagnosticsCore(
    private val context: Context,
    private val storage: AuthStorage,
    private val serverConfig: ServerConfig,
    private val libraryCache: LibraryDiskCache,
    private val imageCache: ImageDiskCache,
    private val libraryRepository: LibraryRepository,
    private val files: DiagnosticsFiles = DiagnosticsFiles.fromFilesDir(context.filesDir),
    private val crashStore: CrashRetentionStore = CrashRetentionStore(files.rootDir),
    private val exportBuilder: DiagnosticsExportBuilder = DiagnosticsExportBuilder(files, crashStore),
) {
    fun snapshot(display: DisplayDiagnosticsSummary? = null): DiagnosticsSnapshot {
        val savedServerUrl = runCatching { storage.serverUrl }.getOrNull()
        val savedUserId = runCatching { storage.userId }.getOrNull()
        val configuredServerUrl = savedServerUrl ?: serverConfig.serverUrl
        val scope = ServerCacheScope.fromOrNull(configuredServerUrl, savedUserId)
        val cacheSummary = scope?.let { scoped ->
            SafeCacheDiagnostics.summarize(
                library = libraryCache.diagnosticSnapshot(scoped),
                image = imageCache.diagnosticSnapshot(scoped),
                state = libraryRepository.state.value,
            )
        }
        return DiagnosticsSnapshot(
            generatedAtEpochMs = System.currentTimeMillis(),
            app = AndroidAppDiagnostics.summary(context),
            runtime = RuntimeDiagnosticsSummary.capture(),
            device = AndroidDeviceDiagnostics.summary(),
            display = display,
            playback = PlaybackDiagnosticsSummaryProvider.summarize(),
            server = SafeServerDiagnostics.summarize(configuredServerUrl),
            auth = SafeAuthDiagnostics.summarize(storage),
            cache = cacheSummary,
        )
    }

    fun exportBundle(display: DisplayDiagnosticsSummary? = null): File = exportBuilder.build(snapshot(display))

    fun clearDiagnostics() {
        DiagnosticsMaintenance.clearDiagnostics(files)
    }

    fun retainedCrashFiles(): List<File> = crashStore.retainedCrashFiles()
}

object DiagnosticsCrashHandler {
    private val installed = AtomicBoolean(false)

    fun install(context: Context) {
        if (!installed.compareAndSet(false, true)) return
        val appContext = context.applicationContext
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        val files = DiagnosticsFiles.fromFilesDir(appContext.filesDir)
        val crashStore = CrashRetentionStore(files.rootDir)
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            try {
                crashStore.writeCrash(
                    thread = thread,
                    throwable = throwable,
                    snapshot = DiagnosticsSnapshot(
                        generatedAtEpochMs = System.currentTimeMillis(),
                        app = AndroidAppDiagnostics.summary(appContext),
                        runtime = RuntimeDiagnosticsSummary.capture(),
                        device = AndroidDeviceDiagnostics.summary(),
                    ),
                )
            } catch (_: Throwable) {
                // Never throw from the crash path; delegate to Android below.
            } finally {
                if (previous != null) {
                    previous.uncaughtException(thread, throwable)
                } else {
                    Process.killProcess(Process.myPid())
                    exitProcess(10)
                }
            }
        }
        DiagnosticLog.info("CrashHandler", "Android crash handler installed", source = DiagnosticLog.Source.Diagnostics)
    }
}

object AndroidAppDiagnostics {
    fun summary(context: Context): AppDiagnosticsSummary {
        val info = runCatching { context.packageManager.getPackageInfoCompat(context.packageName) }.getOrNull()
        return AppDiagnosticsSummary(
            applicationId = context.packageName,
            versionName = info?.versionName ?: BuildConfig.VERSION_NAME,
            versionCode = info?.longVersionCodeCompat() ?: BuildConfig.VERSION_CODE.toLong(),
            buildType = BuildConfig.BUILD_TYPE,
            flavor = BuildConfig.FLAVOR.takeIf { it.isNotBlank() },
        )
    }

    private fun android.content.pm.PackageManager.getPackageInfoCompat(packageName: String): PackageInfo =
        if (Build.VERSION.SDK_INT >= 33) {
            getPackageInfo(packageName, android.content.pm.PackageManager.PackageInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            getPackageInfo(packageName, 0)
        }

    private fun PackageInfo.longVersionCodeCompat(): Long =
        if (Build.VERSION.SDK_INT >= 28) {
            longVersionCode
        } else {
            @Suppress("DEPRECATION")
            versionCode.toLong()
        }
}

object AndroidDeviceDiagnostics {
    fun summary(): DeviceDiagnosticsSummary = DeviceDiagnosticsSummary(
        manufacturer = Build.MANUFACTURER.orUnknown(),
        brand = Build.BRAND.orUnknown(),
        model = Build.MODEL.orUnknown(),
        device = Build.DEVICE.orUnknown(),
        product = Build.PRODUCT.orUnknown(),
        sdkInt = Build.VERSION.SDK_INT,
        release = Build.VERSION.RELEASE.orUnknown(),
        supportedAbis = Build.SUPPORTED_ABIS?.toList().orEmpty(),
    )

    private fun String?.orUnknown(): String = this?.takeIf { it.isNotBlank() } ?: "unknown"
}

object AndroidDisplayDiagnostics {
    fun snapshot(context: Context, window: Window? = null): DisplayDiagnosticsSummary {
        val display = runCatching {
            context.getSystemService(DisplayManager::class.java)?.getDisplay(Display.DEFAULT_DISPLAY)
        }.getOrNull()
            ?: return DisplayDiagnosticsSummary(defaultDisplayPresent = false)
        val mode = display.mode
        val hdrCapabilities = runCatching {
            @Suppress("DEPRECATION")
            display.hdrCapabilities
        }.getOrNull()
        val hdrTypes = when {
            Build.VERSION.SDK_INT >= 34 -> runCatching { mode.supportedHdrTypes.toList() }.getOrDefault(emptyList())
            else -> {
                @Suppress("DEPRECATION")
                hdrCapabilities?.supportedHdrTypes?.toList().orEmpty()
            }
        }.map(::hdrTypeName).distinct().sorted()
        return DisplayDiagnosticsSummary(
            defaultDisplayPresent = true,
            displayName = display.name,
            refreshRateHz = display.refreshRate,
            resolution = "${mode.physicalWidth}x${mode.physicalHeight}",
            hdrTypes = hdrTypes,
            desiredMaxLuminance = hdrCapabilities?.desiredMaxLuminance,
            desiredMaxAverageLuminance = hdrCapabilities?.desiredMaxAverageLuminance,
            desiredMinLuminance = hdrCapabilities?.desiredMinLuminance,
            wideColorGamut = runCatching { display.isWideColorGamut }.getOrNull(),
            windowColorMode = window?.let { colorModeName(it.colorMode) },
        )
    }

    fun logCurrentDisplay(context: Context, window: Window? = null) {
        val summary = snapshot(context, window)
        DiagnosticLog.info(
            "DisplayDiagnostics",
            "displayPresent=${summary.defaultDisplayPresent} hdrTypes=${summary.hdrTypes} wideColor=${summary.wideColorGamut ?: "unknown"} colorMode=${summary.windowColorMode ?: "unknown"}",
            source = DiagnosticLog.Source.Diagnostics,
        )
    }

    private fun hdrTypeName(type: Int): String = when (type) {
        Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION -> "DolbyVision"
        Display.HdrCapabilities.HDR_TYPE_HDR10 -> "HDR10"
        Display.HdrCapabilities.HDR_TYPE_HDR10_PLUS -> "HDR10+"
        Display.HdrCapabilities.HDR_TYPE_HLG -> "HLG"
        else -> "UNKNOWN($type)"
    }

    private fun colorModeName(mode: Int): String = when (mode) {
        android.content.pm.ActivityInfo.COLOR_MODE_DEFAULT -> "DEFAULT"
        android.content.pm.ActivityInfo.COLOR_MODE_WIDE_COLOR_GAMUT -> "WIDE_COLOR_GAMUT"
        android.content.pm.ActivityInfo.COLOR_MODE_HDR -> "HDR"
        else -> "UNKNOWN($mode)"
    }
}

class FerrexDiagnosticsApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        DiagnosticsCrashHandler.install(this)
    }
}
