package com.ferrex.android.core.diagnostics

import java.util.Locale

sealed interface DiagnosticsActionStatus {
    data object Idle : DiagnosticsActionStatus
    data object Running : DiagnosticsActionStatus
    data class Success(val message: String) : DiagnosticsActionStatus
    data class Failure(val message: String) : DiagnosticsActionStatus
}

data class DiagnosticsPanelState(
    val exportStatus: DiagnosticsActionStatus = DiagnosticsActionStatus.Idle,
    val clearStatus: DiagnosticsActionStatus = DiagnosticsActionStatus.Idle,
    val clearConfirmationVisible: Boolean = false,
)

sealed interface DiagnosticsPanelEvent {
    data object ExportStarted : DiagnosticsPanelEvent
    data class ExportSucceeded(val fileName: String) : DiagnosticsPanelEvent
    data class ExportFailed(val message: String) : DiagnosticsPanelEvent
    data object ClearRequested : DiagnosticsPanelEvent
    data object ClearCancelled : DiagnosticsPanelEvent
    data object ClearStarted : DiagnosticsPanelEvent
    data object ClearSucceeded : DiagnosticsPanelEvent
    data class ClearFailed(val message: String) : DiagnosticsPanelEvent
    data object DismissMessages : DiagnosticsPanelEvent
}

object DiagnosticsPanelReducer {
    fun reduce(
        state: DiagnosticsPanelState,
        event: DiagnosticsPanelEvent,
    ): DiagnosticsPanelState = when (event) {
        DiagnosticsPanelEvent.ExportStarted -> state.copy(
            exportStatus = DiagnosticsActionStatus.Running,
        )
        is DiagnosticsPanelEvent.ExportSucceeded -> state.copy(
            exportStatus = DiagnosticsActionStatus.Success("Export ready: ${event.fileName}. Choose an app from the Android share sheet."),
        )
        is DiagnosticsPanelEvent.ExportFailed -> state.copy(
            exportStatus = DiagnosticsActionStatus.Failure(event.message.ifBlank { "Diagnostics export failed. Retry or go back." }),
        )
        DiagnosticsPanelEvent.ClearRequested -> state.copy(clearConfirmationVisible = true)
        DiagnosticsPanelEvent.ClearCancelled -> state.copy(clearConfirmationVisible = false)
        DiagnosticsPanelEvent.ClearStarted -> state.copy(
            clearConfirmationVisible = false,
            clearStatus = DiagnosticsActionStatus.Running,
        )
        DiagnosticsPanelEvent.ClearSucceeded -> state.copy(
            clearStatus = DiagnosticsActionStatus.Success("Diagnostic logs, retained crashes, and previous exports were cleared. Server, sign-in, cache, and playback data were preserved."),
        )
        is DiagnosticsPanelEvent.ClearFailed -> state.copy(
            clearConfirmationVisible = false,
            clearStatus = DiagnosticsActionStatus.Failure(event.message.ifBlank { "Diagnostics clear failed. Retry or go back." }),
        )
        DiagnosticsPanelEvent.DismissMessages -> state.copy(
            exportStatus = DiagnosticsActionStatus.Idle,
            clearStatus = DiagnosticsActionStatus.Idle,
        )
    }
}

data class DiagnosticsSummaryRow(
    val label: String,
    val value: String,
)

