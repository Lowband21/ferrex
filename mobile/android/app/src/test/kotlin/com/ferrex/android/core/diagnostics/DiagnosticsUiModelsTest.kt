package com.ferrex.android.core.diagnostics

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class DiagnosticsUiModelsTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun exportFilePolicyAllowsOnlyZipFilesInsideDiagnosticsExports() {
        val filesDir = temporaryFolder.newFolder("files")
        val exportsDir = DiagnosticsExportFilePolicy.exportsDir(filesDir).apply { mkdirs() }
        val crashDir = filesDir.resolve("diagnostics/crashes").apply { mkdirs() }
        val allowed = exportsDir.resolve("ferrex-diagnostics.zip").apply { writeText("zip") }
        val wrongExtension = exportsDir.resolve("ferrex-diagnostics.txt").apply { writeText("text") }
        val outside = crashDir.resolve("crash.zip").apply { writeText("zip") }
        val traversal = exportsDir.resolve("../crashes/traversal.zip").apply { writeText("zip") }

        assertTrue(DiagnosticsExportFilePolicy.isAllowedExportFile(allowed, filesDir))
        assertFalse(DiagnosticsExportFilePolicy.isAllowedExportFile(wrongExtension, filesDir))
        assertFalse(DiagnosticsExportFilePolicy.isAllowedExportFile(outside, filesDir))
        assertFalse(DiagnosticsExportFilePolicy.isAllowedExportFile(traversal, filesDir))
    }

    @Test
    fun reducerKeepsExportFailuresRetryableAndSeparatesClearConfirmation() {
        val exporting = DiagnosticsPanelReducer.reduce(DiagnosticsPanelState(), DiagnosticsPanelEvent.ExportStarted)
        assertEquals(DiagnosticsActionStatus.Running, exporting.exportStatus)

        val failed = DiagnosticsPanelReducer.reduce(exporting, DiagnosticsPanelEvent.ExportFailed("share failed"))
        assertEquals(DiagnosticsActionStatus.Failure("share failed"), failed.exportStatus)
        assertFalse(failed.clearConfirmationVisible)

        val confirming = DiagnosticsPanelReducer.reduce(failed, DiagnosticsPanelEvent.ClearRequested)
        assertTrue(confirming.clearConfirmationVisible)
        assertEquals(DiagnosticsActionStatus.Failure("share failed"), confirming.exportStatus)

        val clearing = DiagnosticsPanelReducer.reduce(confirming, DiagnosticsPanelEvent.ClearStarted)
        assertFalse(clearing.clearConfirmationVisible)
        assertEquals(DiagnosticsActionStatus.Running, clearing.clearStatus)

        val cleared = DiagnosticsPanelReducer.reduce(clearing, DiagnosticsPanelEvent.ClearSucceeded)
        assertTrue((cleared.clearStatus as DiagnosticsActionStatus.Success).message.contains("preserved"))
    }

    @Test
    fun summaryPresenterRendersOnlySafeDiagnosticsFields() {
        val snapshot = DiagnosticsSnapshot(
            generatedAtEpochMs = 1L,
            app = AppDiagnosticsSummary(
                applicationId = "com.ferrex.android.test",
                versionName = "0.1.0",
                versionCode = 1L,
                buildType = "debug",
                flavor = "mobile",
            ),
            device = DeviceDiagnosticsSummary(
                manufacturer = "Acme",
                brand = "AcmeBrand",
                model = "Living Room TV",
                device = "secret-device-codename",
                product = "secret-product",
                sdkInt = 35,
                release = "15",
            ),
            server = ServerDiagnosticsSummary(
                configured = true,
                canonicalOrigin = "https://ferrex.local",
                canonicalUrlHash = "server-url-hash",
            ),
            auth = AuthDiagnosticsSummary(
                accessTokenPresent = true,
                refreshTokenPresent = true,
                sessionPresent = true,
                deviceSessionPresent = true,
                userIdHash = "user-hash",
                requiresPinSetup = false,
            ),
            playback = PlaybackDiagnosticsSummary(
                retainedEntryCount = 4,
                warningCount = 1,
                errorCount = 2,
            ),
            cache = CacheDiagnosticsSummary(
                library = LibraryCacheDiagnosticsSummary(
                    scopeDirectoryName = "scope-hash",
                    relativeScopePath = "library/scope-hash",
                    approximateBytes = 2048L,
                    libraryListPresent = true,
                    libraryDirectoryCount = 1,
                    cachedMovieBatchFiles = 2,
                    cachedSeriesBundleFiles = 3,
                    quarantineFileCount = 0,
                    staleOfflineMarkerPresent = false,
                ),
                image = ImageCacheDiagnosticsSummary(
                    scopeDirectoryName = "scope-hash",
                    relativeImagesPath = "images/scope-hash",
                    approximateBytes = 4096L,
                    manifestEntryFiles = 8,
                    coilBlobBytes = 1024L,
                    quarantineFileCount = 1,
                    staleOfflineMarkerPresent = false,
                ),
            ),
        )

        val rendered = DiagnosticsSummaryPresenter.rows(snapshot, retainedCrashCount = 2).joinToString("\n") { "${it.label}: ${it.value}" }

        assertTrue(rendered.contains("App/build"))
        assertTrue(rendered.contains("https://ferrex.local"))
        assertTrue(rendered.contains("user hash user-hash"))
        assertTrue(rendered.contains("2 crash file"))
        listOf(
            "access-secret",
            "refresh-secret",
            "session-secret",
            "device-session-secret",
            "local-device-secret",
            "password-secret",
            "pin-proof-secret",
            "playback-ticket",
        ).forEach { forbidden ->
            assertFalse("unsafe field rendered: $forbidden", rendered.contains(forbidden))
        }
    }
}
