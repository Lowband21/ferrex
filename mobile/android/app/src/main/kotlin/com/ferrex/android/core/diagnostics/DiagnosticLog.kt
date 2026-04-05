package com.ferrex.android.core.diagnostics

import android.util.Log
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.ConcurrentLinkedDeque

/**
 * Lightweight structured logger that mirrors to logcat AND retains the
 * last [MAX_ENTRIES] messages in a lock-free ring buffer.
 *
 * Usage:
 * ```
 * DiagnosticLog.i("Player", "Buffered 12.4s in 340ms")
 * DiagnosticLog.e("Player", "Stream died", exception)
 * ```
 *
 * After a crash the retained entries can be dumped to disk via [dumpToFile]
 * so the user (or a future bug-report screen) can see what led up to it.
 */
object DiagnosticLog {

    private const val MAX_ENTRIES = 500

    enum class Level { DEBUG, INFO, WARN, ERROR }

    data class Entry(
        val timestampMs: Long,
        val tag: String,
        val level: Level,
        val message: String,
        val throwable: String?, // Serialised stack trace, null if no error
    ) {
        fun format(): String {
            val ts = DATE_FMT.get()!!.format(Date(timestampMs))
            val lvl = level.name.first()
            val err = if (throwable != null) "\n  $throwable" else ""
            return "$ts $lvl/$tag: $message$err"
        }
    }

    // ConcurrentLinkedDeque gives lock-free append; we trim from the head.
    private val entries = ConcurrentLinkedDeque<Entry>()

    private val DATE_FMT = object : ThreadLocal<SimpleDateFormat>() {
        override fun initialValue() =
            SimpleDateFormat("HH:mm:ss.SSS", Locale.US)
    }

    // ── Public API ──────────────────────────────────────────────

    fun d(tag: String, message: String) = log(Level.DEBUG, tag, message, null)
    fun i(tag: String, message: String) = log(Level.INFO, tag, message, null)
    fun w(tag: String, message: String, error: Throwable? = null) = log(Level.WARN, tag, message, error)
    fun e(tag: String, message: String, error: Throwable? = null) = log(Level.ERROR, tag, message, error)

    fun log(level: Level, tag: String, message: String, error: Throwable?) {
        // Always mirror to logcat
        when (level) {
            Level.DEBUG -> if (error != null) Log.d(tag, message, error) else Log.d(tag, message)
            Level.INFO  -> if (error != null) Log.i(tag, message, error) else Log.i(tag, message)
            Level.WARN  -> if (error != null) Log.w(tag, message, error) else Log.w(tag, message)
            Level.ERROR -> if (error != null) Log.e(tag, message, error) else Log.e(tag, message)
        }

        val entry = Entry(
            timestampMs = System.currentTimeMillis(),
            tag = tag,
            level = level,
            message = message,
            throwable = error?.stackTraceToString()?.take(2000), // cap trace size
        )
        entries.addLast(entry)

        // Trim from the head if we exceed capacity
        while (entries.size > MAX_ENTRIES) {
            entries.pollFirst()
        }
    }

    /** Return the most recent [count] entries, oldest first. */
    fun recentEntries(count: Int = MAX_ENTRIES): List<Entry> {
        val all = entries.toList()
        return if (all.size <= count) all else all.takeLast(count)
    }

    /**
     * Dump all retained entries to [file].
     * Safe to call from a crash handler — no allocations beyond the write buffer.
     */
    fun dumpToFile(file: File) {
        try {
            file.parentFile?.mkdirs()
            file.bufferedWriter().use { writer ->
                writer.write("=== Ferrex Diagnostic Log ===\n")
                writer.write("Dumped: ${Date()}\n")
                writer.write("Entries: ${entries.size}\n\n")
                for (entry in entries) {
                    writer.write(entry.format())
                    writer.write("\n")
                }
            }
        } catch (_: Exception) {
            // Last resort — don't throw from the crash handler path
        }
    }
}
