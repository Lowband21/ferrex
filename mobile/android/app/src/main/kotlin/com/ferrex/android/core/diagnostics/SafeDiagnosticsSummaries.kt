package com.ferrex.android.core.diagnostics

import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.auth.AuthStorage
import com.ferrex.android.core.image.ImageDiskCacheDiagnostics
import com.ferrex.android.core.library.LibraryDiskCacheDiagnostics
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.ServerCacheScope
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

object SafeAuthDiagnostics {
    fun summarize(storage: AuthStorage): AuthDiagnosticsSummary {
        val accessToken = runCatching { storage.accessToken }.getOrNull()
        val refreshToken = runCatching { storage.refreshToken }.getOrNull()
        val sessionId = runCatching { storage.sessionId }.getOrNull()
        val deviceSessionId = runCatching { storage.deviceSessionId }.getOrNull()
        val userId = runCatching { storage.userId }.getOrNull()
        val requiresPinSetup = runCatching { storage.requiresPinSetup }.getOrDefault(false)
        return AuthDiagnosticsSummary(
            accessTokenPresent = !accessToken.isNullOrBlank(),
            refreshTokenPresent = !refreshToken.isNullOrBlank(),
            sessionPresent = !sessionId.isNullOrBlank(),
            deviceSessionPresent = !deviceSessionId.isNullOrBlank(),
            userIdHash = userId?.trim()?.takeIf { it.isNotEmpty() }?.sha256Short(),
            requiresPinSetup = requiresPinSetup,
        )
    }
}

object SafeServerDiagnostics {
    fun summarize(serverUrl: String?): ServerDiagnosticsSummary {
        val normalized = serverUrl?.let(ServerConfig::normalize).orEmpty()
        if (normalized.isBlank()) return ServerDiagnosticsSummary(configured = false)

        val canonical = runCatching { ServerCacheScope.canonicalizeServerUrl(normalized) }
            .getOrDefault(normalized.trimEnd('/'))
        val parsed = canonical.toHttpUrlOrNull()
        val origin = parsed?.let { url ->
            buildString {
                append(url.scheme)
                append("://")
                append(url.host)
                if (url.port != url.defaultPort()) append(':').append(url.port)
            }
        }
        return ServerDiagnosticsSummary(
            configured = true,
            canonicalOrigin = origin,
            canonicalUrlHash = canonical.sha256Short(),
        )
    }

    private fun okhttp3.HttpUrl.defaultPort(): Int = if (scheme == "https") 443 else 80
}

object PlaybackDiagnosticsSummaryProvider {
    fun summarize(entries: List<DiagnosticLog.Entry> = DiagnosticLog.recentEntries(source = DiagnosticLog.Source.Playback)): PlaybackDiagnosticsSummary =
        PlaybackDiagnosticsSummary(
            retainedEntryCount = entries.size,
            warningCount = entries.count { it.level == DiagnosticLog.Level.Warn },
            errorCount = entries.count { it.level == DiagnosticLog.Level.Error },
            lastEventEpochMs = entries.lastOrNull()?.timestampMs,
        )
}

object SafeCacheDiagnostics {
    fun summarize(
        library: LibraryDiskCacheDiagnostics?,
        image: ImageDiskCacheDiagnostics?,
        state: LibraryRepositoryState? = null,
    ): CacheDiagnosticsSummary = CacheDiagnosticsSummary(
        library = library?.toSummary(state),
        image = image?.toSummary(),
    )

    private fun LibraryDiskCacheDiagnostics.toSummary(state: LibraryRepositoryState?): LibraryCacheDiagnosticsSummary =
        LibraryCacheDiagnosticsSummary(
            scopeDirectoryName = scopeDirectoryName,
            relativeScopePath = relativeScopePath,
            approximateBytes = approximateBytes,
            libraryListPresent = libraryListPresent,
            libraryDirectoryCount = libraryDirectoryCount,
            knownLibraryCount = state?.libraries?.size,
            cachedMovieBatchFiles = cachedMovieBatchFiles,
            cachedSeriesBundleFiles = cachedSeriesBundleFiles,
            cachedMovieCount = state?.movieLibraries?.sumOf { it.accessor.movieCount }
                ?: state?.movieAccessor?.movieCount,
            cachedSeriesCount = state?.seriesLibraries?.sumOf { it.accessor.seriesReferenceCount }
                ?: state?.seriesAccessor?.seriesReferenceCount,
            cachedEpisodeCount = state?.seriesLibraries?.sumOf { it.accessor.episodeCount }
                ?: state?.seriesAccessor?.episodeCount,
            quarantineFileCount = quarantineFileCount,
            staleOfflineMarkerPresent = staleOfflineMarkerPresent,
        )

    private fun ImageDiskCacheDiagnostics.toSummary(): ImageCacheDiagnosticsSummary = ImageCacheDiagnosticsSummary(
        scopeDirectoryName = scopeDirectoryName,
        relativeImagesPath = relativeImagesPath,
        approximateBytes = approximateBytes,
        manifestEntryFiles = manifestEntryFiles,
        coilBlobBytes = coilBlobBytes,
        quarantineFileCount = quarantineFileCount,
        staleOfflineMarkerPresent = staleOfflineMarkerPresent,
    )
}

internal fun String.sha256Short(length: Int = 16): String = sha256Hex().take(length.coerceAtLeast(1))
