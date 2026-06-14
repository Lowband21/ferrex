package com.ferrex.android.ui.diagnostics

import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.diagnostics.AndroidDiagnosticsCore
import com.ferrex.android.core.diagnostics.AndroidDisplayDiagnostics
import com.ferrex.android.core.diagnostics.DiagnosticsActionStatus
import com.ferrex.android.core.diagnostics.DiagnosticsExportShare
import com.ferrex.android.core.diagnostics.DiagnosticsPanelEvent
import com.ferrex.android.core.diagnostics.DiagnosticsPanelReducer
import com.ferrex.android.core.diagnostics.DiagnosticsPanelState
import com.ferrex.android.core.diagnostics.DiagnosticsSnapshot
import com.ferrex.android.core.diagnostics.DiagnosticsSummaryPresenter
import com.ferrex.android.core.diagnostics.DiagnosticsSummaryRow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun PhoneDiagnosticsScreen(
    diagnostics: AndroidDiagnosticsCore?,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var panelState by remember { mutableStateOf(DiagnosticsPanelState()) }
    var snapshot by remember { mutableStateOf<DiagnosticsSnapshot?>(null) }
    var retainedCrashCount by remember { mutableStateOf(0) }

    fun dispatch(event: DiagnosticsPanelEvent) {
        panelState = DiagnosticsPanelReducer.reduce(panelState, event)
    }

    fun refreshSummary() {
        scope.launch {
            val refreshed = withContext(Dispatchers.IO) {
                diagnostics?.let {
                    val display = AndroidDisplayDiagnostics.snapshot(context)
                    it.snapshot(display) to it.retainedCrashFiles().size
                }
            }
            snapshot = refreshed?.first
            retainedCrashCount = refreshed?.second ?: 0
        }
    }

    fun exportDiagnostics() {
        if (diagnostics == null) {
            dispatch(DiagnosticsPanelEvent.ExportFailed("Diagnostics are unavailable in this build. Back and recovery exits remain available."))
            return
        }
        scope.launch {
            dispatch(DiagnosticsPanelEvent.ExportStarted)
            val result = runCatching {
                withContext(Dispatchers.IO) {
                    diagnostics.exportBundle(AndroidDisplayDiagnostics.snapshot(context))
                }
            }
            val file = result.getOrElse { throwable ->
                dispatch(DiagnosticsPanelEvent.ExportFailed(throwable.safeMessage("Diagnostics export failed. Retry or go back.")))
                return@launch
            }
            val shareResult = runCatching {
                val sendIntent = DiagnosticsExportShare.shareIntent(context, file)
                context.startActivity(Intent.createChooser(sendIntent, "Share Ferrex diagnostics"))
            }
            shareResult.onSuccess {
                dispatch(DiagnosticsPanelEvent.ExportSucceeded(file.name))
                refreshSummary()
            }.onFailure { throwable ->
                val message = when (throwable) {
                    is ActivityNotFoundException -> "No app can share the diagnostics export. Retry after installing a file/share target, or go back."
                    else -> throwable.safeMessage("Diagnostics share failed. Retry or go back.")
                }
                dispatch(DiagnosticsPanelEvent.ExportFailed(message))
            }
        }
    }

    fun clearDiagnostics() {
        if (diagnostics == null) {
            dispatch(DiagnosticsPanelEvent.ClearFailed("Diagnostics are unavailable in this build. Auth, server, cache, and playback data were not changed."))
            return
        }
        scope.launch {
            dispatch(DiagnosticsPanelEvent.ClearStarted)
            val result = runCatching {
                withContext(Dispatchers.IO) { diagnostics.clearDiagnostics() }
            }
            result.onSuccess {
                dispatch(DiagnosticsPanelEvent.ClearSucceeded)
                refreshSummary()
            }.onFailure { throwable ->
                dispatch(DiagnosticsPanelEvent.ClearFailed(throwable.safeMessage("Diagnostics clear failed. Retry or go back.")))
            }
        }
    }

    LaunchedEffect(diagnostics) { refreshSummary() }
    BackHandler(onBack = onBack)

    if (panelState.clearConfirmationVisible) {
        AlertDialog(
            onDismissRequest = { dispatch(DiagnosticsPanelEvent.ClearCancelled) },
            title = { Text("Clear diagnostics/logs?") },
            text = {
                Text("This clears only local diagnostic logs, retained crash files, and previous diagnostic exports. Sign-in, server, library cache, image cache, and playback state are preserved.")
            },
            confirmButton = {
                Button(onClick = { clearDiagnostics() }) { Text("Clear diagnostics") }
            },
            dismissButton = {
                TextButton(onClick = { dispatch(DiagnosticsPanelEvent.ClearCancelled) }) { Text("Cancel") }
            },
        )
    }

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp, vertical = 32.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            item {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(onClick = onBack) { Text("Back") }
                    Text(
                        text = "Settings & Diagnostics",
                        style = MaterialTheme.typography.headlineMedium,
                        color = MaterialTheme.colorScheme.primary,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        text = "Review safe app, device, server, session, cache, playback, and crash summaries; export a redacted bundle; or clear only diagnostics/logs.",
                        style = MaterialTheme.typography.bodyLarge,
                    )
                }
            }
            item {
                DiagnosticsStatusCard(status = panelState.exportStatus)
            }
            item {
                DiagnosticsStatusCard(status = panelState.clearStatus)
            }
            if (diagnostics == null) {
                item {
                    StateCard(
                        title = "Diagnostics unavailable",
                        body = "This build did not provide the diagnostics core. Back and recovery exits remain available.",
                    )
                }
            }
            val currentSnapshot = snapshot
            if (currentSnapshot == null) {
                item {
                    StateCard(
                        title = "Loading diagnostics summary",
                        body = "Preparing safe local summaries without rendering raw credentials or device identifiers.",
                    )
                }
            } else {
                items(DiagnosticsSummaryPresenter.rows(currentSnapshot, retainedCrashCount), key = { it.label }) { row ->
                    DiagnosticsRowCard(row)
                }
            }
            item {
                DiagnosticsActions(
                    exportRunning = panelState.exportStatus is DiagnosticsActionStatus.Running,
                    clearRunning = panelState.clearStatus is DiagnosticsActionStatus.Running,
                    onExport = { exportDiagnostics() },
                    onClear = { dispatch(DiagnosticsPanelEvent.ClearRequested) },
                    onBack = onBack,
                )
            }
        }
    }
}

