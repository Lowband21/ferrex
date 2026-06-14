package com.ferrex.android.core.watch

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

sealed interface ContinueWatchingResult<out T> {
    data class Success<T>(val value: T) : ContinueWatchingResult<T>
    data class Failure(val message: String) : ContinueWatchingResult<Nothing>
}

interface ContinueWatchingTransport {
    suspend fun fetchContinueWatching(): ContinueWatchingResult<List<ContinueWatchingApiItem>>
}

class ContinueWatchingRepository(
    private val transport: ContinueWatchingTransport,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) {
    private val _state = MutableStateFlow(ContinueWatchingState())
    val state: StateFlow<ContinueWatchingState> = _state.asStateFlow()

    suspend fun refresh(): ContinueWatchingState = withContext(ioDispatcher) {
        val previousCards = _state.value.cards
        publish(_state.value.copy(status = ContinueWatchingStatus.Loading))
        when (val result = transport.fetchContinueWatching()) {
            is ContinueWatchingResult.Success -> {
                val cards = result.value.map(ContinueWatchingMapper::toCard)
                publish(
                    ContinueWatchingState(
                        status = if (cards.isEmpty()) ContinueWatchingStatus.Empty else ContinueWatchingStatus.Fresh(cards.size),
                        cards = cards,
                    ),
                )
            }
            is ContinueWatchingResult.Failure -> {
                if (previousCards.isNotEmpty()) {
                    publish(
                        ContinueWatchingState(
                            status = ContinueWatchingStatus.StaleOffline(result.message, previousCards.size),
                            cards = previousCards,
                        ),
                    )
                } else {
                    publish(ContinueWatchingState(status = ContinueWatchingStatus.ErrorRetryable(result.message)))
                }
            }
        }
    }

    private fun publish(state: ContinueWatchingState): ContinueWatchingState {
        _state.value = state
        return state
    }
}
