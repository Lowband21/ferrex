package com.ferrex.android.core.library

import com.ferrex.android.core.api.ServerConfig
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import java.security.MessageDigest
import java.util.Locale

/**
 * Canonical cache identity for data that came from one Ferrex server.
 *
 * Authenticated library payloads are scoped by both canonical server URL and
 * user id so one account never reuses another account's library/search/image
 * metadata. Pre-authenticated data can use a null user id, but protected
 * library repositories should pass the current authenticated user id whenever
 * it is available.
 */
class ServerCacheScope private constructor(
    val canonicalServerUrl: String,
    val userId: String?,
    val directoryName: String,
) {
    val isAuthenticated: Boolean get() = !userId.isNullOrBlank()

    override fun equals(other: Any?): Boolean = other is ServerCacheScope &&
        canonicalServerUrl == other.canonicalServerUrl &&
        userId == other.userId &&
        directoryName == other.directoryName

    override fun hashCode(): Int {
        var result = canonicalServerUrl.hashCode()
        result = 31 * result + (userId?.hashCode() ?: 0)
        result = 31 * result + directoryName.hashCode()
        return result
    }

    override fun toString(): String = "ServerCacheScope(canonicalServerUrl=$canonicalServerUrl, userId=$userId, directoryName=$directoryName)"

    companion object {
        fun from(serverUrl: String, userId: String?): ServerCacheScope {
            val canonicalUrl = canonicalizeServerUrl(serverUrl)
            require(canonicalUrl.isNotBlank()) { "Server URL is not configured" }
            val normalizedUserId = userId?.trim()?.takeIf { it.isNotBlank() }
            val keyMaterial = buildString {
                append(canonicalUrl)
                append('\n')
                append(normalizedUserId.orEmpty())
            }
            val directoryName = buildString {
                append("server-")
                append(keyMaterial.sha256Hex().take(24))
                if (normalizedUserId == null) {
                    append("-anonymous")
                } else {
                    append("-user-")
                    append(normalizedUserId.sha256Hex().take(16))
                }
            }
            return ServerCacheScope(
                canonicalServerUrl = canonicalUrl,
                userId = normalizedUserId,
                directoryName = directoryName,
            )
        }

        fun fromOrNull(serverUrl: String?, userId: String?): ServerCacheScope? {
            val normalized = serverUrl?.let(ServerConfig::normalize).orEmpty()
            if (normalized.isBlank()) return null
            return from(normalized, userId)
        }

        fun canonicalizeServerUrl(serverUrl: String): String {
            val normalized = ServerConfig.normalize(serverUrl)
            val parsed = normalized.toHttpUrlOrNull()
            if (parsed != null) {
                val canonical = parsed.newBuilder()
                    .scheme(parsed.scheme.lowercase(Locale.US))
                    .host(parsed.host.lowercase(Locale.US))
                    .build()
                    .toString()
                    .trimEnd('/')
                return canonical
            }
            return normalized.trimEnd('/')
        }
    }
}

private fun String.sha256Hex(): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(toByteArray(Charsets.UTF_8))
    return digest.joinToString(separator = "") { byte -> "%02x".format(byte) }
}