object DiagnosticsSummaryPresenter {
    fun rows(
        snapshot: DiagnosticsSnapshot,
        retainedCrashCount: Int,
    ): List<DiagnosticsSummaryRow> = buildList {
        add(
            DiagnosticsSummaryRow(
                label = "App/build",
                value = listOfNotNull(
                    snapshot.app.applicationId,
                    snapshot.app.versionName.takeIf { it.isNotBlank() }?.let { "v$it" },
                    "code ${snapshot.app.versionCode}",
                    snapshot.app.flavor?.let { "flavor $it" },
                    snapshot.app.buildType?.let { "build $it" },
                ).joinToString(" • "),
            ),
        )
        snapshot.device?.let { device ->
            add(
                DiagnosticsSummaryRow(
                    label = "Device",
                    value = listOf(
                        device.manufacturer,
                        device.brand,
                        device.model,
                        "Android ${device.release} (SDK ${device.sdkInt})",
                    ).filter { it.isNotBlank() }.joinToString(" • "),
                ),
            )
        }
        snapshot.display?.let { display ->
            add(
                DiagnosticsSummaryRow(
                    label = "Display",
                    value = if (display.defaultDisplayPresent) {
                        listOfNotNull(
                            display.displayName,
                            display.resolution,
                            display.refreshRateHz?.let { String.format(Locale.US, "%.1f Hz", it) },
                            display.hdrTypes.takeIf { it.isNotEmpty() }?.joinToString(prefix = "HDR "),
                            display.windowColorMode?.let { "color $it" },
                        ).joinToString(" • ")
                    } else {
                        "No default display reported"
                    },
                ),
            )
        }
        add(
            DiagnosticsSummaryRow(
                label = "Server",
                value = if (snapshot.server.configured) {
                    listOfNotNull(
                        snapshot.server.canonicalOrigin ?: "Configured server",
                        snapshot.server.canonicalUrlHash?.let { "url hash $it" },
                    ).joinToString(" • ")
                } else {
                    "No server configured"
                },
            ),
        )
        add(
            DiagnosticsSummaryRow(
                label = "Session",
                value = listOfNotNull(
                    "access token saved: ${snapshot.auth.accessTokenPresent.yesNo()}",
                    "refresh token saved: ${snapshot.auth.refreshTokenPresent.yesNo()}",
                    "session marker: ${snapshot.auth.sessionPresent.yesNo()}",
                    "device session marker: ${snapshot.auth.deviceSessionPresent.yesNo()}",
                    snapshot.auth.userIdHash?.let { "user hash $it" },
                    "PIN setup required: ${snapshot.auth.requiresPinSetup.yesNo()}",
                ).joinToString(" • "),
            ),
        )
        add(
            DiagnosticsSummaryRow(
                label = "Playback",
                value = "${snapshot.playback.retainedEntryCount} retained event(s) • ${snapshot.playback.warningCount} warning(s) • ${snapshot.playback.errorCount} error(s)",
            ),
        )
        snapshot.cache?.library?.let { library ->
            add(
                DiagnosticsSummaryRow(
                    label = "Library cache",
                    value = "scope ${library.scopeDirectoryName} • ${library.approximateBytes.formatBytes()} • ${library.cachedMovieBatchFiles} movie batch file(s) • ${library.cachedSeriesBundleFiles} series bundle file(s) • ${library.quarantineFileCount} quarantine file(s)",
                ),
            )
        }
        snapshot.cache?.image?.let { image ->
            add(
                DiagnosticsSummaryRow(
                    label = "Image cache",
                    value = "scope ${image.scopeDirectoryName} • ${image.approximateBytes.formatBytes()} • ${image.manifestEntryFiles} manifest file(s) • ${image.coilBlobBytes.formatBytes()} Coil blob(s) • ${image.quarantineFileCount} quarantine file(s)",
                ),
            )
        }
        add(
            DiagnosticsSummaryRow(
                label = "Retained crashes",
                value = "$retainedCrashCount crash file(s) retained for export",
            ),
        )
        add(
            DiagnosticsSummaryRow(
                label = "Privacy",
                value = "Diagnostics show safe summaries only. Raw tokens, passwords, PIN proofs, session IDs, local device IDs, private keys, and playback tickets are redacted and are not rendered here.",
            ),
        )
    }

    private fun Boolean.yesNo(): String = if (this) "yes" else "no"

    private fun Long.formatBytes(): String = when {
        this >= 1024L * 1024L -> String.format(Locale.US, "%.1f MiB", this / (1024.0 * 1024.0))
        this >= 1024L -> String.format(Locale.US, "%.1f KiB", this / 1024.0)
        else -> "$this B"
    }
}
