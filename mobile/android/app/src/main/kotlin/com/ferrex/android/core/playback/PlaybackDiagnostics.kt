package com.ferrex.android.core.playback

import com.ferrex.android.core.diagnostics.DiagnosticLog
import com.ferrex.android.core.diagnostics.DiagnosticsRedactor

/**
 * Playback-facing adapter for the retained diagnostics core. Playback entries
 * are redacted by the shared diagnostics redactor before storage/export so
 * stream tickets, session tokens, bearer values, and URLs are never retained raw.
 */
object PlaybackDiagnosticLog {
    private const val MAX_ENTRIES = 200

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

    fun recentEntries(limit: Int = MAX_ENTRIES): List<Entry> = DiagnosticLog
        .recentEntries(limit = limit, source = DiagnosticLog.Source.Playback)
        .map { entry ->
            Entry(
                timestampMs = entry.timestampMs,
                level = entry.level.toPlaybackLevel(),
                tag = entry.tag,
                message = entry.message,
                throwable = entry.throwable,
            )
        }

    fun clearForTests() {
        DiagnosticLog.clear()
    }

    fun redact(value: String): String = DiagnosticsRedactor.redactText(value)

    private fun record(level: Level, tag: String, message: String, throwable: Throwable?) {
        DiagnosticLog.record(
            level = level.toDiagnosticLevel(),
            tag = tag,
            message = message,
            throwable = throwable,
            source = DiagnosticLog.Source.Playback,
        )
    }

    private fun Level.toDiagnosticLevel(): DiagnosticLog.Level = when (this) {
        Level.Debug -> DiagnosticLog.Level.Debug
        Level.Info -> DiagnosticLog.Level.Info
        Level.Warn -> DiagnosticLog.Level.Warn
        Level.Error -> DiagnosticLog.Level.Error
    }

    private fun DiagnosticLog.Level.toPlaybackLevel(): Level = when (this) {
        DiagnosticLog.Level.Debug -> Level.Debug
        DiagnosticLog.Level.Info -> Level.Info
        DiagnosticLog.Level.Warn -> Level.Warn
        DiagnosticLog.Level.Error -> Level.Error
    }
}
