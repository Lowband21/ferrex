package com.ferrex.android.ui.detail

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.toUuidString
import dagger.hilt.android.lifecycle.HiltViewModel
import ferrex.media.MovieReference
import ferrex.media.SeriesReference
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject

/**
 * ViewModel for movie and series detail screens.
 *
 * Loads media data from the [LibraryRepository]'s in-memory batch cache
 * rather than making a network call. The batch sync already fetched full
 * [EnhancedMovieDetails] / [EnhancedSeriesDetails] as part of the
 * FlatBuffers batch data, so detail views are instant.
 */
@HiltViewModel
class DetailViewModel @Inject constructor(
    savedStateHandle: SavedStateHandle,
    private val serverConfig: ServerConfig,
    private val repository: LibraryRepository,
) : ViewModel() {

    val mediaId: String = savedStateHandle.get<String>("movieId")
        ?: savedStateHandle.get<String>("seriesId")
        ?: ""

    private val isMovie: Boolean = savedStateHandle.get<String>("movieId") != null

    private val _uiState = MutableStateFlow<DetailUiState>(DetailUiState.Loading)
    val uiState: StateFlow<DetailUiState> = _uiState.asStateFlow()

    init {
        if (mediaId.isNotEmpty()) {
            loadDetails()
        }
    }

    private fun loadDetails() {
        // Look up from the locally cached batch data — no network call needed.
        // The batch sync already fetched full details.
        if (isMovie) {
            val movie = repository.findMovieByUuid(mediaId)
            if (movie != null) {
                _uiState.value = DetailUiState.MovieDetail(movie)
            } else {
                _uiState.value = DetailUiState.Error("Movie not found in cache")
            }
        } else {
            val series = repository.findSeriesByUuid(mediaId)
            if (series != null) {
                _uiState.value = DetailUiState.SeriesDetail(series)
            } else {
                _uiState.value = DetailUiState.Error("Series not found in cache")
            }
        }
    }

    // ── Image URL helpers ───────────────────────────────────────────

    fun backdropUrl(movie: MovieReference): String? {
        val iid = movie.details?.primaryBackdropIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
    }

    fun posterUrl(movie: MovieReference): String? {
        val iid = movie.details?.primaryPosterIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
    }

    fun seriesPosterUrl(series: SeriesReference): String? {
        val iid = series.details?.primaryPosterIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
    }

    fun seriesBackdropUrl(series: SeriesReference): String? {
        val iid = series.details?.primaryBackdropIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
    }

    fun streamUrl(): String =
        "${serverConfig.serverUrl}/api/v1/stream/$mediaId"
}

sealed interface DetailUiState {
    data object Loading : DetailUiState
    data class MovieDetail(val movie: MovieReference) : DetailUiState
    data class SeriesDetail(val series: SeriesReference) : DetailUiState
    data class Error(val message: String) : DetailUiState
}
