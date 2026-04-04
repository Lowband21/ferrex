package com.ferrex.android.core.library

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import java.io.File
import java.io.FileInputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.channels.FileChannel
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Disk-backed FlatBuffer cache.
 *
 * Stores raw FlatBuffer byte arrays from batch responses to app internal
 * storage, keyed by library + batch + version. On read, memory-maps the
 * file via [FileChannel.map] so FlatBuffers reads directly from the OS
 * page cache — zero copies, zero GC pressure on scroll.
 *
 * The wire format IS the cache format. No transformation step.
 */
@Singleton
class LibraryCache @Inject constructor(
    @ApplicationContext private val context: Context,
) {
    private val cacheDir: File
        get() = File(context.filesDir, "library_cache").also { it.mkdirs() }

    // ── Movie batches ───────────────────────────────────────────────

    fun getMovieBatch(libraryId: String, batchId: Int): ByteBuffer? {
        val file = movieBatchFile(libraryId, batchId)
        if (!file.exists()) return null
        return memoryMap(file)
    }

    fun writeMovieBatch(libraryId: String, batchId: Int, data: ByteArray) {
        movieBatchFile(libraryId, batchId).writeBytes(data)
    }

    // ── Series bundles ──────────────────────────────────────────────

    fun getSeriesBundle(libraryId: String, seriesId: String): ByteBuffer? {
        val file = seriesBundleFile(libraryId, seriesId)
        if (!file.exists()) return null
        return memoryMap(file)
    }

    fun writeSeriesBundle(libraryId: String, seriesId: String, data: ByteArray) {
        seriesBundleFile(libraryId, seriesId).writeBytes(data)
    }

    // ── Batch version tracking ──────────────────────────────────────

    /**
     * Returns cached batch versions for a library.
     * Format: Map<batchId, version>
     */
    fun getCachedMovieBatchVersions(libraryId: String): Map<Int, Long> {
        val dir = File(cacheDir, "movies/$libraryId")
        if (!dir.exists()) return emptyMap()

        return dir.listFiles()
            ?.filter { it.extension == "fb" }
            ?.associate { file ->
                val parts = file.nameWithoutExtension.split("_v")
                val batchId = parts.getOrNull(0)?.toIntOrNull() ?: return@associate (-1 to 0L)
                val version = parts.getOrNull(1)?.toLongOrNull() ?: 0L
                batchId to version
            }
            ?.filterKeys { it >= 0 }
            ?: emptyMap()
    }

    fun writeMovieBatchVersioned(libraryId: String, batchId: Int, version: Long, data: ByteArray) {
        // Remove old version files for this batch
        val dir = File(cacheDir, "movies/$libraryId").also { it.mkdirs() }
        dir.listFiles()?.filter { it.name.startsWith("${batchId}_v") }?.forEach { it.delete() }

        File(dir, "${batchId}_v${version}.fb").writeBytes(data)
    }

    fun getMovieBatchVersioned(libraryId: String, batchId: Int): ByteBuffer? {
        val dir = File(cacheDir, "movies/$libraryId")
        if (!dir.exists()) return null
        val file = dir.listFiles()?.firstOrNull { it.name.startsWith("${batchId}_v") }
            ?: return null
        return memoryMap(file)
    }

    // ── Cleanup ─────────────────────────────────────────────────────

    fun clearLibrary(libraryId: String) {
        File(cacheDir, "movies/$libraryId").deleteRecursively()
        File(cacheDir, "series/$libraryId").deleteRecursively()
    }

    fun clearAll() {
        cacheDir.deleteRecursively()
    }

    // ── Private ─────────────────────────────────────────────────────

    private fun movieBatchFile(libraryId: String, batchId: Int): File {
        return File(cacheDir, "movies/$libraryId/$batchId.fb").also {
            it.parentFile?.mkdirs()
        }
    }

    private fun seriesBundleFile(libraryId: String, seriesId: String): File {
        return File(cacheDir, "series/$libraryId/$seriesId.fb").also {
            it.parentFile?.mkdirs()
        }
    }

    /**
     * Memory-map a file for zero-copy FlatBuffer access.
     * The OS page cache handles the actual I/O.
     */
    private fun memoryMap(file: File): ByteBuffer {
        val channel = FileInputStream(file).channel
        return channel.map(FileChannel.MapMode.READ_ONLY, 0, channel.size())
            .order(ByteOrder.LITTLE_ENDIAN)
    }
}
