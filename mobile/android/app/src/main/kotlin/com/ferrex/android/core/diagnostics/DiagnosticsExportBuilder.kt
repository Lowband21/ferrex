package com.ferrex.android.core.diagnostics

import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.io.File
import java.nio.charset.StandardCharsets
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

class DiagnosticsExportBuilder(
    private val files: DiagnosticsFiles,
    private val crashStore: CrashRetentionStore,
    private val clockMillis: () -> Long = { System.currentTimeMillis() },
) {
    fun build(snapshot: DiagnosticsSnapshot): File {
        files.exportDir.mkdirs()
        val generatedAt = clockMillis()
        val logEntries = DiagnosticLog.recentEntries()
        val crashes = crashStore.retainedCrashFiles().sortedBy { it.name }
        val zipEntries = buildList {
            add("manifest.json")
            add("diagnostics.txt")
            add("logs/diagnostic-log.txt")
            crashes.forEach { crash -> add("crashes/${crash.name}") }
        }
        val manifest = DiagnosticsBundleManifest(
            generatedAtEpochMs = generatedAt,
            snapshot = snapshot.copy(generatedAtEpochMs = generatedAt),
            retainedLogCount = logEntries.size,
            retainedCrashFiles = crashes.map { crash ->
                RetainedCrashDiagnosticsFile(
                    name = crash.name,
                    sizeBytes = crash.length(),
                    lastModifiedEpochMs = crash.lastModified(),
                )
            },
            files = zipEntries,
        )
        val target = uniqueExportFile(generatedAt)
        ZipOutputStream(target.outputStream().buffered()).use { zip ->
            zip.putText("manifest.json", JSON.encodeToString(manifest))
            zip.putText("diagnostics.txt", renderHumanSummary(manifest))
            zip.putText("logs/diagnostic-log.txt", DiagnosticLog.render(logEntries))
            crashes.forEach { crash ->
                zip.putText("crashes/${crash.name}", crash.readText())
            }
        }
        return target
    }

    private fun renderHumanSummary(manifest: DiagnosticsBundleManifest): String = DiagnosticsRedactor.redactText(
        buildString {
            appendLine("Ferrex Android diagnostics export")
            appendLine("generatedAtEpochMs=${manifest.generatedAtEpochMs}")
            appendLine("applicationId=${manifest.snapshot.app.applicationId}")
            appendLine("versionName=${manifest.snapshot.app.versionName}")
            appendLine("buildType=${manifest.snapshot.app.buildType ?: "unknown"}")
            appendLine("flavor=${manifest.snapshot.app.flavor ?: "unknown"}")
            appendLine("serverConfigured=${manifest.snapshot.server.configured}")
            appendLine("serverOrigin=${manifest.snapshot.server.canonicalOrigin ?: "unknown"}")
            appendLine("serverHash=${manifest.snapshot.server.canonicalUrlHash ?: "none"}")
            appendLine("accessTokenPresent=${manifest.snapshot.auth.accessTokenPresent}")
            appendLine("refreshTokenPresent=${manifest.snapshot.auth.refreshTokenPresent}")
            appendLine("sessionPresent=${manifest.snapshot.auth.sessionPresent}")
            appendLine("deviceSessionPresent=${manifest.snapshot.auth.deviceSessionPresent}")
            appendLine("userIdHash=${manifest.snapshot.auth.userIdHash ?: "none"}")
            appendLine("requiresPinSetup=${manifest.snapshot.auth.requiresPinSetup}")
            appendLine("retainedLogCount=${manifest.retainedLogCount}")
            appendLine("retainedCrashFiles=${manifest.retainedCrashFiles.size}")
            appendLine("playbackEntryCount=${manifest.snapshot.playback.retainedEntryCount}")
            appendLine("playbackWarningCount=${manifest.snapshot.playback.warningCount}")
            appendLine("playbackErrorCount=${manifest.snapshot.playback.errorCount}")
            manifest.snapshot.display?.let { display ->
                appendLine("displayPresent=${display.defaultDisplayPresent}")
                appendLine("displayHdrTypes=${display.hdrTypes.joinToString()}")
                appendLine("windowColorMode=${display.windowColorMode ?: "unknown"}")
            }
            manifest.snapshot.cache?.library?.let { library ->
                appendLine("libraryScope=${library.scopeDirectoryName}")
                appendLine("libraryBytes=${library.approximateBytes}")
                appendLine("cachedMovieBatchFiles=${library.cachedMovieBatchFiles}")
                appendLine("cachedSeriesBundleFiles=${library.cachedSeriesBundleFiles}")
                appendLine("libraryQuarantineFiles=${library.quarantineFileCount}")
            }
            manifest.snapshot.cache?.image?.let { image ->
                appendLine("imageBytes=${image.approximateBytes}")
                appendLine("imageManifestFiles=${image.manifestEntryFiles}")
                appendLine("imageQuarantineFiles=${image.quarantineFileCount}")
            }
            appendLine()
            appendLine("Files:")
            manifest.files.forEach { appendLine("- $it") }
        },
    )

    private fun uniqueExportFile(timestampMs: Long): File {
        val base = "ferrex-diagnostics-${timestampMs.toString().padStart(13, '0')}"
        var candidate = File(files.exportDir, "$base.zip")
        var index = 1
        while (candidate.exists()) {
            candidate = File(files.exportDir, "$base-$index.zip")
            index += 1
        }
        return candidate
    }

    private fun ZipOutputStream.putText(name: String, text: String) {
        val entry = ZipEntry(name).apply { time = 0L }
        putNextEntry(entry)
        write(DiagnosticsRedactor.redactText(text).toByteArray(StandardCharsets.UTF_8))
        closeEntry()
    }

    companion object {
        private val JSON = Json {
            prettyPrint = true
            encodeDefaults = true
        }
    }
}
