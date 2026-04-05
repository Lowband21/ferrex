package com.ferrex.android.ui.search

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.core.search.SearchHit
import com.ferrex.android.core.search.SearchResult
import com.ferrex.android.core.search.SearchService
import dagger.hilt.android.lifecycle.HiltViewModel
import ferrex.media.MovieReference
import ferrex.media.SeriesReference
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.launch
import javax.inject.Inject

@HiltViewModel
class SearchViewModel @Inject constructor(
    private val searchService: SearchService,
    private val serverConfig: ServerConfig,
) : ViewModel() {

    private val _query = MutableStateFlow("")
    val query: StateFlow<String> = _query.asStateFlow()

    private val _uiState = MutableStateFlow<SearchUiState>(SearchUiState.Idle)
    val uiState: StateFlow<SearchUiState> = _uiState.asStateFlow()

    init {
        observeQuery()
    }

    fun updateQuery(newQuery: String) {
        _query.value = newQuery
        if (newQuery.isBlank()) {
            _uiState.value = SearchUiState.Idle
        }
    }

    @OptIn(FlowPreview::class)
    private fun observeQuery() {
        viewModelScope.launch {
            _query
                .debounce(300L)
                .distinctUntilChanged()
                .filter { it.length >= 2 }
                .flatMapLatest { query ->
                    flow {
                        emit(SearchUiState.Loading)
                        when (val result = searchService.search(query)) {
                            is SearchResult.Success -> {
                                emit(SearchUiState.Results(result.hits))
                            }
                            is SearchResult.Error -> {
                                emit(SearchUiState.Error(result.message))
                            }
                        }
                    }
                }
                .collect { state ->
                    _uiState.value = state
                }
        }
    }

    fun posterUrlForMovie(movie: MovieReference): String? {
        val iid = movie.details?.primaryPosterIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
    }

    fun posterUrlForSeries(series: SeriesReference): String? {
        val iid = series.details?.primaryPosterIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
    }
}

sealed interface SearchUiState {
    data object Idle : SearchUiState
    data object Loading : SearchUiState
    data class Results(val hits: List<SearchHit>) : SearchUiState
    data class Error(val message: String) : SearchUiState
}
