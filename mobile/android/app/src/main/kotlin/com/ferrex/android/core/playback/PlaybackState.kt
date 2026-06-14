package com.ferrex.android.core.playback

import com.ferrex.android.core.api.ApiResult

sealed interface PlaybackPlayerState {
    data object Idle : PlaybackPlayerState

    data class Loading(
        val message: String = "Preparing playback…",
        val retryAttempt: Int = 0,
        val maxRetryAttempts: Int = 0,
    ) : PlaybackPlayerState

    data class Ready(
        val prepared: PreparedPlayback,
    ) : PlaybackPlayerState

    data class Error(
        val failure: PlaybackFailure,
        val actions: PlaybackRecoveryActions = PlaybackRecoveryActions.forFailure(failure),
    ) : PlaybackPlayerState

    data class SessionInvalidated(
        val failure: PlaybackFailure,
    ) : PlaybackPlayerState
}

data class PreparedPlayback(
    val route: PlaybackRouteContract,
    val streamUrl: String,
    val startPositionMs: Long,
    val ticketExpiresInSeconds: Long,
) {
    val redactedStreamUrl: String = PlaybackDiagnosticLog.redact(streamUrl)
}

data class PlaybackRecoveryActions(
    val retry: Boolean,
    val changeServer: Boolean,
    val signOut: Boolean,
) {
    companion object {
        fun forFailure(failure: PlaybackFailure): PlaybackRecoveryActions = PlaybackRecoveryActions(
            retry = failure.userRetryable,
            changeServer = true,
            signOut = true,
        )
    }
}

data class PlaybackFailure(
    val kind: PlaybackFailureKind,
    val message: String,
    val httpStatusCode: Int? = null,
    val autoRetryable: Boolean = false,
    val userRetryable: Boolean = true,
) {
    val isAuthFailure: Boolean
        get() = kind == PlaybackFailureKind.Unauthorized || kind == PlaybackFailureKind.Forbidden
}

enum class PlaybackFailureKind {
    Unauthorized,
    Forbidden,
    MissingFile,
    LibraryOffline,
    Network,
    Timeout,
    Server,
    UnsupportedFormat,
    Decoder,
    Unknown,
}

object PlaybackFailureMapper {
    fun fromApiResult(result: ApiResult<*>): PlaybackFailure = when (result) {
        is ApiResult.HttpError -> fromHttpStatus(result.code, result.message)
        is ApiResult.NetworkError -> PlaybackFailure(
            kind = PlaybackFailureKind.Network,
            message = result.message.ifBlank { "Network unavailable while preparing playback." },
            autoRetryable = true,
        )
        is ApiResult.ServerError -> PlaybackFailure(
            kind = PlaybackFailureKind.Server,
            message = result.message.ifBlank { "The server could not prepare playback." },
            autoRetryable = true,
        )
        ApiResult.EmptyBody -> PlaybackFailure(
            kind = PlaybackFailureKind.Server,
            message = "The server returned an empty playback response.",
            autoRetryable = true,
        )
        is ApiResult.ParseError -> PlaybackFailure(
            kind = PlaybackFailureKind.Server,
            message = "The playback response was not understood.",
            autoRetryable = false,
        )
        is ApiResult.Success -> PlaybackFailure(
            kind = PlaybackFailureKind.Unknown,
            message = "Playback failed unexpectedly.",
            autoRetryable = false,
        )
    }

    fun fromHttpStatus(statusCode: Int, message: String? = null): PlaybackFailure = when (statusCode) {
        401 -> PlaybackFailure(
            kind = PlaybackFailureKind.Unauthorized,
            message = "Playback authorization expired. Ferrex will retry with a fresh playback ticket.",
            httpStatusCode = statusCode,
            autoRetryable = true,
            userRetryable = false,
        )
        403 -> PlaybackFailure(
            kind = PlaybackFailureKind.Forbidden,
            message = "This session is not allowed to stream the selected media.",
            httpStatusCode = statusCode,
            autoRetryable = true,
            userRetryable = false,
        )
        404 -> PlaybackFailure(
            kind = PlaybackFailureKind.MissingFile,
            message = "The media file is missing on the server. Retry after the library is available, change server, or sign out.",
            httpStatusCode = statusCode,
            autoRetryable = false,
        )
        408, 429 -> PlaybackFailure(
            kind = PlaybackFailureKind.Timeout,
            message = "The stream timed out before playback could continue.",
            httpStatusCode = statusCode,
            autoRetryable = true,
        )
        503 -> PlaybackFailure(
            kind = PlaybackFailureKind.LibraryOffline,
            message = "The media library appears to be offline. Retry, change server, or sign out.",
            httpStatusCode = statusCode,
            autoRetryable = false,
        )
        in 500..599 -> PlaybackFailure(
            kind = PlaybackFailureKind.Server,
            message = message?.takeIf { it.isNotBlank() } ?: "The server could not stream this media.",
            httpStatusCode = statusCode,
            autoRetryable = true,
        )
        else -> PlaybackFailure(
            kind = PlaybackFailureKind.Unknown,
            message = message?.takeIf { it.isNotBlank() } ?: "Playback failed with HTTP $statusCode.",
            httpStatusCode = statusCode,
            autoRetryable = false,
        )
    }

    fun network(message: String = "Network connection failed while streaming."): PlaybackFailure = PlaybackFailure(
        kind = PlaybackFailureKind.Network,
        message = message,
        autoRetryable = true,
    )

    fun timeout(message: String = "The stream connection timed out."): PlaybackFailure = PlaybackFailure(
        kind = PlaybackFailureKind.Timeout,
        message = message,
        autoRetryable = true,
    )

    fun unsupported(message: String = "This media format is not supported on this device."): PlaybackFailure = PlaybackFailure(
        kind = PlaybackFailureKind.UnsupportedFormat,
        message = message,
        autoRetryable = false,
        userRetryable = false,
    )

    fun decoder(message: String = "The device could not initialize a decoder for this media."): PlaybackFailure = PlaybackFailure(
        kind = PlaybackFailureKind.Decoder,
        message = message,
        autoRetryable = false,
        userRetryable = false,
    )

    fun unknown(message: String = "Playback failed unexpectedly."): PlaybackFailure = PlaybackFailure(
        kind = PlaybackFailureKind.Unknown,
        message = message,
        autoRetryable = false,
    )
}
