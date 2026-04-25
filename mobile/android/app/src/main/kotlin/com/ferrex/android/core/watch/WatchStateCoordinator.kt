package com.ferrex.android.core.watch

import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Lightweight in-process invalidation bus for watch-derived UI.
 *
 * Used so Home and detail surfaces can react immediately after local watch
 * mutations instead of waiting for passive polling or lifecycle resumes.
 */
@Singleton
class WatchStateCoordinator @Inject constructor() {
    private val _events = MutableSharedFlow<WatchStateInvalidation>(
        replay = 0,
        extraBufferCapacity = 8,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )

    val events: SharedFlow<WatchStateInvalidation> = _events

    fun notifyWatchStateChanged(reason: String) {
        _events.tryEmit(WatchStateInvalidation(reason = reason))
    }
}

data class WatchStateInvalidation(
    val reason: String,
)
