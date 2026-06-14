package com.ferrex.android.core.diagnostics

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.core.content.FileProvider
import java.io.File

object DiagnosticsExportFilePolicy {
    const val EXPORTS_RELATIVE_PATH = "diagnostics/exports"

    fun exportsDir(filesDir: File): File = File(filesDir, EXPORTS_RELATIVE_PATH)

    fun isAllowedExportFile(file: File, filesDir: File): Boolean {
        val exportRoot = runCatching { exportsDir(filesDir).canonicalFile }.getOrNull() ?: return false
        val candidate = runCatching { file.canonicalFile }.getOrNull() ?: return false
        return candidate.isFile &&
            candidate.extension.equals("zip", ignoreCase = true) &&
            candidate.toPath().startsWith(exportRoot.toPath())
    }
}

object DiagnosticsExportShare {
    const val MIME_TYPE = "application/zip"

    fun providerAuthority(context: Context): String = "${context.packageName}.diagnostics.fileprovider"

    fun contentUriForExport(context: Context, file: File): Uri {
        require(DiagnosticsExportFilePolicy.isAllowedExportFile(file, context.filesDir)) {
            "Diagnostics export must be a zip file inside the diagnostics exports directory"
        }
        return FileProvider.getUriForFile(context, providerAuthority(context), file)
    }

    fun shareIntent(context: Context, file: File): Intent {
        val uri = contentUriForExport(context, file)
        return Intent(Intent.ACTION_SEND).apply {
            type = MIME_TYPE
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            clipData = ClipData.newUri(context.contentResolver, file.name, uri)
        }
    }
}
