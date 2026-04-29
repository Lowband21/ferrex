package com.ferrex.android.ui.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.watch.ContinueWatchingData
import com.ferrex.android.core.watch.ContinueWatchingItem
import com.ferrex.android.core.watch.WatchProgress
import com.ferrex.android.core.watch.WatchService
import com.ferrex.android.core.watch.WatchStateCoordinator
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

/**
 * ViewModel for the home screen.
 *
 * Manages:
 * - Continue watching list (GET /watch/continue)
 * - Watch state for progress indicators on poster cards
 */
@HiltViewModel
class HomeViewModel @Inject constructor(
    private val watchService: WatchService,
    private val watchStateCoordinator: WatchStateCoordinator,
    private val serverConfig: ServerConfig,
) : ViewModel() {

    private val _continueWatching = MutableStateFlow<List<ContinueWatchingItem>>(emptyList())
    val continueWatching: StateFlow<List<ContinueWatchingItem>> = _continueWatching.asStateFlow()

    /** Watch progress keyed by media file ID, for poster progress indicators. */
    private val _watchProgressMap = MutableStateFlow<Map<String, WatchProgress>>(emptyMap())
    val watchProgressMap: StateFlow<Map<String, WatchProgress>> = _watchProgressMap.asStateFlow()

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _hasLoaded = MutableStateFlow(false)
    val hasLoaded: StateFlow<Boolean> = _hasLoaded.asStateFlow()

    init {
        refresh()

        viewModelScope.launch {
            watchStateCoordinator.events.collect {
                refresh()
            }
        }
    }

    fun refresh() {
        viewModelScope.launch {
            _isLoading.value = true
            try {
                coroutineScope {
                    val continueWatching = async { fetchContinueWatching() }
                    val watchState = async { fetchWatchState() }
                    continueWatching.await()
                    watchState.await()
                }
            } finally {
                _hasLoaded.value = true
                _isLoading.value = false
            }
        }
    }

    private suspend fun fetchContinueWatching() {
        when (val result = watchService.getContinueWatching()) {
            is ApiResult.Success -> {
                _continueWatching.value = result.data.items
            }
            else -> {} // Continue watching is optional — don't block the home screen
        }
    }

    private suspend fun fetchWatchState() {
        when (val result = watchService.getWatchState()) {
            is ApiResult.Success -> {
                _watchProgressMap.value = result.data
            }
            else -> {}
        }
    }

    /**
     * Build poster URL for a continue watching item.
     */
    fun posterUrl(item: ContinueWatchingItem): String? {
        return item.posterIid?.let { iid ->
            "${serverConfig.serverUrl}/api/v1/images/iid/$iid"
        }
    }
}
