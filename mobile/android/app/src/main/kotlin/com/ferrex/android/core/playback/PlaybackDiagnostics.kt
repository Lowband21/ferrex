package com.ferrex.android.core.playback

import java.util.concurrent.ConcurrentLinkedDeque

/**
 * Small in-process playback diagnostic buffer. All entries are redacted before
 * storage so stream tickets, session tokens, and bearer values are never kept in
 * crash/debug surfaces.
 */
object PlaybackDiagnosticLog {
    private const val MAX_ENTRIES = 200
    private val entries = ConcurrentLinkedDeque<Entry>()

    data class Entry(
        val timestampMs: Long,
        val level: Level,
        val tag: String,
        val message: String,
        val throwable: String? = null,
    )

    enum class Level { Debug, Info, Warn, Error }

    fun debug(tag: String, message: String, throwable: Throwable? = null) = record(Level.Debug, tag, message, throwable)
    fun info(tag: String, message: String, throwable: Throwable? = null) = record(Level.Info, tag, message, throwable)
    fun warn(tag: String, message: String, throwable: Throwable? = null) = record(Level.Warn, tag, message, throwable)
    fun error(tag: String, message: String, throwable: Throwable? = null) = record(Level.Error, tag, message, throwable)

    fun recentEntries(limit: Int = MAX_ENTRIES): List<Entry> {
        val snapshot = entries.toList()
        return if (snapshot.size <= limit) snapshot else snapshot.takeLast(limit)
    }

    fun clearForTests() {
        entries.clear()
    }

    fun redact(value: String): String = value
        .replace(BEARER_PATTERN) { match -> "${match.groupValues[1]}<redacted>" }
        .replace(QUERY_TOKEN_PATTERN) { match -> "${match.groupValues[1]}=<redacted>" }
        .replace(JSON_TOKEN_PATTERN) { match -> "${match.groupValues[1]}<redacted>${match.groupValues[2]}" }

    private fun record(level: Level, tag: String, message: String, throwable: Throwable?) {
        entries.addLast(
            Entry(
                timestampMs = System.currentTimeMillis(),
                level = level,
                tag = tag,
                message = redact(message),
                throwable = throwable?.stackTraceToString()?.take(2000)?.let(::redact),
            ),
        )
        while (entries.size > MAX_ENTRIES) {
            entries.pollFirst()
        }
    }

    private val BEARER_PATTERN = Regex("(?i)(\\bBearer\\s+)[^\\s,;]+")
    private val QUERY_TOKEN_PATTERN = Regex("(?i)(\\b(?:access_token|refresh_token|token|ticket))=[^\\s&#]+")
    private val JSON_TOKEN_PATTERN = Regex("(?i)(\\\"(?:access_token|refresh_token|token|ticket)\\\"\\s*:\\s*\\\")[^\\\"]+(\\\")")
}
