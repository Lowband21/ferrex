package com.ferrex.android.ui.diagnostics

import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
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
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import com.ferrex.android.ui.qa.FerrexQaTags
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

internal object PhoneDiagnosticsPresentation {
    val RootTag: String = FerrexQaTags.namespaced("phone", "diagnostics")
    val HeaderTag: String = FerrexQaTags.namespaced("phone", "diagnostics", "header")
    val ActionsTag: String = FerrexQaTags.namespaced("phone", "diagnostics", "actions")
    val VisibleBackActionLabel: String? = null

    fun statusTag(key: String): String = FerrexQaTags.namespaced("phone", "diagnostics", "status", key)

    fun statusDescription(title: String, body: String): String = "$title. $body"

    fun actionLabels(exportRunning: Boolean, clearRunning: Boolean): List<String> = listOf(
        if (exportRunning) "Preparing export…" else "Export / Share diagnostics",
        if (clearRunning) "Clearing diagnostics…" else "Clear diagnostics/logs",
    )
}

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
            dispatch(DiagnosticsPanelEvent.ExportFailed("Diagnostics are unavailable in this build. Android back and recovery exits remain available."))
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
                dispatch(DiagnosticsPanelEvent.ExportFailed(throwable.safeMessage("Diagnostics export failed. Retry or use Android back.")))
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
                    is ActivityNotFoundException -> "No app can share the diagnostics export. Retry after installing a file/share target, or use Android back."
                    else -> throwable.safeMessage("Diagnostics share failed. Retry or use Android back.")
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
                dispatch(DiagnosticsPanelEvent.ClearFailed(throwable.safeMessage("Diagnostics clear failed. Retry or use Android back.")))
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
                .testTag(PhoneDiagnosticsPresentation.RootTag)
                .padding(horizontal = 24.dp, vertical = 32.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            item {
                Column(
                    modifier = Modifier.testTag(PhoneDiagnosticsPresentation.HeaderTag),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
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
                DiagnosticsStatusBand(status = panelState.exportStatus)
            }
            item {
                DiagnosticsStatusBand(status = panelState.clearStatus)
            }
            if (diagnostics == null) {
                item {
                    StateBand(
                        title = "Diagnostics unavailable",
                        body = "This build did not provide the diagnostics core. Android back and recovery exits remain available.",
                    )
                }
            }
            val currentSnapshot = snapshot
            if (currentSnapshot == null) {
                item {
                    StateBand(
                        title = "Loading diagnostics summary",
                        body = "Preparing safe local summaries without rendering raw credentials or device identifiers.",
                    )
                }
            } else {
                items(DiagnosticsSummaryPresenter.rows(currentSnapshot, retainedCrashCount), key = { it.label }) { row ->
                    DiagnosticsSummaryBand(row)
                }
            }
            item {
                DiagnosticsActions(
                    exportRunning = panelState.exportStatus is DiagnosticsActionStatus.Running,
                    clearRunning = panelState.clearStatus is DiagnosticsActionStatus.Running,
                    onExport = { exportDiagnostics() },
                    onClear = { dispatch(DiagnosticsPanelEvent.ClearRequested) },
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
) {
    val labels = PhoneDiagnosticsPresentation.actionLabels(exportRunning, clearRunning)
    Column(
        modifier = Modifier.testTag(PhoneDiagnosticsPresentation.ActionsTag),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Button(
            modifier = Modifier.fillMaxWidth(),
            enabled = !exportRunning && !clearRunning,
            onClick = onExport,
        ) {
            Text(labels[0])
        }
        OutlinedButton(
            modifier = Modifier.fillMaxWidth(),
            enabled = !exportRunning && !clearRunning,
            onClick = onClear,
        ) {
            Text(labels[1])
        }
    }
}

@Composable
private fun DiagnosticsStatusBand(status: DiagnosticsActionStatus) {
    when (status) {
        DiagnosticsActionStatus.Idle -> Unit
        DiagnosticsActionStatus.Running -> StateBand(
            title = "Diagnostics action in progress",
            body = "Please wait. Android back remains available if you need to leave this screen.",
        )
        is DiagnosticsActionStatus.Success -> StateBand(
            title = "Diagnostics updated",
            body = status.message,
        )
        is DiagnosticsActionStatus.Failure -> StateBand(
            title = "Diagnostics action failed",
            body = status.message,
            error = true,
        )
    }
}

@Composable
private fun DiagnosticsSummaryBand(row: DiagnosticsSummaryRow) {
    StateBand(title = row.label, body = row.value)
}

@Composable
private fun StateBand(
    title: String,
    body: String,
    error: Boolean = false,
) {
    val scheme = MaterialTheme.colorScheme
    val background = if (error) {
        scheme.errorContainer.copy(alpha = 0.28f)
    } else {
        scheme.surfaceVariant.copy(alpha = 0.18f)
    }
    val titleColor = if (error) scheme.error else scheme.primary
    val textColor = if (error) scheme.onErrorContainer else scheme.onSurface
    val dividerColor = if (error) scheme.error.copy(alpha = 0.65f) else scheme.outline.copy(alpha = 0.42f)
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .testTag(PhoneDiagnosticsPresentation.statusTag(title))
            .semantics(mergeDescendants = true) {
                contentDescription = PhoneDiagnosticsPresentation.statusDescription(title, body)
            }
            .background(background)
            .drawBehind { drawBottomDivider(dividerColor) }
            .padding(horizontal = 0.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
            color = titleColor,
        )
        Text(
            text = body,
            style = MaterialTheme.typography.bodyMedium,
            color = textColor,
        )
    }
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawBottomDivider(color: Color) {
    val stroke = 1.dp.toPx()
    drawLine(
        color = color,
        start = androidx.compose.ui.geometry.Offset(0f, size.height - stroke / 2f),
        end = androidx.compose.ui.geometry.Offset(size.width, size.height - stroke / 2f),
        strokeWidth = stroke,
    )
}

private fun Throwable.safeMessage(fallback: String): String = message?.takeIf { it.isNotBlank() } ?: fallback
