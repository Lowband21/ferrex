package com.ferrex.android.core.diagnostics

import com.ferrex.android.core.api.AuthTokens
import com.ferrex.android.core.auth.AuthStorage
import com.ferrex.android.core.image.ImageDiskCache
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageManifestRecord
import com.ferrex.android.core.image.ManifestImageStatus
import com.ferrex.android.core.library.LibraryDiskCache
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.LibraryKind
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.RetryClassification
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.playback.PlaybackDiagnosticLog
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.util.UUID
import java.util.zip.ZipFile

class DiagnosticsCoreTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun redactorCoversHeadersQueriesJsonAndBodySecrets() {
        val raw = """
            Authorization: Bearer session-secret
            Proxy Authorization: Basic dXNlcjpwYXNz
            Cookie: sid=cookie-secret; refresh_token=refresh-cookie
            GET https://ferrex.local/stream?access_token=access-secret&ticket=playback-ticket&device_session_id=device-session
            {"refresh_token":"refresh-secret","password":"pw","pin":1234,"client_proof":"pin-proof","device_signature":"signature-secret","private_key":"-----BEGIN PRIVATE KEY-----abc-----END PRIVATE KEY-----"}
            local_device_id=local-device-secret session_id=session-secret-body api_key=api-secret
        """.trimIndent()

        val redacted = DiagnosticsRedactor.redactText(raw)

