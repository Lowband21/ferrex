package com.ferrex.android.core.diagnostics

import android.content.Context
import android.util.Log
import java.io.File
import java.io.PrintWriter
import java.io.StringWriter
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Installs a global [Thread.UncaughtExceptionHandler] that:
 *
 * 1. Writes the crash stack trace + recent [DiagnosticLog] entries to
 *    `{filesDir}/crashes/crash-{timestamp}.txt`
 * 2. Logs the crash to logcat at ERROR level
 * 3. Delegates to the previous handler so the default Android crash
 *    dialog / process-kill still happens
 *
 * Call [install] once from [android.app.Application.onCreate].
 */
object CrashHandler {

    private const val TAG = "CrashHandler"
    private const val MAX_CRASH_FILES = 10

    fun install(context: Context) {
        val appContext = context.applicationContext
        val previous = Thread.getDefaultUncaughtExceptionHandler()

        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            try {
                writeCrashFile(appContext, thread, throwable)
            } catch (e: Exception) {
                // Absolute last resort — the crash handler itself crashed.
                Log.e(TAG, "Failed to write crash file", e)
            }

            // Let the system handle the rest (show dialog, kill process)
            previous?.uncaughtException(thread, throwable)
        }

        DiagnosticLog.i(TAG, "Crash handler installed")
    }

    /**
     * Returns the crash directory, creating it if needed.
     */
    fun crashDir(context: Context): File =
        File(context.filesDir, "crashes").also { it.mkdirs() }

    /**
     * Returns crash files sorted newest-first.
     */
    fun listCrashFiles(context: Context): List<File> =
        crashDir(context)
            .listFiles { f -> f.extension == "txt" }
            ?.sortedByDescending { it.lastModified() }
            ?: emptyList()

    private fun writeCrashFile(context: Context, thread: Thread, throwable: Throwable) {
        val dir = crashDir(context)
        val timestamp = SimpleDateFormat("yyyyMMdd-HHmmss-SSS", Locale.US).format(Date())
        val file = File(dir, "crash-$timestamp.txt")

        file.bufferedWriter().use { w ->
            w.write("=== Ferrex Crash Report ===\n")
            w.write("Time:   ${Date()}\n")
            w.write("Thread: ${thread.name} (id=${thread.id})\n\n")

            // The exception + full stack trace
            w.write("--- Exception ---\n")
            val sw = StringWriter()
            throwable.printStackTrace(PrintWriter(sw))
            w.write(sw.toString())
            w.write("\n")

            // Runtime context
            w.write("--- Runtime ---\n")
            val rt = Runtime.getRuntime()
            val maxMb = rt.maxMemory() / (1024 * 1024)
            val totalMb = rt.totalMemory() / (1024 * 1024)
            val freeMb = rt.freeMemory() / (1024 * 1024)
            val usedMb = totalMb - freeMb
            w.write("Heap: used=${usedMb}MB, total=${totalMb}MB, max=${maxMb}MB\n")
            w.write("Available processors: ${rt.availableProcessors()}\n\n")

            // Recent diagnostic log entries (the trail leading up to the crash)
            w.write("--- Recent DiagnosticLog (last 200 entries) ---\n")
            for (entry in DiagnosticLog.recentEntries(200)) {
                w.write(entry.format())
                w.write("\n")
            }
        }

        Log.e(TAG, "Crash file written: ${file.absolutePath}")

        // Prune old crash files
        val files = listCrashFiles(context)
        if (files.size > MAX_CRASH_FILES) {
            files.drop(MAX_CRASH_FILES).forEach { it.delete() }
        }
    }
}
