package com.ferrex.android.core.library

import android.content.Context
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.StringWriter
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.channels.FileChannel
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Properties

/**
 * Atomic disk layout for server/user-scoped library metadata and payloads.
 *
 * Layout:
 * library_cache/v1/scopes/<scope>/
 *   scope.properties
 *   libraries/list.fb
 *   libraries/<library-id>/movies/versions.properties
 *   libraries/<library-id>/movies/batches/<batch-id>.fb
 *   libraries/<library-id>/series/versions.properties
 *   libraries/<library-id>/series/bundles/<series-id>.fb
 *   metadata/stale-offline.properties
 *   images/
 *   search/
 *   quarantine/<payload>.fb + <payload>.properties
 */
class LibraryDiskCache(
    private val rootDir: File,
) {
    init {
        rootDir.mkdirs()
    }

    fun writeLibraryList(scope: ServerCacheScope, bytes: ByteArray) {
        ensureScope(scope)
        writeBytesAtomically(libraryListFile(scope), bytes)
    }

    fun readLibraryList(scope: ServerCacheScope): CachedPayload<Unit>? {
        val file = libraryListFile(scope)
        if (!file.exists()) return null
        return CachedPayload(Unit, 0L, file, memoryMap(file))
    }

    fun quarantineLibraryList(scope: ServerCacheScope, reason: String): File? {
        val file = libraryListFile(scope)
        if (!file.exists()) return null
        return quarantineFile(scope, category = "libraries", libraryId = "list", itemId = "metadata", file = file, reason = reason)
    }

    fun cachedMovieBatchVersions(scope: ServerCacheScope, libraryId: String): Map<Int, Long> =
        readVersions(movieVersionsFile(scope, libraryId)).mapKeysNotNull { key -> key.toIntOrNull() }

    fun writeMovieBatch(scope: ServerCacheScope, libraryId: String, batchId: Int, version: Long, bytes: ByteArray) {
        ensureScope(scope)
        writeBytesAtomically(movieBatchFile(scope, libraryId, batchId), bytes)
        val versions = cachedMovieBatchVersions(scope, libraryId).toMutableMap()
        versions[batchId] = version
        writeVersions(movieVersionsFile(scope, libraryId), versions.mapKeys { it.key.toString() })
    }

    fun readMovieBatchPayloads(scope: ServerCacheScope, libraryId: String): List<CachedPayload<Int>> {
        val versions = cachedMovieBatchVersions(scope, libraryId)
        val dir = movieBatchDir(scope, libraryId)
        if (!dir.exists()) return emptyList()
        return dir.listFiles { file -> file.isFile && file.extension == "fb" }
            ?.mapNotNull { file ->
                val batchId = file.nameWithoutExtension.toIntOrNull() ?: return@mapNotNull null
                CachedPayload(batchId, versions[batchId] ?: 0L, file, memoryMap(file))
            }
            ?.sortedBy { it.id }
            .orEmpty()
    }

    fun deleteMovieBatches(scope: ServerCacheScope, libraryId: String, batchIds: Collection<Int>) {
        if (batchIds.isEmpty()) return
        val versions = cachedMovieBatchVersions(scope, libraryId).toMutableMap()
        batchIds.forEach { batchId ->
            movieBatchFile(scope, libraryId, batchId).delete()
            versions.remove(batchId)
        }
        writeVersions(movieVersionsFile(scope, libraryId), versions.mapKeys { it.key.toString() })
    }

    fun quarantineMovieBatch(scope: ServerCacheScope, libraryId: String, batchId: Int, reason: String): File? {
        val file = movieBatchFile(scope, libraryId, batchId)
        if (!file.exists()) return null
        val quarantined = quarantineFile(scope, "movie-batch", libraryId, batchId.toString(), file, reason)
        deleteMovieBatches(scope, libraryId, listOf(batchId))
        return quarantined
    }

    fun cachedSeriesBundleVersions(scope: ServerCacheScope, libraryId: String): Map<String, Long> =
        readVersions(seriesVersionsFile(scope, libraryId))

    fun writeSeriesBundle(scope: ServerCacheScope, libraryId: String, seriesId: String, version: Long, bytes: ByteArray) {
        ensureScope(scope)
        writeBytesAtomically(seriesBundleFile(scope, libraryId, seriesId), bytes)
        val versions = cachedSeriesBundleVersions(scope, libraryId).toMutableMap()
        versions[seriesId] = version
        writeVersions(seriesVersionsFile(scope, libraryId), versions)
    }

    fun readSeriesBundlePayloads(scope: ServerCacheScope, libraryId: String): List<CachedPayload<String>> {
        val versions = cachedSeriesBundleVersions(scope, libraryId)
        val dir = seriesBundleDir(scope, libraryId)
        if (!dir.exists()) return emptyList()
        return dir.listFiles { file -> file.isFile && file.extension == "fb" }
            ?.map { file -> CachedPayload(file.nameWithoutExtension, versions[file.nameWithoutExtension] ?: 0L, file, memoryMap(file)) }
            ?.sortedBy { it.id }
            .orEmpty()
    }

    fun deleteSeriesBundles(scope: ServerCacheScope, libraryId: String, seriesIds: Collection<String>) {
        if (seriesIds.isEmpty()) return
        val versions = cachedSeriesBundleVersions(scope, libraryId).toMutableMap()
        seriesIds.forEach { seriesId ->
            seriesBundleFile(scope, libraryId, seriesId).delete()
            versions.remove(seriesId)
        }
        writeVersions(seriesVersionsFile(scope, libraryId), versions)
    }

    fun quarantineSeriesBundle(scope: ServerCacheScope, libraryId: String, seriesId: String, reason: String): File? {
        val file = seriesBundleFile(scope, libraryId, seriesId)
        if (!file.exists()) return null
        val quarantined = quarantineFile(scope, "series-bundle", libraryId, seriesId, file, reason)
        deleteSeriesBundles(scope, libraryId, listOf(seriesId))
        return quarantined
    }

    fun markStaleOffline(scope: ServerCacheScope, libraryId: String?, message: String) {
        ensureScope(scope)
        val properties = Properties().apply {
            setProperty("state", "stale-offline")
            setProperty("library_id", libraryId.orEmpty())
            setProperty("message", message)
            setProperty("updated_at_millis", System.currentTimeMillis().toString())
        }
        writePropertiesAtomically(staleOfflineFile(scope), properties)
    }

    fun staleOfflineMetadataExists(scope: ServerCacheScope): Boolean = staleOfflineFile(scope).exists()

    fun clearSelectedLibrary(scope: ServerCacheScope, libraryId: String) {
        libraryDir(scope, libraryId).deleteRecursively()
    }

    fun clearAllForScope(scope: ServerCacheScope) {
        scopeDir(scope).deleteRecursively()
        ensureScope(scope)
    }

    fun debugScopeDir(scope: ServerCacheScope): File {
        ensureScope(scope)
        return scopeDir(scope)
    }

    fun quarantinedFiles(scope: ServerCacheScope): List<File> {
        val dir = quarantineDir(scope)
        return dir.listFiles { file -> file.isFile && file.extension == "fb" }?.toList().orEmpty()
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

    private fun librariesDir(scope: ServerCacheScope): File = File(scopeDir(scope), "libraries")

    private fun libraryListFile(scope: ServerCacheScope): File = File(librariesDir(scope), "list.fb")

    private fun libraryDir(scope: ServerCacheScope, libraryId: String): File = File(librariesDir(scope), safeName(libraryId))

    private fun movieDir(scope: ServerCacheScope, libraryId: String): File = File(libraryDir(scope, libraryId), "movies")

    private fun movieBatchDir(scope: ServerCacheScope, libraryId: String): File = File(movieDir(scope, libraryId), "batches")

    private fun movieBatchFile(scope: ServerCacheScope, libraryId: String, batchId: Int): File =
        File(movieBatchDir(scope, libraryId), "$batchId.fb")

    private fun movieVersionsFile(scope: ServerCacheScope, libraryId: String): File = File(movieDir(scope, libraryId), "versions.properties")

    private fun seriesDir(scope: ServerCacheScope, libraryId: String): File = File(libraryDir(scope, libraryId), "series")

    private fun seriesBundleDir(scope: ServerCacheScope, libraryId: String): File = File(seriesDir(scope, libraryId), "bundles")

    private fun seriesBundleFile(scope: ServerCacheScope, libraryId: String, seriesId: String): File =
        File(seriesBundleDir(scope, libraryId), "${safeName(seriesId)}.fb")

    private fun seriesVersionsFile(scope: ServerCacheScope, libraryId: String): File = File(seriesDir(scope, libraryId), "versions.properties")

    private fun metadataDir(scope: ServerCacheScope): File = File(scopeDir(scope), "metadata")

    private fun staleOfflineFile(scope: ServerCacheScope): File = File(metadataDir(scope), "stale-offline.properties")

    private fun quarantineDir(scope: ServerCacheScope): File = File(scopeDir(scope), "quarantine")

    private fun quarantineFile(
        scope: ServerCacheScope,
        category: String,
        libraryId: String,
        itemId: String,
        file: File,
        reason: String,
    ): File {
        ensureScope(scope)
        val dir = quarantineDir(scope).also { it.mkdirs() }
        val target = File(
            dir,
            "${System.currentTimeMillis()}-${safeName(category)}-${safeName(libraryId)}-${safeName(itemId)}.fb",
        )
        moveFile(file, target)
        val properties = Properties().apply {
            setProperty("category", category)
            setProperty("library_id", libraryId)
            setProperty("item_id", itemId)
            setProperty("reason", reason)
            setProperty("created_at_millis", System.currentTimeMillis().toString())
        }
        writePropertiesAtomically(File(dir, "${target.name}.properties"), properties)
        return target
    }

    private fun memoryMap(file: File): ByteBuffer {
        if (file.length() <= 0L) throw IOException("Cache file is empty: ${file.name}")
        FileInputStream(file).use { input ->
            val channel = input.channel
            return channel.map(FileChannel.MapMode.READ_ONLY, 0, channel.size())
                .order(ByteOrder.LITTLE_ENDIAN)
        }
    }

    private fun readVersions(file: File): Map<String, Long> {
        if (!file.exists()) return emptyMap()
        val properties = Properties()
        FileInputStream(file).use { properties.load(it) }
        return properties.stringPropertyNames().mapNotNull { key ->
            val version = properties.getProperty(key).toLongOrNull() ?: return@mapNotNull null
            key to version
        }.toMap()
    }

    private fun writeVersions(file: File, versions: Map<String, Long>) {
        val properties = Properties()
        versions.toSortedMap().forEach { (id, version) -> properties.setProperty(id, version.toString()) }
        writePropertiesAtomically(file, properties)
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
            Files.move(
                source.toPath(),
                target.toPath(),
                StandardCopyOption.ATOMIC_MOVE,
                StandardCopyOption.REPLACE_EXISTING,
            )
        } catch (_: AtomicMoveNotSupportedException) {
            Files.move(source.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING)
        }
    }

    private fun safeName(value: String): String = value.replace(Regex("[^A-Za-z0-9._-]"), "_")

    private fun <K> Map<String, Long>.mapKeysNotNull(transform: (String) -> K?): Map<K, Long> =
        entries.mapNotNull { (key, value) -> transform(key)?.let { it to value } }.toMap()

    companion object {
        fun fromContext(context: Context): LibraryDiskCache = LibraryDiskCache(File(context.filesDir, "library_cache"))
    }
}

data class CachedPayload<T>(
    val id: T,
    val version: Long,
    val file: File,
    val bytes: ByteBuffer,
)
