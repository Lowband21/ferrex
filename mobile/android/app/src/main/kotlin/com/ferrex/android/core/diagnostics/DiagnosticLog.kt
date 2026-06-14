package com.ferrex.android.core.diagnostics

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.ConcurrentLinkedDeque

/**
 * In-process diagnostic log retained for crash reports and user-exportable
 * bundles. Entries are redacted before they enter the ring buffer.
 */
object DiagnosticLog {
    const val DEFAULT_MAX_ENTRIES = 500

    enum class Level { Debug, Info, Warn, Error }

    enum class Source { App, Playback, Crash, Diagnostics }

    data class Entry(
        val timestampMs: Long,
        val level: Level,
        val tag: String,
        val message: String,
        val throwable: String? = null,
        val source: Source = Source.App,
    ) {
        fun format(): String {
            val ts = DATE_FORMAT.get()!!.format(Date(timestampMs))
            val levelChar = level.name.first()
            val throwableText = throwable?.let { "\n  $it" }.orEmpty()
            return "$ts $levelChar/$source/$tag: $message$throwableText"
        }
    }

    private val entries = ConcurrentLinkedDeque<Entry>()

    fun debug(tag: String, message: String, throwable: Throwable? = null, source: Source = Source.App) =
        record(Level.Debug, tag, message, throwable, source)

    fun info(tag: String, message: String, throwable: Throwable? = null, source: Source = Source.App) =
        record(Level.Info, tag, message, throwable, source)

    fun warn(tag: String, message: String, throwable: Throwable? = null, source: Source = Source.App) =
        record(Level.Warn, tag, message, throwable, source)

    fun error(tag: String, message: String, throwable: Throwable? = null, source: Source = Source.App) =
        record(Level.Error, tag, message, throwable, source)

    fun record(
        level: Level,
        tag: String,
        message: String,
        throwable: Throwable? = null,
        source: Source = Source.App,
        timestampMs: Long = System.currentTimeMillis(),
    ) {
        val entry = Entry(
            timestampMs = timestampMs,
            level = level,
            tag = DiagnosticsRedactor.redactText(tag).take(80),
            message = DiagnosticsRedactor.redactText(message),
            throwable = throwable?.let { DiagnosticsRedactor.redactThrowable(it, maxChars = 4_000) },
            source = source,
        )
        entries.addLast(entry)
        trimTo(DEFAULT_MAX_ENTRIES)
    }

    fun appendRedacted(entry: Entry) {
        entries.addLast(
            entry.copy(
                tag = DiagnosticsRedactor.redactText(entry.tag).take(80),
                message = DiagnosticsRedactor.redactText(entry.message),
                throwable = entry.throwable?.let(DiagnosticsRedactor::redactText),
            ),
        )
        trimTo(DEFAULT_MAX_ENTRIES)
    }

    fun recentEntries(limit: Int = DEFAULT_MAX_ENTRIES, source: Source? = null): List<Entry> {
        val boundedLimit = limit.coerceAtLeast(0)
        if (boundedLimit == 0) return emptyList()
        val snapshot = entries.toList().let { all -> source?.let { src -> all.filter { it.source == src } } ?: all }
        return if (snapshot.size <= boundedLimit) snapshot else snapshot.takeLast(boundedLimit)
    }

    fun clear() {
        entries.clear()
    }

    fun render(entries: List<Entry> = recentEntries()): String = buildString {
        appendLine("=== Ferrex Diagnostic Log ===")
        appendLine("entries=${entries.size}")
        appendLine()
        entries.forEach { entry -> appendLine(entry.format()) }
    }

    private fun trimTo(maxEntries: Int) {
        while (entries.size > maxEntries) {
            entries.pollFirst()
        }
    }

    private val DATE_FORMAT = object : ThreadLocal<SimpleDateFormat>() {
        override fun initialValue(): SimpleDateFormat = SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS", Locale.US)
    }
}
