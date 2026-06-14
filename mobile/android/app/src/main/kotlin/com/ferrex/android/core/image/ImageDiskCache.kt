package com.ferrex.android.core.image

import android.content.Context
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

    fun markStaleOffline(scope: ServerCacheScope, message: String) {
        ensureScope(scope)
        val properties = Properties().apply {
            setProperty("state", "stale-offline")
            setProperty("message", message)
            setProperty("updated_at_millis", System.currentTimeMillis().toString())
        }
        writePropertiesAtomically(File(imagesDir(scope), "stale-offline.properties"), properties)
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
        val coilFiles = coilDiskCacheDir(scope).let { dir ->
            if (dir.exists()) dir.walkTopDown().filter { it.isFile }.toList() else emptyList()
        }
        return ImageDiskCacheDiagnostics(
            scopeDirectoryName = scope.directoryName,
            relativeImagesPath = "library_cache/v1/scopes/${scope.directoryName}/images",
            approximateBytes = files.sumOf { it.length() },
            manifestEntryFiles = files.count { file -> file.extension == "properties" && file.parentFile?.name == "manifest" },
            coilBlobBytes = coilFiles.sumOf { it.length() },
            quarantineFileCount = files.count { file -> file.parentFile?.name == "quarantine" && !file.name.endsWith(".reason.properties") },
            staleOfflineMarkerPresent = File(imagesDir(scope), "stale-offline.properties").exists(),
        )
    }

    fun quarantinedManifestFiles(scope: ServerCacheScope): List<File> {
        val dir = quarantineDir(scope)
        return dir.listFiles { file ->
            file.isFile && file.extension == "properties" && !file.name.endsWith(".reason.properties")
        }?.toList().orEmpty()
    }

    private fun readManifestProperties(expectedKey: ImageRequestKey, file: File): ImageManifestRecord {
        val properties = Properties()
        FileInputStream(file).use { properties.load(it) }
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

data class ImageDiskCacheDiagnostics(
    val scopeDirectoryName: String,
    val relativeImagesPath: String,
    val approximateBytes: Long,
    val manifestEntryFiles: Int,
    val coilBlobBytes: Long,
    val quarantineFileCount: Int,
    val staleOfflineMarkerPresent: Boolean,
)

sealed interface ManifestCacheRead {
    data class Valid(val record: ImageManifestRecord) : ManifestCacheRead
    data object Missing : ManifestCacheRead
    data class Corrupt(val message: String, val quarantinedFile: File) : ManifestCacheRead
}
