package com.ferrex.android.core.diagnostics

import java.io.PrintWriter
import java.io.StringWriter
import java.security.MessageDigest

/**
 * Shared secret redactor for every Android diagnostic surface.
 *
 * All retained logs, crash reports, exported headers/URLs, throwable strings,
 * and JSON/body-like payload snippets should pass through this object before
 * they are written to memory or disk.
 */
object DiagnosticsRedactor {
    private const val REDACTED = "<redacted>"

    private val secretNameAlternation = listOf(
        "authorization",
        "access[_-]?token",
        "refresh[_-]?token",
        "id[_-]?token",
        "token",
        "playback[_-]?ticket",
        "ticket",
        "password",
        "passwd",
        "pin",
        "pin[_-]?proof",
        "client[_-]?proof",
        "cookie",
        "set[_-]?cookie",
        "session[_-]?id",
        "device[_-]?session[_-]?id",
        "local[_-]?device[_-]?id",
        "private[_-]?key",
        "device[_-]?private[_-]?key",
        "signature",
        "device[_-]?signature",
        "api[_-]?key",
        "secret",
    ).joinToString("|")

    private val unquotedSecretNameAlternation = listOf(
        "access[_-]?token",
        "refresh[_-]?token",
        "id[_-]?token",
        "token",
        "playback[_-]?ticket",
        "ticket",
        "password",
        "passwd",
        "pin",
        "pin[_-]?proof",
        "client[_-]?proof",
        "session[_-]?id",
        "device[_-]?session[_-]?id",
        "local[_-]?device[_-]?id",
        "private[_-]?key",
        "device[_-]?private[_-]?key",
        "signature",
        "device[_-]?signature",
        "api[_-]?key",
        "secret",
    ).joinToString("|")

    private val authorizationHeaderPattern = Regex(
        pattern = "(?i)\\b(authorization\\s*[:=]\\s*)(bearer|basic)\\s+[^\\s,;\\r\\n]+",
    )
    private val bearerOrBasicPattern = Regex(
        pattern = "(?i)\\b(bearer|basic)\\s+[^\\s,;\\r\\n]+",
    )
    private val cookieHeaderPattern = Regex(
        pattern = "(?i)\\b((?:set-)?cookie\\s*[:=]\\s*)[^\\r\\n]+",
    )
    private val queryOrFormPattern = Regex(
        pattern = "(?i)(^|[?&;\\s])((?:$secretNameAlternation)\\s*=\\s*)[^&#;\\s]+",
    )
    private val quotedKeyValuePattern = Regex(
        pattern = "(?i)([\\\"'](?:$secretNameAlternation)[\\\"']\\s*:\\s*[\\\"'])[^\\\"'\\r\\n]*([\\\"'])",
    )
    private val quotedPrimitiveKeyValuePattern = Regex(
        pattern = "(?i)([\\\"'](?:$secretNameAlternation)[\\\"']\\s*:\\s*)(?![\\\"'])[^,}\\]\\s]+",
    )
    private val unquotedKeyValuePattern = Regex(
        pattern = "(?i)(\\b(?:$unquotedSecretNameAlternation)\\b\\s*[:=]\\s*)(?!$REDACTED)[^,}\\]\\s&;]+",
    )
    private val pemPrivateKeyPattern = Regex(
        pattern = "-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
        options = setOf(RegexOption.DOT_MATCHES_ALL),
    )

    fun redact(value: String?): String? = value?.let(::redactText)

    fun redactText(value: String): String {
        if (value.isEmpty()) return value
        return value
            .replace(pemPrivateKeyPattern, "-----BEGIN PRIVATE KEY-----$REDACTED-----END PRIVATE KEY-----")
            .replace(authorizationHeaderPattern) { match ->
                "${match.groupValues[1]}${match.groupValues[2]} $REDACTED"
            }
            .replace(cookieHeaderPattern) { match -> "${match.groupValues[1]}$REDACTED" }
            .replace(bearerOrBasicPattern) { match -> "${match.groupValues[1]} $REDACTED" }
            .replace(queryOrFormPattern) { match -> "${match.groupValues[1]}${match.groupValues[2]}$REDACTED" }
            .replace(quotedKeyValuePattern) { match -> "${match.groupValues[1]}$REDACTED${match.groupValues[2]}" }
            .replace(quotedPrimitiveKeyValuePattern) { match -> "${match.groupValues[1]}$REDACTED" }
            .replace(unquotedKeyValuePattern) { match -> "${match.groupValues[1]}$REDACTED" }
    }

    fun redactHeader(name: String, value: String?): String? {
        val headerValue = value ?: return null
        return if (name.isSecretHeaderName()) REDACTED else redactText(headerValue)
    }

    fun redactHeaders(headers: Map<String, String>): Map<String, String> =
        headers.toSortedMap(String.CASE_INSENSITIVE_ORDER).mapValues { (name, value) ->
            redactHeader(name, value).orEmpty()
        }

    fun redactUrl(url: String): String = redactText(url)

    fun redactThrowable(throwable: Throwable, maxChars: Int = 16_000): String =
        redactText(throwable.stackTraceToStringCompat()).take(maxChars.coerceAtLeast(0))

    private fun String.isSecretHeaderName(): Boolean {
        val normalized = trim().lowercase()
        return normalized == "authorization" || normalized == "cookie" || normalized == "set-cookie" || normalized.endsWith("-token")
    }
}

internal fun String.sha256Hex(): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(toByteArray(Charsets.UTF_8))
    return digest.joinToString(separator = "") { byte -> "%02x".format(byte) }
}

private fun Throwable.stackTraceToStringCompat(): String {
    val writer = StringWriter()
    printStackTrace(PrintWriter(writer))
    return writer.toString()
}