@Composable
private fun DiagnosticsActions(
    exportRunning: Boolean,
    clearRunning: Boolean,
    onExport: () -> Unit,
    onClear: () -> Unit,
    onBack: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Button(
            modifier = Modifier.fillMaxWidth(),
            enabled = !exportRunning && !clearRunning,
            onClick = onExport,
        ) {
            Text(if (exportRunning) "Preparing export…" else "Export / Share diagnostics")
        }
        OutlinedButton(
            modifier = Modifier.fillMaxWidth(),
            enabled = !exportRunning && !clearRunning,
            onClick = onClear,
        ) {
            Text(if (clearRunning) "Clearing diagnostics…" else "Clear diagnostics/logs")
        }
        TextButton(modifier = Modifier.fillMaxWidth(), onClick = onBack) { Text("Back") }
    }
}

@Composable
private fun DiagnosticsStatusCard(status: DiagnosticsActionStatus) {
    when (status) {
        DiagnosticsActionStatus.Idle -> Unit
        DiagnosticsActionStatus.Running -> StateCard(
            title = "Diagnostics action in progress",
            body = "Please wait. Back remains available if you need to leave this screen.",
        )
        is DiagnosticsActionStatus.Success -> StateCard(
            title = "Diagnostics updated",
            body = status.message,
        )
        is DiagnosticsActionStatus.Failure -> StateCard(
            title = "Diagnostics action failed",
            body = status.message,
            error = true,
        )
    }
}

@Composable
private fun DiagnosticsRowCard(row: DiagnosticsSummaryRow) {
    StateCard(title = row.label, body = row.value)
}

@Composable
private fun StateCard(
    title: String,
    body: String,
    error: Boolean = false,
) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = if (error) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.surfaceVariant,
            contentColor = if (error) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(modifier = Modifier.fillMaxWidth()) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleSmall,
                    color = if (error) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.primary,
                )
            }
            Text(text = body, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

private fun Throwable.safeMessage(fallback: String): String = message?.takeIf { it.isNotBlank() } ?: fallback
