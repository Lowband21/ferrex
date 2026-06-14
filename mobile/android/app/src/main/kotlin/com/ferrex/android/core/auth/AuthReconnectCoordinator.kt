package com.ferrex.android.core.auth

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

enum class AuthReconnectTrigger {
    Manual,
    Connectivity,
    Backoff,
}

sealed interface AuthReconnectResult {
    data object Online : AuthReconnectResult
    data object TemporaryOffline : AuthReconnectResult
    data object Terminal : AuthReconnectResult
}

class AuthReconnectCoordinator(
    private val scope: CoroutineScope,
    backoffDelaysMillis: List<Long> = DEFAULT_BACKOFF_DELAYS_MILLIS,
    private val attemptReconnect: suspend (AuthReconnectTrigger) -> AuthReconnectResult,
) {
    private val backoffDelaysMillis = backoffDelaysMillis.map { it.coerceAtLeast(0L) }
        .ifEmpty { DEFAULT_BACKOFF_DELAYS_MILLIS }
    private var retryJob: Job? = null
    private var backoffIndex = 0

    fun retryNow() {
        backoffIndex = 0
        launchAttempt(AuthReconnectTrigger.Manual, delayMillis = 0L, replaceExisting = true)
    }

    fun notifyConnectivityAvailable() {
        backoffIndex = 0
        launchAttempt(AuthReconnectTrigger.Connectivity, delayMillis = 0L, replaceExisting = true)
    }

    fun scheduleBackoffRetry() {
        if (retryJob?.isActive == true) return
        launchAttempt(AuthReconnectTrigger.Backoff, delayMillis = nextBackoffDelayMillis(), replaceExisting = false)
    }

    fun markOnline() {
        backoffIndex = 0
        retryJob?.cancel()
        retryJob = null
    }

    fun cancel() {
        backoffIndex = 0
        retryJob?.cancel()
        retryJob = null
    }

    private fun launchAttempt(
        trigger: AuthReconnectTrigger,
        delayMillis: Long,
        replaceExisting: Boolean,
    ) {
        if (replaceExisting) {
            retryJob?.cancel()
        }
        retryJob = scope.launch {
            if (delayMillis > 0L) delay(delayMillis)
            val result = try {
                attemptReconnect(trigger)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                AuthReconnectResult.TemporaryOffline
            }
            retryJob = null
            when (result) {
                AuthReconnectResult.Online -> backoffIndex = 0
                AuthReconnectResult.TemporaryOffline -> scheduleBackoffRetry()
                AuthReconnectResult.Terminal -> backoffIndex = 0
            }
        }
    }

    private fun nextBackoffDelayMillis(): Long {
        val delay = backoffDelaysMillis[backoffIndex.coerceAtMost(backoffDelaysMillis.lastIndex)]
        if (backoffIndex < Int.MAX_VALUE) {
            backoffIndex += 1
        }
        return delay
    }

    companion object {
        val DEFAULT_BACKOFF_DELAYS_MILLIS = listOf(1_000L, 2_000L, 5_000L, 10_000L, 30_000L)
    }
}
