package com.ferrex.android.core.auth

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class AuthReconnectCoordinatorTest {
    @Test
    fun manualConnectivityAndBoundedBackoffRetriesUseInjectedScheduler() = runTest {
        val attempts = mutableListOf<Pair<AuthReconnectTrigger, Long>>()
        val results = ArrayDeque(
            listOf(
                AuthReconnectResult.TemporaryOffline,
                AuthReconnectResult.TemporaryOffline,
                AuthReconnectResult.TemporaryOffline,
                AuthReconnectResult.TemporaryOffline,
                AuthReconnectResult.Online,
            ),
        )
        val coordinator = AuthReconnectCoordinator(
            scope = backgroundScope,
            backoffDelaysMillis = listOf(100L, 200L),
        ) { trigger ->
            attempts += trigger to testScheduler.currentTime
            results.removeFirst()
        }

        coordinator.retryNow()
        runCurrent()
        coordinator.notifyConnectivityAvailable()
        runCurrent()
        advanceTimeBy(100L)
        runCurrent()
        advanceTimeBy(200L)
        runCurrent()
        advanceTimeBy(200L)
        runCurrent()

        assertEquals(
            listOf(
                AuthReconnectTrigger.Manual to 0L,
                AuthReconnectTrigger.Connectivity to 0L,
                AuthReconnectTrigger.Backoff to 100L,
                AuthReconnectTrigger.Backoff to 300L,
                AuthReconnectTrigger.Backoff to 500L,
            ),
            attempts,
        )
    }
}
