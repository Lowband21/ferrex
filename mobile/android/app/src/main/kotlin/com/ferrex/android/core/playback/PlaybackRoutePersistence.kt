package com.ferrex.android.core.playback

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.MediaRouteArgs
import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import java.util.Base64

/**
 * Small, versioned codecs for playback-related UI state that must survive Android
 * configuration changes without leaking across authenticated server/user scopes.
 */
object PlaybackRoutePersistence {
    private const val PREFIX = "playback-route"
    private const val VERSION = "1"

    fun scopeKey(serverUrl: String, userId: String): String = pack("auth-scope", VERSION, serverUrl, userId)

    fun encode(route: PlaybackRouteContract): String = pack(
        PREFIX,
        VERSION,
        route.targetMediaId,
        route.logicalMediaId,
        route.mediaType.routeValue,
        route.startPositionSeconds?.toString(),
        route.startOver.toString(),
        route.sourceDetailRoute,
    )

    fun decode(encoded: String): PlaybackRouteContract? {
        val fields = unpack(encoded) ?: return null
        if (fields.size != 8 || fields[0] != PREFIX || fields[1] != VERSION) return null
        val mediaType = fields[4]?.let(BrowseMediaType::fromApi)?.takeIf { it != BrowseMediaType.Unknown } ?: return null
        val targetMediaId = fields[2]?.takeIf { it.isNotBlank() } ?: return null
        val logicalMediaId = fields[3]?.takeIf { it.isNotBlank() } ?: return null
        val startPositionSeconds = fields[5]?.toDoubleOrNull()?.takeIf { it >= 0.0 }
        val startOver = when (fields[6]) {
            "true" -> true
            "false" -> false
            else -> return null
        }
        val sourceDetailRoute = fields[7]?.takeIf { it.isNotBlank() } ?: return null
        return PlaybackRouteContract(
            targetMediaId = targetMediaId,
            logicalMediaId = logicalMediaId,
            mediaType = mediaType,
            startPositionSeconds = startPositionSeconds,
            startOver = startOver,
            sourceDetailRoute = sourceDetailRoute,
        )
    }
}

object MediaRoutePersistence {
    private const val PREFIX = "media-route"
    private const val VERSION = "1"

    fun encode(route: MediaRouteArgs): String = pack(
        PREFIX,
        VERSION,
        route.mediaType.routeValue,
        route.mediaId,
        route.libraryId,
        route.sourceSurface.routeValue,
    )

    fun decode(encoded: String): MediaRouteArgs? {
        val fields = unpack(encoded) ?: return null
        if (fields.size != 6 || fields[0] != PREFIX || fields[1] != VERSION) return null
        return buildRoute(
            mediaTypeValue = fields[2],
            mediaId = fields[3],
            libraryId = fields[4],
            sourceSurfaceValue = fields[5],
        )
    }

    fun decodeRouteString(routeString: String): MediaRouteArgs? {
        val (path, query) = routeString.split("?", limit = 2).let { parts ->
            parts[0] to parts.getOrNull(1).orEmpty()
        }
        val pathSegments = path.split('/')
        if (pathSegments.size != 3 || pathSegments[0] != "media") return null
        val queryParameters = query.split('&')
            .filter { it.isNotBlank() }
            .mapNotNull { entry ->
                val parts = entry.split('=', limit = 2)
                val key = parts.getOrNull(0)?.urlDecodeOrNull()?.takeIf { it.isNotBlank() } ?: return@mapNotNull null
                val value = parts.getOrNull(1)?.urlDecodeOrNull().orEmpty()
                key to value
            }
            .toMap()
        return buildRoute(
            mediaTypeValue = pathSegments[1],
            mediaId = pathSegments[2],
            libraryId = queryParameters["libraryId"],
            sourceSurfaceValue = queryParameters["source"],
        )
    }

    private fun buildRoute(
        mediaTypeValue: String?,
        mediaId: String?,
        libraryId: String?,
        sourceSurfaceValue: String?,
    ): MediaRouteArgs? {
        val mediaType = mediaTypeValue?.let(BrowseMediaType::fromApi)?.takeIf { it != BrowseMediaType.Unknown } ?: return null
        val sourceSurface = BrowseSourceSurface.entries.firstOrNull { it.routeValue == sourceSurfaceValue } ?: return null
        val safeMediaId = mediaId?.takeIf { it.isNotBlank() } ?: return null
        return MediaRouteArgs(
            mediaType = mediaType,
            mediaId = safeMediaId,
            libraryId = libraryId?.takeIf { it.isNotBlank() },
            sourceSurface = sourceSurface,
        )
    }
}

private const val NULL_FIELD = "~"

private fun pack(vararg fields: String?): String = fields.joinToString("|") { field ->
    field?.let(::encodeField) ?: NULL_FIELD
}

private fun unpack(encoded: String): List<String?>? = runCatching {
    encoded.split('|').map { field ->
        if (field == NULL_FIELD) null else decodeField(field)
    }
}.getOrNull()

private fun encodeField(value: String): String = Base64.getUrlEncoder()
    .withoutPadding()
    .encodeToString(value.toByteArray(StandardCharsets.UTF_8))

private fun decodeField(value: String): String = String(
    Base64.getUrlDecoder().decode(value),
    StandardCharsets.UTF_8,
)

private fun String.urlDecodeOrNull(): String? = runCatching {
    URLDecoder.decode(this, StandardCharsets.UTF_8.name())
}.getOrNull()
