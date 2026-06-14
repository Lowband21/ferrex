package com.ferrex.android.core.diagnostics

import java.io.File

class DiagnosticsFiles(
    val rootDir: File,
) {
    val crashDir: File get() = File(rootDir, "crashes")
    val exportDir: File get() = File(rootDir, "exports")

    init {
        rootDir.mkdirs()
        crashDir.mkdirs()
        exportDir.mkdirs()
    }

    fun clearCrashAndExportFiles() {
        crashDir.deleteRecursively()
        exportDir.deleteRecursively()
        crashDir.mkdirs()
        exportDir.mkdirs()
    }

    companion object {
        fun fromFilesDir(filesDir: File): DiagnosticsFiles = DiagnosticsFiles(File(filesDir, "diagnostics"))
    }
}

object DiagnosticsMaintenance {
    fun clearDiagnostics(files: DiagnosticsFiles) {
        DiagnosticLog.clear()
        files.clearCrashAndExportFiles()
    }
}
