package com.ferrex.android.core.diagnostics

import kotlinx.serialization.Serializable

@Serializable
data class DiagnosticsSnapshot(
    val generatedAtEpochMs: Long,
    val app: AppDiagnosticsSummary,
    val runtime: RuntimeDiagnosticsSummary = RuntimeDiagnosticsSummary.capture(),
    val device: DeviceDiagnosticsSummary? = null,
    val display: DisplayDiagnosticsSummary? = null,
    val playback: PlaybackDiagnosticsSummary = PlaybackDiagnosticsSummary(),
    val server: ServerDiagnosticsSummary = ServerDiagnosticsSummary(),
    val auth: AuthDiagnosticsSummary = AuthDiagnosticsSummary(),
    val cache: CacheDiagnosticsSummary? = null,
)

@Serializable
data class AppDiagnosticsSummary(
    val applicationId: String,
    val versionName: String,
    val versionCode: Long,
    val buildType: String? = null,
    val flavor: String? = null,
)

@Serializable
data class RuntimeDiagnosticsSummary(
    val maxMemoryBytes: Long,
    val totalMemoryBytes: Long,
    val freeMemoryBytes: Long,
    val availableProcessors: Int,
) {
    val usedMemoryBytes: Long get() = (totalMemoryBytes - freeMemoryBytes).coerceAtLeast(0L)

    companion object {
        fun capture(runtime: Runtime = Runtime.getRuntime()): RuntimeDiagnosticsSummary = RuntimeDiagnosticsSummary(
            maxMemoryBytes = runtime.maxMemory(),
            totalMemoryBytes = runtime.totalMemory(),
            freeMemoryBytes = runtime.freeMemory(),
            availableProcessors = runtime.availableProcessors(),
        )
    }
}

@Serializable
data class DeviceDiagnosticsSummary(
    val manufacturer: String,
    val brand: String,
    val model: String,
    val device: String,
    val product: String,
    val sdkInt: Int,
    val release: String,
    val supportedAbis: List<String> = emptyList(),
)

@Serializable
data class DisplayDiagnosticsSummary(
    val defaultDisplayPresent: Boolean,
    val displayName: String? = null,
    val refreshRateHz: Float? = null,
    val resolution: String? = null,
    val hdrTypes: List<String> = emptyList(),
    val desiredMaxLuminance: Float? = null,
    val desiredMaxAverageLuminance: Float? = null,
    val desiredMinLuminance: Float? = null,
    val wideColorGamut: Boolean? = null,
    val windowColorMode: String? = null,
)

@Serializable
data class PlaybackDiagnosticsSummary(
    val retainedEntryCount: Int = 0,
    val warningCount: Int = 0,
    val errorCount: Int = 0,
    val lastEventEpochMs: Long? = null,
)

@Serializable
data class ServerDiagnosticsSummary(
    val configured: Boolean = false,
    val canonicalOrigin: String? = null,
    val canonicalUrlHash: String? = null,
)

@Serializable
data class AuthDiagnosticsSummary(
    val accessTokenPresent: Boolean = false,
    val refreshTokenPresent: Boolean = false,
    val sessionPresent: Boolean = false,
    val deviceSessionPresent: Boolean = false,
    val userIdHash: String? = null,
    val requiresPinSetup: Boolean = false,
)

@Serializable
data class CacheDiagnosticsSummary(
    val library: LibraryCacheDiagnosticsSummary? = null,
    val image: ImageCacheDiagnosticsSummary? = null,
)

@Serializable
data class CacheHealthDiagnosticsSummary(
    val state: String,
    val selectedLibraryIdHash: String? = null,
    val cachedItems: Int? = null,
    val expectedItems: Int? = null,
    val pendingItems: Int? = null,
    val failedItems: Int? = null,
    val cachedSeriesBundles: Int? = null,
    val expectedSeriesBundles: Int? = null,
    val pendingSeriesBundles: Int? = null,
    val failedSeriesBundles: Int? = null,
    val quarantinedPayloads: Int? = null,
    val retryClassification: String? = null,
)

@Serializable
data class LibraryCacheDiagnosticsSummary(
    val scopeDirectoryName: String,
    val relativeScopePath: String,
    val approximateBytes: Long,
    val libraryListPresent: Boolean,
    val libraryDirectoryCount: Int,
    val knownLibraryCount: Int? = null,
    val cachedMovieBatchFiles: Int,
    val cachedSeriesBundleFiles: Int,
    val cachedMovieCount: Int? = null,
    val cachedSeriesCount: Int? = null,
    val cachedEpisodeCount: Int? = null,
    val quarantineFileCount: Int,
    val staleOfflineMarkerPresent: Boolean,
    val health: CacheHealthDiagnosticsSummary? = null,
    val selectedClearPreservesOtherLibraries: Boolean = true,
    val allClearScopedToServerUser: Boolean = true,
)

@Serializable
data class ImageCacheDiagnosticsSummary(
    val scopeDirectoryName: String,
    val relativeImagesPath: String,
    val approximateBytes: Long,
    val manifestEntryFiles: Int,
    val coilBlobBytes: Long,
    val quarantineFileCount: Int,
    val staleOfflineMarkerPresent: Boolean,
    val selectedClearDropsCoilBlobsForScope: Boolean = true,
    val allClearPreservesLibraryAndWatchState: Boolean = true,
)

@Serializable
data class RetainedCrashDiagnosticsFile(
    val name: String,
    val sizeBytes: Long,
    val lastModifiedEpochMs: Long,
)

@Serializable
data class DiagnosticsBundleManifest(
    val generatedAtEpochMs: Long,
    val snapshot: DiagnosticsSnapshot,
    val retainedLogCount: Int,
    val retainedCrashFiles: List<RetainedCrashDiagnosticsFile>,
    val files: List<String>,
)
