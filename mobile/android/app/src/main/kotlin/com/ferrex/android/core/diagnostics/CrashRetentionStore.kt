package com.ferrex.android.core.diagnostics

import java.io.File
import java.nio.charset.StandardCharsets

class CrashRetentionStore(
    diagnosticsRoot: File,
    private val maxCrashFiles: Int = DEFAULT_MAX_CRASH_FILES,
    private val maxCrashFileBytes: Int = DEFAULT_MAX_CRASH_FILE_BYTES,
    private val clockMillis: () -> Long = { System.currentTimeMillis() },
) {
    val crashDir: File = File(diagnosticsRoot, "crashes").also { it.mkdirs() }

    fun writeCrash(
        thread: Thread,
        throwable: Throwable,
        snapshot: DiagnosticsSnapshot,
        recentEntries: List<DiagnosticLog.Entry> = DiagnosticLog.recentEntries(limit = 200),
    ): File = writeCrash(
        threadName = thread.name,
        threadId = thread.id,
        throwable = throwable,
        snapshot = snapshot,
        recentEntries = recentEntries,
    )

    fun writeCrash(
        threadName: String,
        threadId: Long,
        throwable: Throwable,
        snapshot: DiagnosticsSnapshot,
        recentEntries: List<DiagnosticLog.Entry> = DiagnosticLog.recentEntries(limit = 200),
    ): File {
        crashDir.mkdirs()
        val generatedAt = clockMillis()
        val file = uniqueCrashFile(generatedAt)
        val report = renderCrashReport(generatedAt, threadName, threadId, throwable, snapshot, recentEntries)
        file.writeBytes(report.toBoundedUtf8(maxCrashFileBytes))
        file.setLastModified(generatedAt)
        pruneOldCrashes()
        DiagnosticLog.error("CrashRetention", "Retained crash file ${file.name}", source = DiagnosticLog.Source.Crash)
        return file
    }

    fun retainedCrashFiles(): List<File> = crashDir
        .listFiles { file -> file.isFile && file.name.startsWith("crash-") && file.extension == "txt" }
        ?.sortedByDescending { it.name }
        .orEmpty()

    fun retainedCrashFileSummaries(): List<RetainedCrashDiagnosticsFile> = retainedCrashFiles().map { file ->
        RetainedCrashDiagnosticsFile(
            name = file.name,
            sizeBytes = file.length(),
            lastModifiedEpochMs = file.lastModified(),
        )
    }

    fun clear() {
        crashDir.deleteRecursively()
        crashDir.mkdirs()
    }

    private fun renderCrashReport(
        generatedAt: Long,
        threadName: String,
        threadId: Long,
        throwable: Throwable,
        snapshot: DiagnosticsSnapshot,
        recentEntries: List<DiagnosticLog.Entry>,
    ): String = DiagnosticsRedactor.redactText(
        buildString {
            appendLine("=== Ferrex Crash Report ===")
            appendLine("generatedAtEpochMs=$generatedAt")
            appendLine("thread=${threadName} (id=$threadId)")
            appendLine()
            appendLine("--- App ---")
            appendLine("applicationId=${snapshot.app.applicationId}")
            appendLine("versionName=${snapshot.app.versionName}")
            appendLine("versionCode=${snapshot.app.versionCode}")
            snapshot.app.buildType?.let { appendLine("buildType=$it") }
            snapshot.app.flavor?.let { appendLine("flavor=$it") }
            appendLine()
            appendLine("--- Runtime ---")
            appendLine("heapUsedBytes=${snapshot.runtime.usedMemoryBytes}")
            appendLine("heapTotalBytes=${snapshot.runtime.totalMemoryBytes}")
            appendLine("heapMaxBytes=${snapshot.runtime.maxMemoryBytes}")
            appendLine("availableProcessors=${snapshot.runtime.availableProcessors}")
            snapshot.device?.let { device ->
                appendLine()
                appendLine("--- Device ---")
                appendLine("manufacturer=${device.manufacturer}")
                appendLine("brand=${device.brand}")
                appendLine("model=${device.model}")
                appendLine("device=${device.device}")
                appendLine("product=${device.product}")
                appendLine("sdkInt=${device.sdkInt}")
                appendLine("release=${device.release}")
                appendLine("supportedAbis=${device.supportedAbis.joinToString()}")
            }
            snapshot.display?.let { display ->
                appendLine()
                appendLine("--- Display ---")
                appendLine("defaultDisplayPresent=${display.defaultDisplayPresent}")
                appendLine("displayName=${display.displayName ?: "unknown"}")
                appendLine("resolution=${display.resolution ?: "unknown"}")
                appendLine("refreshRateHz=${display.refreshRateHz ?: "unknown"}")
                appendLine("hdrTypes=${display.hdrTypes.joinToString()}")
                appendLine("wideColorGamut=${display.wideColorGamut ?: "unknown"}")
                appendLine("windowColorMode=${display.windowColorMode ?: "unknown"}")
            }
            appendLine()
            appendLine("--- Playback Summary ---")
            appendLine("playbackEntries=${snapshot.playback.retainedEntryCount}")
            appendLine("playbackWarnings=${snapshot.playback.warningCount}")
            appendLine("playbackErrors=${snapshot.playback.errorCount}")
            appendLine("playbackLastEvent=${snapshot.playback.lastEventEpochMs ?: "none"}")
            appendLine()
            appendLine("--- Server/Auth Summary ---")
            appendLine("serverConfigured=${snapshot.server.configured}")
            appendLine("serverOrigin=${snapshot.server.canonicalOrigin ?: "unknown"}")
            appendLine("serverHash=${snapshot.server.canonicalUrlHash ?: "none"}")
            appendLine("accessTokenPresent=${snapshot.auth.accessTokenPresent}")
            appendLine("refreshTokenPresent=${snapshot.auth.refreshTokenPresent}")
            appendLine("sessionPresent=${snapshot.auth.sessionPresent}")
            appendLine("deviceSessionPresent=${snapshot.auth.deviceSessionPresent}")
            appendLine("userIdHash=${snapshot.auth.userIdHash ?: "none"}")
            appendLine("requiresPinSetup=${snapshot.auth.requiresPinSetup}")
            snapshot.cache?.let { cache ->
                appendLine()
                appendLine("--- Cache Summary ---")
                cache.library?.let { library ->
                    appendLine("libraryScope=${library.scopeDirectoryName}")
                    appendLine("libraryBytes=${library.approximateBytes}")
                    appendLine("knownLibraries=${library.knownLibraryCount ?: "unknown"}")
                    appendLine("movieBatchFiles=${library.cachedMovieBatchFiles}")
                    appendLine("seriesBundleFiles=${library.cachedSeriesBundleFiles}")
                    appendLine("quarantinedLibraryFiles=${library.quarantineFileCount}")
                    appendLine("libraryStaleOffline=${library.staleOfflineMarkerPresent}")
                }
                cache.image?.let { image ->
                    appendLine("imageBytes=${image.approximateBytes}")
                    appendLine("imageManifestFiles=${image.manifestEntryFiles}")
                    appendLine("imageCoilBlobBytes=${image.coilBlobBytes}")
                    appendLine("quarantinedImageFiles=${image.quarantineFileCount}")
                    appendLine("imageStaleOffline=${image.staleOfflineMarkerPresent}")
                }
            }
            appendLine()
            appendLine("--- Throwable ---")
            appendLine(DiagnosticsRedactor.redactThrowable(throwable, maxChars = maxCrashFileBytes))
            appendLine()
            appendLine("--- Recent Redacted Diagnostic Log ---")
            recentEntries.takeLast(200).forEach { entry -> appendLine(entry.format()) }
        },
    )

    private fun uniqueCrashFile(timestampMs: Long): File {
        val base = "crash-${timestampMs.toString().padStart(13, '0')}"
        var candidate = File(crashDir, "$base.txt")
        var index = 1
        while (candidate.exists()) {
            candidate = File(crashDir, "$base-$index.txt")
            index += 1
        }
        return candidate
    }

    private fun pruneOldCrashes() {
        if (maxCrashFiles <= 0) {
            crashDir.deleteRecursively()
            crashDir.mkdirs()
            return
        }
        retainedCrashFiles().drop(maxCrashFiles).forEach { it.delete() }
    }

    private fun String.toBoundedUtf8(maxBytes: Int): ByteArray {
        val safeMaxBytes = maxBytes.coerceAtLeast(0)
        val bytes = toByteArray(StandardCharsets.UTF_8)
        if (bytes.size <= safeMaxBytes) return bytes
        val marker = "\n--- crash report truncated to $safeMaxBytes bytes ---\n".toByteArray(StandardCharsets.UTF_8)
        if (safeMaxBytes <= marker.size) return marker.copyOf(safeMaxBytes)
        val prefix = bytes.copyOf(safeMaxBytes - marker.size)
        return prefix + marker
    }

    companion object {
        const val DEFAULT_MAX_CRASH_FILES = 8
        const val DEFAULT_MAX_CRASH_FILE_BYTES = 128 * 1024
    }
}
