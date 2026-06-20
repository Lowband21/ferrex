package com.ferrex.android.core.image

import android.content.Context
import com.ferrex.android.core.library.LibrarySyncFailure
import com.ferrex.android.core.library.ServerCacheScope
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.StringWriter
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Properties

/**
 * Server/user-scoped image manifest metadata and Coil blob cache directories.
 *
 * The layout intentionally lives under the same `library_cache/v1/scopes/<scope>`
 * root as LOW-344 so reset/clear recovery paths can remove library and image
 * metadata together without wiping unrelated app data.
 */
class ImageDiskCache(
    private val rootDir: File,
) {
    init {
        rootDir.mkdirs()
    }

    fun writeManifestEntry(scope: ServerCacheScope, record: ImageManifestRecord) {
        ensureScope(scope)
        val properties = Properties().apply {
            setProperty("iid", record.key.iid)
            setProperty("category", record.key.category.wireName)
            setProperty("updated_at_millis", System.currentTimeMillis().toString())
            when (val status = record.status) {
                is ManifestImageStatus.Ready -> {
                    setProperty("status", "ready")
                    setProperty("token", status.token)
                    setProperty("retry_after_millis", "0")
                }
                is ManifestImageStatus.Pending -> {
                    setProperty("status", "pending")
                    setProperty("retry_after_millis", status.retryAfterMillis.toString())
                }
                is ManifestImageStatus.Failed -> {
                    setProperty("status", "failed")
                    setProperty("reason", status.reason)
                    setProperty("retry_after_millis", "0")
                }
            }
        }
        writePropertiesAtomically(manifestFile(scope, record.key), properties)
    }

    fun readManifestEntry(scope: ServerCacheScope, key: ImageRequestKey): ManifestCacheRead {
        val file = manifestFile(scope, key)
        if (!file.exists()) return ManifestCacheRead.Missing
        return try {
            ManifestCacheRead.Valid(readManifestProperties(key, file))
        } catch (e: IllegalArgumentException) {
            val quarantined = quarantineManifestFile(scope, key, file, e.message ?: "Invalid image manifest cache")
            ManifestCacheRead.Corrupt(e.message ?: "Invalid image manifest cache", quarantined)
        } catch (e: IOException) {
            val quarantined = quarantineManifestFile(scope, key, file, e.message ?: "Unreadable image manifest cache")
            ManifestCacheRead.Corrupt(e.message ?: "Unreadable image manifest cache", quarantined)
        }
    }

    fun clearManifestEntries(scope: ServerCacheScope, keys: Collection<ImageRequestKey>) {
        val distinctKeys = keys.distinct()
        if (distinctKeys.isEmpty()) return
        distinctKeys.forEach { key -> manifestFile(scope, key).delete() }
        // Coil disk keys are internal to the image loader. A selected-library clear
        // conservatively drops server-scoped immutable blob files while retaining
        // unrelated library/search metadata, guaranteeing stale image bytes cannot
        // survive a user-visible cache recovery action.
        coilDiskCacheDir(scope).deleteRecursively()
    }

    fun clearAll(scope: ServerCacheScope) {
        imagesDir(scope).deleteRecursively()
    }

    fun clearCoilDiskCache(scope: ServerCacheScope) {
        File(imagesDir(scope), "coil-blobs").deleteRecursively()
    }

    fun recordManifestBatchSuccess(
        scope: ServerCacheScope,
        kind: ImageManifestBatchKind,
        requestedKeyCount: Int,
        records: Collection<ImageManifestRecord>,
    ) {
        val readyCount = records.count { it.status is ManifestImageStatus.Ready }
        val pendingCount = records.count { it.status is ManifestImageStatus.Pending }
        val failedCount = records.count { it.status is ManifestImageStatus.Failed }
        writeManifestBatchDiagnostics(
            scope = scope,
            kind = kind,
            outcome = "success",
            requestedKeyCount = requestedKeyCount,
            responseRecordCount = records.size,
            readyCount = readyCount,
            pendingCount = pendingCount,
            failedCount = failedCount,
        )
    }

    fun recordManifestBatchFailure(
        scope: ServerCacheScope,
        kind: ImageManifestBatchKind,
        requestedKeyCount: Int,
        failure: LibrarySyncFailure,
    ) {
        writeManifestBatchDiagnostics(
            scope = scope,
            kind = kind,
            outcome = "failure",
            requestedKeyCount = requestedKeyCount,
            responseRecordCount = 0,
            readyCount = 0,
            pendingCount = 0,
            failedCount = 0,
            failureKind = failure.diagnosticsKind(),
            failureClassification = failure.classification.name,
            failureHttpCode = (failure as? LibrarySyncFailure.Http)?.code,
        )
    }

    fun markStaleOffline(scope: ServerCacheScope, message: String) {
        ensureScope(scope)
        val properties = Properties().apply {
            setProperty("state", "stale-offline")
            setProperty("message", message)
            setProperty("updated_at_millis", System.currentTimeMillis().toString())
        }
        writePropertiesAtomically(staleOfflineFile(scope), properties)
    }

    fun clearStaleOffline(scope: ServerCacheScope) {
        staleOfflineFile(scope).delete()
    }

    fun coilDiskCacheDir(scope: ServerCacheScope): File {
        ensureScope(scope)
        return File(imagesDir(scope), "coil-blobs").also { it.mkdirs() }
    }

    fun debugManifestFile(scope: ServerCacheScope, key: ImageRequestKey): File = manifestFile(scope, key)

    fun diagnosticSnapshot(scope: ServerCacheScope): ImageDiskCacheDiagnostics {
        ensureScope(scope)
        val images = imagesDir(scope)
        val files = if (images.exists()) images.walkTopDown().filter { it.isFile }.toList() else emptyList()
        val manifestFiles = files.filter { file -> file.extension == "properties" && file.parentFile?.name == "manifest" }
        val quarantineReasonFiles = files.filter { file -> file.parentFile?.name == "quarantine" && file.name.endsWith(".reason.properties") }
        val staleOfflineMarkerPresent = staleOfflineFile(scope).exists()
        val coilFiles = coilDiskCacheDir(scope).let { dir ->
            if (dir.exists()) dir.walkTopDown().filter { it.isFile }.toList() else emptyList()
        }
        return ImageDiskCacheDiagnostics(
            scopeDirectoryName = scope.directoryName,
            relativeImagesPath = "library_cache/v1/scopes/${scope.directoryName}/images",
            approximateBytes = files.sumOf { it.length() },
            manifestEntryFiles = manifestFiles.size,
            coilBlobBytes = coilFiles.sumOf { it.length() },
            quarantineFileCount = files.count { file -> file.parentFile?.name == "quarantine" && !file.name.endsWith(".reason.properties") },
            quarantineReasonFileCount = quarantineReasonFiles.size,
            lastQuarantineEpochMs = quarantineReasonFiles.mapNotNull(::quarantineCreatedAtMillis).maxOrNull(),
            staleOfflineMarkerPresent = staleOfflineMarkerPresent,
            manifestStatus = manifestStatusDiagnostics(manifestFiles, staleOfflineMarkerPresent),
            lastManifestBatch = readManifestBatchDiagnostics(scope),
        )
    }

    fun quarantinedManifestFiles(scope: ServerCacheScope): List<File> {
        val dir = quarantineDir(scope)
        return dir.listFiles { file ->
            file.isFile && file.extension == "properties" && !file.name.endsWith(".reason.properties")
        }?.toList().orEmpty()
    }

    private fun manifestStatusDiagnostics(
        manifestFiles: List<File>,
        staleOfflineMarkerPresent: Boolean,
    ): ImageManifestStatusDiagnostics {
        var readyCount = 0
        var pendingCount = 0
        var failedCount = 0
        var corruptCount = 0
        manifestFiles.forEach { file ->
            val properties = runCatching { readProperties(file) }.getOrNull()
            when (properties?.getProperty("status")) {
                "ready" -> {
                    if (properties.getProperty("token")?.trim()?.isNotEmpty() == true) {
                        readyCount += 1
                    } else {
                        corruptCount += 1
                    }
                }
                "pending" -> {
                    if (properties.getProperty("retry_after_millis")?.toLongOrNull() != null) {
                        pendingCount += 1
                    } else {
                        corruptCount += 1
                    }
                }
                "failed" -> failedCount += 1
                else -> corruptCount += 1
            }
        }
        val staleCount = if (staleOfflineMarkerPresent) readyCount + pendingCount + failedCount else 0
        return ImageManifestStatusDiagnostics(
            readyCount = readyCount,
            pendingCount = pendingCount,
            failedCount = failedCount,
            staleCount = staleCount,
            corruptCount = corruptCount,
        )
    }

    private fun quarantineCreatedAtMillis(file: File): Long? = runCatching {
        readProperties(file).getProperty("created_at_millis")?.toLongOrNull()
    }.getOrNull()

    private fun readManifestBatchDiagnostics(scope: ServerCacheScope): ImageManifestBatchDiagnostics? {
        val file = manifestBatchFile(scope)
        if (!file.exists()) return null
        val properties = runCatching { readProperties(file) }.getOrNull() ?: return null
        return ImageManifestBatchDiagnostics(
            lastOutcome = properties.getProperty("last_outcome")?.takeIf { it.isNotBlank() },
            lastKind = properties.getProperty("last_kind")?.takeIf { it.isNotBlank() },
            lastRequestEpochMs = properties.getProperty("last_request_at_millis")?.toLongOrNull(),
            lastSuccessEpochMs = properties.getProperty("last_success_at_millis")?.toLongOrNull(),
            lastFailureEpochMs = properties.getProperty("last_failure_at_millis")?.toLongOrNull(),
            lastRetryEpochMs = properties.getProperty("last_retry_at_millis")?.toLongOrNull(),
            lastRequestedKeyCount = properties.getProperty("last_requested_key_count")?.toIntOrNull() ?: 0,
            lastResponseRecordCount = properties.getProperty("last_response_record_count")?.toIntOrNull() ?: 0,
            lastReadyCount = properties.getProperty("last_ready_count")?.toIntOrNull() ?: 0,
            lastPendingCount = properties.getProperty("last_pending_count")?.toIntOrNull() ?: 0,
            lastFailedCount = properties.getProperty("last_failed_count")?.toIntOrNull() ?: 0,
            lastFailureKind = properties.getProperty("last_failure_kind")?.takeIf { it.isNotBlank() },
            lastFailureClassification = properties.getProperty("last_failure_classification")?.takeIf { it.isNotBlank() },
            lastFailureHttpCode = properties.getProperty("last_failure_http_code")?.toIntOrNull(),
        )
    }

    private fun writeManifestBatchDiagnostics(
        scope: ServerCacheScope,
        kind: ImageManifestBatchKind,
        outcome: String,
        requestedKeyCount: Int,
        responseRecordCount: Int,
        readyCount: Int,
        pendingCount: Int,
        failedCount: Int,
        failureKind: String? = null,
        failureClassification: String? = null,
        failureHttpCode: Int? = null,
    ) {
        ensureScope(scope)
        val now = System.currentTimeMillis()
        val file = manifestBatchFile(scope)
        val properties = if (file.exists()) runCatching { readProperties(file) }.getOrDefault(Properties()) else Properties()
        properties.apply {
            setProperty("last_outcome", outcome)
            setProperty("last_kind", kind.wireName)
            setProperty("last_request_at_millis", now.toString())
            setProperty("last_requested_key_count", requestedKeyCount.coerceAtLeast(0).toString())
            setProperty("last_response_record_count", responseRecordCount.coerceAtLeast(0).toString())
            setProperty("last_ready_count", readyCount.coerceAtLeast(0).toString())
            setProperty("last_pending_count", pendingCount.coerceAtLeast(0).toString())
            setProperty("last_failed_count", failedCount.coerceAtLeast(0).toString())
            if (outcome == "success") setProperty("last_success_at_millis", now.toString())
            if (kind == ImageManifestBatchKind.Retry) setProperty("last_retry_at_millis", now.toString())
            if (outcome == "failure") {
                setProperty("last_failure_at_millis", now.toString())
                failureKind?.let { setProperty("last_failure_kind", it) }
                failureClassification?.let { setProperty("last_failure_classification", it) }
                if (failureHttpCode != null) {
                    setProperty("last_failure_http_code", failureHttpCode.toString())
                } else {
                    remove("last_failure_http_code")
                }
            }
        }
        writePropertiesAtomically(file, properties)
    }

    private fun LibrarySyncFailure.diagnosticsKind(): String = when (this) {
        is LibrarySyncFailure.Network -> "Network"
        is LibrarySyncFailure.Http -> "Http"
        is LibrarySyncFailure.Parse -> "Parse"
        LibrarySyncFailure.EmptyBody -> "EmptyBody"
    }

    private fun readProperties(file: File): Properties = Properties().apply {
        FileInputStream(file).use { load(it) }
    }

    private fun readManifestProperties(expectedKey: ImageRequestKey, file: File): ImageManifestRecord {
        val properties = readProperties(file)
        val iid = properties.getProperty("iid")?.trim()?.takeIf { it.isNotBlank() }
            ?: throw IllegalArgumentException("Cached image manifest is missing iid")
        val category = BrowseImageCategory.fromWireName(properties.getProperty("category"))
            ?: throw IllegalArgumentException("Cached image manifest has an unknown category")
        val key = ImageRequestKey(iid, category)
        if (key != expectedKey) {
            throw IllegalArgumentException("Cached image manifest key does not match requested image")
        }
        val status = when (properties.getProperty("status")) {
            "ready" -> ManifestImageStatus.Ready(
                properties.getProperty("token")?.trim()?.takeIf { it.isNotBlank() }
                    ?: throw IllegalArgumentException("Cached ready image manifest is missing token"),
            )
            "pending" -> ManifestImageStatus.Pending(
                properties.getProperty("retry_after_millis")?.toLongOrNull()
                    ?: throw IllegalArgumentException("Cached pending image manifest is missing retry timing"),
            )
            "failed" -> ManifestImageStatus.Failed(
                properties.getProperty("reason")?.takeIf { it.isNotBlank() } ?: "Image is not available",
            )
            else -> throw IllegalArgumentException("Cached image manifest has an unknown status")
        }
        return ImageManifestRecord(key, status)
    }

    private fun ensureScope(scope: ServerCacheScope) {
        val dir = scopeDir(scope)
        dir.mkdirs()
        val marker = File(dir, "scope.properties")
        if (!marker.exists()) {
            val properties = Properties().apply {
                setProperty("canonical_server_url", scope.canonicalServerUrl)
                setProperty("user_id", scope.userId.orEmpty())
            }
            writePropertiesAtomically(marker, properties)
        }
    }

    private fun scopeDir(scope: ServerCacheScope): File = File(rootDir, "v1/scopes/${scope.directoryName}")

    private fun imagesDir(scope: ServerCacheScope): File = File(scopeDir(scope), "images")

    private fun staleOfflineFile(scope: ServerCacheScope): File = File(imagesDir(scope), "stale-offline.properties")

    private fun manifestBatchFile(scope: ServerCacheScope): File = File(imagesDir(scope), "manifest-batch.properties")

    private fun manifestDir(scope: ServerCacheScope): File = File(imagesDir(scope), "manifest")

    private fun manifestFile(scope: ServerCacheScope, key: ImageRequestKey): File =
        File(manifestDir(scope), "${safeName(key.cacheKey)}.properties")

    private fun quarantineDir(scope: ServerCacheScope): File = File(imagesDir(scope), "quarantine")

    private fun quarantineManifestFile(
        scope: ServerCacheScope,
        key: ImageRequestKey,
        file: File,
        reason: String,
    ): File {
        ensureScope(scope)
        val dir = quarantineDir(scope).also { it.mkdirs() }
        val target = File(dir, "${System.currentTimeMillis()}-${safeName(key.cacheKey)}.properties")
        moveFile(file, target)
        val properties = Properties().apply {
            setProperty("category", key.category.wireName)
            setProperty("iid", key.iid)
            setProperty("reason", reason)
            setProperty("created_at_millis", System.currentTimeMillis().toString())
        }
        writePropertiesAtomically(File(dir, "${target.name}.reason.properties"), properties)
        return target
    }

    private fun writePropertiesAtomically(file: File, properties: Properties) {
        val writer = StringWriter()
        properties.store(writer, null)
        writeBytesAtomically(file, writer.toString().toByteArray(Charsets.UTF_8))
    }

    private fun writeBytesAtomically(file: File, bytes: ByteArray) {
        file.parentFile?.mkdirs()
        val tmp = File(file.parentFile, ".${file.name}.${System.nanoTime()}.tmp")
        FileOutputStream(tmp).use { output ->
            output.write(bytes)
            output.fd.sync()
        }
        moveFile(tmp, file)
    }

    private fun moveFile(source: File, target: File) {
        target.parentFile?.mkdirs()
        try {
            Files.move(source.toPath(), target.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
        } catch (_: AtomicMoveNotSupportedException) {
            Files.move(source.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING)
        }
    }

    private fun safeName(value: String): String = value.replace(Regex("[^A-Za-z0-9._-]"), "_")

    companion object {
        fun fromContext(context: Context): ImageDiskCache = ImageDiskCache(File(context.filesDir, "library_cache"))
    }
}

enum class ImageManifestBatchKind(val wireName: String) {
    Resolve("resolve"),
    Retry("retry"),
}

data class ImageManifestStatusDiagnostics(
    val readyCount: Int = 0,
    val pendingCount: Int = 0,
    val failedCount: Int = 0,
    val staleCount: Int = 0,
    val corruptCount: Int = 0,
)

data class ImageManifestBatchDiagnostics(
    val lastOutcome: String? = null,
    val lastKind: String? = null,
    val lastRequestEpochMs: Long? = null,
    val lastSuccessEpochMs: Long? = null,
    val lastFailureEpochMs: Long? = null,
    val lastRetryEpochMs: Long? = null,
    val lastRequestedKeyCount: Int = 0,
    val lastResponseRecordCount: Int = 0,
    val lastReadyCount: Int = 0,
    val lastPendingCount: Int = 0,
    val lastFailedCount: Int = 0,
    val lastFailureKind: String? = null,
    val lastFailureClassification: String? = null,
    val lastFailureHttpCode: Int? = null,
)

data class ImageDiskCacheDiagnostics(
    val scopeDirectoryName: String,
    val relativeImagesPath: String,
    val approximateBytes: Long,
    val manifestEntryFiles: Int,
    val coilBlobBytes: Long,
    val quarantineFileCount: Int,
    val staleOfflineMarkerPresent: Boolean,
    val quarantineReasonFileCount: Int = 0,
    val lastQuarantineEpochMs: Long? = null,
    val manifestStatus: ImageManifestStatusDiagnostics = ImageManifestStatusDiagnostics(),
    val lastManifestBatch: ImageManifestBatchDiagnostics? = null,
)

sealed interface ManifestCacheRead {
    data class Valid(val record: ImageManifestRecord) : ManifestCacheRead
    data object Missing : ManifestCacheRead
    data class Corrupt(val message: String, val quarantinedFile: File) : ManifestCacheRead
}
