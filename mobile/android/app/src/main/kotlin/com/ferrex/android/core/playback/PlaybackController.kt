package com.ferrex.android.core.playback

import com.ferrex.android.core.api.ApiResult
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

private const val PLAYBACK_CONTROLLER_TAG = "PlaybackController"

data class PlaybackRetryPolicy(
    val maxAutoRetries: Int = 2,
    val backoffMillis: (attempt: Int) -> Long = { attempt -> 1_500L * attempt },
)

class PlaybackController(
    private val route: PlaybackRouteContract,
    private val ticketTransport: PlaybackTicketTransport,
    private val streamUrlFactory: PlaybackStreamUrlFactory,
    private val progressReporter: PlaybackProgressReporter?,
    private val resumeProgressProvider: PlaybackResumeProgressProvider? = null,
    private val scope: CoroutineScope,
    private val retryPolicy: PlaybackRetryPolicy = PlaybackRetryPolicy(),
    private val onSessionInvalidated: (PlaybackFailure) -> Unit,
    private val onProgressCommitted: () -> Unit = {},
) {
    private val _state = MutableStateFlow<PlaybackPlayerState>(PlaybackPlayerState.Idle)
    val state: StateFlow<PlaybackPlayerState> = _state.asStateFlow()

    private var workJob: Job? = null
    private var lastKnownPositionMs: Long = route.initialStartPositionMs
    private var autoRetryCount: Int = 0
    private var invalidatedSession: Boolean = false

    fun prepare() {
        autoRetryCount = 0
        invalidatedSession = false
        prepareInitial()
    }

    fun retry() {
        autoRetryCount = 0
        invalidatedSession = false
        prepareFrom(lastKnownPositionMs)
    }

    fun close() {
        workJob?.cancel()
        workJob = null
    }

    fun onPlaybackReady() {
        PlaybackDiagnosticLog.debug(PLAYBACK_CONTROLLER_TAG, "Playback renderer reached READY")
    }

    fun onPlayerError(failure: PlaybackFailure, positionMs: Long) {
        lastKnownPositionMs = positionMs.coerceAtLeast(0L)
        PlaybackDiagnosticLog.warn(
            PLAYBACK_CONTROLLER_TAG,
            "Playback error kind=${failure.kind} http=${failure.httpStatusCode ?: "none"} positionMs=$lastKnownPositionMs attempt=$autoRetryCount/${retryPolicy.maxAutoRetries}: ${failure.message}",
        )
        handleFailure(failure)
    }

    fun reportProgress(positionMs: Long, durationMs: Long) {
        val safePositionMs = positionMs.coerceAtLeast(0L)
        val safeDurationMs = durationMs.coerceAtLeast(0L)
        if (safeDurationMs <= 0L) return
        lastKnownPositionMs = safePositionMs
        scope.launch {
            when (val result = progressReporter?.reportProgress(
                route = route,
                positionSeconds = safePositionMs / 1000.0,
                durationSeconds = safeDurationMs / 1000.0,
            ) ?: ApiResult.Success(Unit)) {
                is ApiResult.Success -> onProgressCommitted()
                is ApiResult.HttpError -> {
                    val failure = PlaybackFailureMapper.fromHttpStatus(result.code, result.message)
                    if (failure.isAuthFailure) {
                        invalidateSession(failure)
                    } else {
                        PlaybackDiagnosticLog.warn(PLAYBACK_CONTROLLER_TAG, "Progress report failed: HTTP ${result.code}")
                    }
                }
                else -> PlaybackDiagnosticLog.warn(PLAYBACK_CONTROLLER_TAG, "Progress report failed: ${PlaybackFailureMapper.fromApiResult(result).kind}")
            }
        }
    }

    fun onPlaybackExit(positionMs: Long, durationMs: Long) {
        reportProgress(positionMs, durationMs)
    }

    fun onPlaybackEnded(durationMs: Long) {
        lastKnownPositionMs = durationMs.coerceAtLeast(0L)
        reportProgress(lastKnownPositionMs, durationMs)
    }

    private fun prepareInitial() {
        workJob?.cancel()
        workJob = scope.launch {
            val positionMs = resolveInitialStartPosition() ?: return@launch
            fetchTicketAndPublish(positionMs)
        }
    }

    private fun prepareFrom(positionMs: Long) {
        workJob?.cancel()
        workJob = scope.launch {
            fetchTicketAndPublish(positionMs.coerceAtLeast(0L))
        }
    }

    private suspend fun resolveInitialStartPosition(): Long? {
        if (!PlaybackStartPositionResolver.requiresServerResume(route)) {
            lastKnownPositionMs = route.initialStartPositionMs
            return lastKnownPositionMs
        }

        _state.value = PlaybackPlayerState.Loading(
            message = "Checking resume position…",
            retryAttempt = autoRetryCount,
            maxRetryAttempts = retryPolicy.maxAutoRetries,
        )

        val result = resumeProgressProvider?.fetchResumeProgress(route.logicalMediaId)
        if (result == null) {
            lastKnownPositionMs = 0L
            return lastKnownPositionMs
        }

        return when (result) {
            is ApiResult.Success -> {
                lastKnownPositionMs = PlaybackStartPositionResolver.fromServerProgress(result.data)
                PlaybackDiagnosticLog.info(
                    PLAYBACK_CONTROLLER_TAG,
                    "Resolved server resume media=${route.logicalMediaId} positionMs=$lastKnownPositionMs",
                )
                lastKnownPositionMs
            }
            is ApiResult.HttpError -> {
                val failure = PlaybackFailureMapper.fromHttpStatus(result.code, result.message)
                if (failure.isAuthFailure) {
                    invalidateSession(failure)
                    null
                } else {
                    PlaybackDiagnosticLog.warn(
                        PLAYBACK_CONTROLLER_TAG,
                        "Resume lookup failed with HTTP ${result.code}; starting at 0",
                    )
                    lastKnownPositionMs = 0L
                    lastKnownPositionMs
                }
            }
            else -> {
                PlaybackDiagnosticLog.warn(
                    PLAYBACK_CONTROLLER_TAG,
                    "Resume lookup failed: ${PlaybackFailureMapper.fromApiResult(result).kind}; starting at 0",
                )
                lastKnownPositionMs = 0L
                lastKnownPositionMs
            }
        }
    }

    private suspend fun fetchTicketAndPublish(positionMs: Long) {
        _state.value = PlaybackPlayerState.Loading(
            retryAttempt = autoRetryCount,
            maxRetryAttempts = retryPolicy.maxAutoRetries,
        )
        PlaybackDiagnosticLog.info(
            PLAYBACK_CONTROLLER_TAG,
            "Preparing ticketed playback media=${route.targetMediaId} positionMs=$positionMs attempt=$autoRetryCount/${retryPolicy.maxAutoRetries}",
        )

        when (val result = ticketTransport.fetchTicket(route.targetMediaId)) {
            is ApiResult.Success -> {
                val streamUrl = try {
                    streamUrlFactory.streamUrl(route.targetMediaId, result.data)
                } catch (e: IllegalArgumentException) {
                    handleFailure(PlaybackFailureMapper.network(e.localizedMessage ?: "Invalid stream URL"))
                    return
                } catch (e: IllegalStateException) {
                    handleFailure(PlaybackFailureMapper.network(e.localizedMessage ?: "Server URL is not configured"))
                    return
                }
                val prepared = PreparedPlayback(
                    route = route,
                    streamUrl = streamUrl,
                    startPositionMs = positionMs,
                    ticketExpiresInSeconds = result.data.expiresInSeconds,
                )
                PlaybackDiagnosticLog.info(
                    PLAYBACK_CONTROLLER_TAG,
                    "Playback ready url=${prepared.redactedStreamUrl} start=${prepared.startPositionMs}ms ticketTtl=${prepared.ticketExpiresInSeconds}s",
                )
                _state.value = PlaybackPlayerState.Ready(prepared)
            }
            else -> handleFailure(PlaybackFailureMapper.fromApiResult(result))
        }
    }

    private fun handleFailure(failure: PlaybackFailure) {
        if ((failure.isAuthFailure || failure.autoRetryable) && autoRetryCount < retryPolicy.maxAutoRetries) {
            scheduleRetry(failure)
            return
        }

        if (failure.isAuthFailure) {
            invalidateSession(failure)
            return
        }

        _state.value = PlaybackPlayerState.Error(failure)
    }

    private fun scheduleRetry(failure: PlaybackFailure) {
        autoRetryCount += 1
        val attempt = autoRetryCount
        val delayMs = retryPolicy.backoffMillis(attempt).coerceAtLeast(0L)
        PlaybackDiagnosticLog.info(
            PLAYBACK_CONTROLLER_TAG,
            "Retrying playback after ${failure.kind} in ${delayMs}ms attempt=$attempt/${retryPolicy.maxAutoRetries}",
        )
        workJob = scope.launch {
            _state.value = PlaybackPlayerState.Loading(
                message = "Reconnecting…",
                retryAttempt = attempt,
                maxRetryAttempts = retryPolicy.maxAutoRetries,
            )
            delay(delayMs)
            fetchTicketAndPublish(lastKnownPositionMs)
        }
    }

    private fun invalidateSession(failure: PlaybackFailure) {
        if (invalidatedSession) return
        invalidatedSession = true
        _state.value = PlaybackPlayerState.SessionInvalidated(failure)
        PlaybackDiagnosticLog.warn(
            PLAYBACK_CONTROLLER_TAG,
            "Invalidating app session after playback auth failure http=${failure.httpStatusCode ?: "none"}",
        )
        onSessionInvalidated(failure)
    }
}