        listOf(
            "session-secret",
            "dXNlcjpwYXNz",
            "cookie-secret",
            "refresh-cookie",
            "access-secret",
            "playback-ticket",
            "device-session",
            "refresh-secret",
            "pw",
            "1234",
            "pin-proof",
            "signature-secret",
            "local-device-secret",
            "session-secret-body",
            "api-secret",
        ).forEach { secret -> assertFalse("secret leaked: $secret", redacted.contains(secret)) }
        assertTrue(redacted.contains("Authorization: Bearer <redacted>"))
        assertTrue(redacted.contains("Basic <redacted>"))
        assertTrue(redacted.contains("access_token=<redacted>"))
        assertTrue(redacted.contains("\"password\":\"<redacted>\""))
        assertTrue(redacted.contains("\"pin\":<redacted>"))
    }

    @Test
    fun throwableRedactionRemovesSecretsFromMessagesAndStackTraceText() {
        val throwable = IllegalStateException("failed with access_token=access-secret password=secret-password")

        val redacted = DiagnosticsRedactor.redactThrowable(throwable)

        assertFalse(redacted.contains("access-secret"))
        assertFalse(redacted.contains("secret-password"))
        assertTrue(redacted.contains("access_token=<redacted>"))
        assertTrue(redacted.contains("password=<redacted>"))
    }

    @Test
    fun crashRetentionRedactsBoundsAndPrunesOldFiles() {
        val files = DiagnosticsFiles(temporaryFolder.newFolder("diagnostics"))
        var now = 1_000L
        val store = CrashRetentionStore(
            diagnosticsRoot = files.rootDir,
            maxCrashFiles = 2,
            maxCrashFileBytes = 1_600,
            clockMillis = { now++ },
        )
        DiagnosticLog.clear()
        DiagnosticLog.info("Auth", "Authorization: Bearer session-secret", source = DiagnosticLog.Source.App)
        val snapshot = testSnapshot()

        repeat(3) { index ->
            store.writeCrash(
                threadName = "main",
                threadId = 1L,
                throwable = RuntimeException("boom-$index refresh_token=refresh-secret"),
                snapshot = snapshot,
            )
        }

        val crashes = store.retainedCrashFiles()
        assertEquals(2, crashes.size)
        crashes.forEach { crash ->
            val text = crash.readText()
            assertTrue(crash.length() <= 1_600)
            assertFalse(text.contains("session-secret"))
            assertFalse(text.contains("refresh-secret"))
            assertTrue(text.contains("<redacted>"))
        }
    }

    @Test
    fun exportBundleIncludesManifestLogsAndCrashFilesWithRedaction() {
        val files = DiagnosticsFiles(temporaryFolder.newFolder("diagnostics"))
        val store = CrashRetentionStore(files.rootDir, clockMillis = { 10L })
        val builder = DiagnosticsExportBuilder(files, store, clockMillis = { 20L })
        DiagnosticLog.clear()
        DiagnosticLog.warn("Playback", "stream failed ticket=playback-ticket", source = DiagnosticLog.Source.Playback)
        store.writeCrash(
            threadName = "main",
            threadId = 1L,
            throwable = RuntimeException("Authorization: Bearer crash-secret"),
            snapshot = testSnapshot(),
        )

        val zip = builder.build(testSnapshot())

        ZipFile(zip).use { archive ->
            val names = archive.entries().asSequence().map { it.name }.toList()
            assertEquals(
                listOf("manifest.json", "diagnostics.txt", "logs/diagnostic-log.txt", "crashes/crash-0000000000010.txt"),
                names,
            )
            val manifest = archive.readText("manifest.json")
            val logs = archive.readText("logs/diagnostic-log.txt")
            val crash = archive.readText("crashes/crash-0000000000010.txt")
            assertTrue(manifest.contains("\"retainedLogCount\": 2"))
            assertTrue(logs.contains("ticket=<redacted>"))
            assertFalse(logs.contains("playback-ticket"))
            assertFalse(crash.contains("crash-secret"))
        }
    }

    @Test
    fun authAndCacheSummariesAvoidRawIdentityAndCountCheapCacheFacts() {
        val storage = FakeAuthStorage().apply {
            serverUrl = "https://ferrex.local/base"
            accessToken = "access-secret"
            refreshToken = "refresh-secret"
            username = "raw-username"
            userId = "user-123"
            sessionId = "session-secret"
            deviceSessionId = "device-session-secret"
            localDeviceId = "local-device-secret"
            requiresPinSetup = true
        }
        val scope = ServerCacheScope.from(storage.serverUrl!!, storage.userId)
        val libraryCache = LibraryDiskCache(temporaryFolder.newFolder("library-cache"))
        val imageCache = ImageDiskCache(temporaryFolder.newFolder("image-cache"))
        val library = LibraryInfo(UUID(0, 1).toString(), "Movies", LibraryKind.Movies)
        libraryCache.writeMovieBatch(scope, library.id, 7, 1L, byteArrayOf(1, 2, 3, 4))
        imageCache.writeManifestEntry(
            scope,
            ImageManifestRecord(
                key = ImageRequestKey(UUID(0, 2).toString(), BrowseImageCategory.Poster),
                status = ManifestImageStatus.Ready("image-token-secret"),
            ),
        )
        val coilBlob = imageCache.coilDiskCacheDir(scope).resolve("blob")
        coilBlob.writeText("blob-bytes")

        val auth = SafeAuthDiagnostics.summarize(storage)
        val server = SafeServerDiagnostics.summarize(storage.serverUrl)
        val cache = SafeCacheDiagnostics.summarize(
            library = libraryCache.diagnosticSnapshot(scope),
            image = imageCache.diagnosticSnapshot(scope),
            state = LibraryRepositoryState(
                scope = scope,
                libraries = listOf(library),
                selectedLibraryId = library.id,
                freshness = LibraryFreshness.SeriesCacheIncomplete(
                    message = "network interrupted",
                    completedBundles = 36,
                    expectedBundles = 400,
                    remainingBundleIds = (0 until 364).map { "series-$it" },
                    itemCount = 172,
                    classification = RetryClassification.Retryable,
                    failedBundleCount = 16,
                ),
            ),
        )
        val combined = "$auth $server $cache"

        assertTrue(auth.accessTokenPresent)
        assertTrue(auth.refreshTokenPresent)
        assertTrue(auth.sessionPresent)
        assertTrue(auth.deviceSessionPresent)
        assertEquals("user-123".sha256Short(), auth.userIdHash)
        assertTrue(auth.requiresPinSetup)
        assertEquals("https://ferrex.local", server.canonicalOrigin)
        assertFalse(combined.contains("access-secret"))
        assertFalse(combined.contains("refresh-secret"))
        assertFalse(combined.contains("raw-username"))
        assertFalse(combined.contains("session-secret"))
        assertFalse(combined.contains("local-device-secret"))
        assertEquals(1, cache.library?.cachedMovieBatchFiles)
        assertEquals("series-cache-incomplete", cache.library?.health?.state)
        assertEquals("${library.id}".sha256Short(), cache.library?.health?.selectedLibraryIdHash)
        assertEquals(172, cache.library?.health?.cachedItems)
        assertEquals(36, cache.library?.health?.cachedSeriesBundles)
        assertEquals(400, cache.library?.health?.expectedSeriesBundles)
        assertEquals(364, cache.library?.health?.pendingSeriesBundles)
        assertEquals(16, cache.library?.health?.failedSeriesBundles)
        assertEquals(1, cache.image?.manifestEntryFiles)
        assertTrue((cache.image?.coilBlobBytes ?: 0L) > 0L)
    }

    @Test
    fun clearDiagnosticsLeavesAuthAndAppCachesUntouched() {
        val diagnostics = DiagnosticsFiles(temporaryFolder.newFolder("diagnostics"))
        val storage = FakeAuthStorage().apply {
            serverUrl = "https://ferrex.local"
            accessToken = "access-secret"
            refreshToken = "refresh-secret"
        }
        val scope = ServerCacheScope.from(storage.serverUrl!!, "user-1")
        val libraryCache = LibraryDiskCache(temporaryFolder.newFolder("library-cache"))
        libraryCache.writeMovieBatch(scope, UUID(0, 5).toString(), 1, 1L, byteArrayOf(1, 2, 3, 4))
        val watchState = temporaryFolder.newFile("watch-state.txt").apply { writeText("keep") }
        DiagnosticLog.info("Test", "token=secret")
        val store = CrashRetentionStore(diagnostics.rootDir, clockMillis = { 1L })
        store.writeCrash(
            threadName = "main",
            threadId = 1L,
            throwable = RuntimeException("token=secret"),
            snapshot = testSnapshot(),
        )
        diagnostics.exportDir.resolve("old.zip").writeText("export")

        DiagnosticsMaintenance.clearDiagnostics(diagnostics)

        assertTrue(DiagnosticLog.recentEntries().isEmpty())
        assertTrue(store.retainedCrashFiles().isEmpty())
        assertTrue(diagnostics.exportDir.listFiles().orEmpty().isEmpty())
        assertEquals("access-secret", storage.accessToken)
        assertEquals("refresh-secret", storage.refreshToken)
        assertTrue(libraryCache.cachedMovieBatchVersions(scope, UUID(0, 5).toString()).isNotEmpty())
        assertEquals("keep", watchState.readText())
    }

    @Test
    fun playbackLogBridgeUsesSharedRedactorAndFeedsExportableLog() {
        DiagnosticLog.clear()

        PlaybackDiagnosticLog.info(
            "Playback",
            "GET https://ferrex.local/stream?access_token=playback-ticket Authorization: Bearer session-secret",
        )

        val playbackEntry = PlaybackDiagnosticLog.recentEntries().single()
        val retainedEntry = DiagnosticLog.recentEntries(source = DiagnosticLog.Source.Playback).single()
        assertFalse(playbackEntry.message.contains("playback-ticket"))
        assertFalse(playbackEntry.message.contains("session-secret"))
        assertTrue(playbackEntry.message.contains("access_token=<redacted>"))
        assertEquals(playbackEntry.message, retainedEntry.message)
        assertEquals(DiagnosticLog.Source.Playback, retainedEntry.source)
    }

    private fun ZipFile.readText(name: String): String = getInputStream(getEntry(name)).bufferedReader().use { it.readText() }

    private fun testSnapshot(): DiagnosticsSnapshot = DiagnosticsSnapshot(
        generatedAtEpochMs = 1L,
        app = AppDiagnosticsSummary(
            applicationId = "com.ferrex.android.test",
            versionName = "0.1.0",
            versionCode = 1L,
            buildType = "debug",
            flavor = "mobile",
        ),
        runtime = RuntimeDiagnosticsSummary(
            maxMemoryBytes = 100,
            totalMemoryBytes = 80,
            freeMemoryBytes = 20,
            availableProcessors = 2,
        ),
        server = ServerDiagnosticsSummary(configured = true, canonicalOrigin = "https://ferrex.local", canonicalUrlHash = "hash"),
        auth = AuthDiagnosticsSummary(accessTokenPresent = true, refreshTokenPresent = true, sessionPresent = true),
    )
}

private class FakeAuthStorage : AuthStorage {
    override var serverUrl: String? = null
    override var accessToken: String? = null
    override var refreshToken: String? = null
    override var username: String? = null
    override var userId: String? = null
    override var userDisplayName: String? = null
    override var userAvatarUrl: String? = null
    override var sessionId: String? = null
    override var deviceSessionId: String? = null
    override var localDeviceId: String? = null
    override var requiresPinSetup: Boolean = false

    override fun storeTokens(tokens: AuthTokens, username: String?, userId: String?) {
        accessToken = tokens.accessToken
        refreshToken = tokens.refreshToken
        this.username = username
        this.userId = userId ?: tokens.userId
        sessionId = tokens.sessionId
        deviceSessionId = tokens.deviceSessionId
        requiresPinSetup = tokens.requiresPinSetup
    }

    override fun clearTokens() {
        accessToken = null
        refreshToken = null
        username = null
        userId = null
        sessionId = null
        deviceSessionId = null
        requiresPinSetup = false
    }

    override fun clearConnectionData() {
        serverUrl = null
        clearTokens()
        localDeviceId = null
    }
}
