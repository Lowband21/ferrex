package com.ferrex.android.tv.ui

import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
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
import androidx.compose.ui.text.style.TextAlign
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
import com.ferrex.android.tv.ui.foundation.TvActionPanel
import com.ferrex.android.tv.ui.foundation.TvActionPanelAction
import com.ferrex.android.tv.ui.foundation.TvActionRole
import com.ferrex.android.tv.ui.foundation.TvScaffold
import com.ferrex.android.tv.ui.foundation.TvTitle
import com.ferrex.android.tv.ui.foundation.rememberTvFocusRestorer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun TvDiagnosticsScreen(
    diagnostics: AndroidDiagnosticsCore?,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val focusRestorer = rememberTvFocusRestorer("diagnostics")
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
                    is ActivityNotFoundException -> "No app can share the diagnostics export. Retry after installing a file/share target, or press Back."
                    else -> throwable.safeMessage("Diagnostics share failed. Retry or press Back.")
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
                dispatch(DiagnosticsPanelEvent.ClearFailed(throwable.safeMessage("Diagnostics clear failed. Retry or press Back.")))
            }
        }
    }

    LaunchedEffect(diagnostics) { refreshSummary() }
    BackHandler(onBack = onBack)

    TvScaffold(
        contentMaxWidth = 1180.dp,
        horizontalPadding = 72.dp,
        verticalPadding = 42.dp,
        verticalArrangement = Arrangement.Top,
        scrollable = true,
    ) {
        TvTitle("Settings & Diagnostics", "Export a redacted bundle or clear diagnostics/logs without touching auth, server, cache, or playback data.")
        Text(
            text = "Safe summaries only: raw tokens, passwords, PIN proofs, session IDs, local device IDs, private keys, and playback tickets are never rendered here.",
            style = MaterialTheme.typography.titleMedium,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(22.dp))
        DiagnosticsStatusCard(panelState.exportStatus)
        DiagnosticsStatusCard(panelState.clearStatus)
        if (diagnostics == null) {
            DiagnosticsRowCard(
                DiagnosticsSummaryRow(
                    label = "Diagnostics unavailable",
                    value = "This build did not provide the diagnostics core. Back and recovery exits remain available.",
                ),
            )
        }
        val currentSnapshot = snapshot
        if (currentSnapshot == null) {
            DiagnosticsRowCard(
                DiagnosticsSummaryRow(
                    label = "Loading diagnostics summary",
                    value = "Preparing safe local summaries without rendering raw credentials or device identifiers.",
                ),
            )
        } else {
            DiagnosticsSummaryPresenter.rows(currentSnapshot, retainedCrashCount).forEach { row ->
                DiagnosticsRowCard(row)
            }
        }
        Spacer(Modifier.height(18.dp))
        if (panelState.clearConfirmationVisible) {
            TvActionPanel(
                title = "Clear diagnostics/logs?",
                supportingText = "This clears only local diagnostic logs, retained crash files, and previous diagnostic exports. Sign-in, server, library cache, image cache, and playback state are preserved.",
                actions = listOf(
                    TvActionPanelAction("confirm-clear", "Clear diagnostics", TvActionRole.Destructive, onSelect = { clearDiagnostics() }),
                    TvActionPanelAction("cancel-clear", "Cancel", TvActionRole.Back, onSelect = { dispatch(DiagnosticsPanelEvent.ClearCancelled) }),
                    TvActionPanelAction("back", "Back", TvActionRole.Back, onSelect = onBack),
                ),
                focusRestorer = focusRestorer,
                surfaceKey = "clear-confirmation",
                autoFocus = true,
                buttonMaxWidth = 620.dp,
            )
        } else {
            val busy = panelState.exportStatus is DiagnosticsActionStatus.Running || panelState.clearStatus is DiagnosticsActionStatus.Running
            TvActionPanel(
                title = "Diagnostics actions",
                supportingText = "After export or clear, focus returns to the last diagnostics action so the D-pad never strands you.",
                actions = listOf(
                    TvActionPanelAction(
                        key = "export",
                        label = if (panelState.exportStatus is DiagnosticsActionStatus.Running) "Preparing export…" else "Export / Share diagnostics",
                        role = TvActionRole.Primary,
                        enabled = !busy,
                        busy = panelState.exportStatus is DiagnosticsActionStatus.Running,
                        onSelect = { exportDiagnostics() },
                    ),
                    TvActionPanelAction(
                        key = "clear",
                        label = if (panelState.clearStatus is DiagnosticsActionStatus.Running) "Clearing diagnostics…" else "Clear diagnostics/logs",
                        role = TvActionRole.Destructive,
                        enabled = !busy,
                        busy = panelState.clearStatus is DiagnosticsActionStatus.Running,
                        onSelect = { dispatch(DiagnosticsPanelEvent.ClearRequested) },
                    ),
                    TvActionPanelAction("back", "Back", TvActionRole.Back, onSelect = onBack),
                ),
                focusRestorer = focusRestorer,
                surfaceKey = "diagnostics-actions",
                autoFocus = true,
                buttonMaxWidth = 620.dp,
            )
        }
    }
}

@Composable
private fun DiagnosticsStatusCard(status: DiagnosticsActionStatus) {
    when (status) {
        DiagnosticsActionStatus.Idle -> Unit
        DiagnosticsActionStatus.Running -> DiagnosticsRowCard(
            DiagnosticsSummaryRow(
                label = "Diagnostics action in progress",
                value = "Please wait. Back remains available if you need to leave this screen.",
            ),
        )
        is DiagnosticsActionStatus.Success -> DiagnosticsRowCard(
            DiagnosticsSummaryRow(label = "Diagnostics updated", value = status.message),
        )
        is DiagnosticsActionStatus.Failure -> DiagnosticsRowCard(
            DiagnosticsSummaryRow(label = "Diagnostics action failed", value = status.message),
            error = true,
        )
    }
}

@Composable
private fun DiagnosticsRowCard(
    row: DiagnosticsSummaryRow,
    error: Boolean = false,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 10.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (error) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.76f),
            contentColor = if (error) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Column(modifier = Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                text = row.label,
                style = MaterialTheme.typography.titleLarge,
                color = if (error) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.primary,
                fontWeight = FontWeight.SemiBold,
            )
            Text(row.value, style = MaterialTheme.typography.titleMedium)
        }
    }
}

private fun Throwable.safeMessage(fallback: String): String = message?.takeIf { it.isNotBlank() } ?: fallback
