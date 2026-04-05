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
    // Fallback chain: primary_poster_iid → poster_path (TMDB CDN)

    fun backdropUrl(movie: MovieReference): String? {
        movie.details?.primaryBackdropIid?.let { iid ->
            return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
        }
        movie.details?.backdropPath?.let { path ->
            return "https://image.tmdb.org/t/p/w780$path"
        }
        return null
    }

    fun posterUrl(movie: MovieReference): String? {
        movie.details?.primaryPosterIid?.let { iid ->
            return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
        }
        movie.details?.posterPath?.let { path ->
            return "https://image.tmdb.org/t/p/w342$path"
        }
        return null
    }

    fun seriesPosterUrl(series: SeriesReference): String? {
        series.details?.primaryPosterIid?.let { iid ->
            return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
        }
        series.details?.posterPath?.let { path ->
            return "https://image.tmdb.org/t/p/w342$path"
        }
        return null
    }

    fun seriesBackdropUrl(series: SeriesReference): String? {
        series.details?.primaryBackdropIid?.let { iid ->
            return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
        }
        series.details?.backdropPath?.let { path ->
            return "https://image.tmdb.org/t/p/w780$path"
        }
        return null
    }

    /**
     * Build the stream URL using the media FILE's ID (not the movie's ID).
     * The stream endpoint looks up by media_file.id, not movie.id.
     */
    fun streamUrl(movie: MovieReference): String? {
        val fileId = movie.file?.id?.toUuidString() ?: return null
        return "${serverConfig.serverUrl}/api/v1/stream/$fileId"
    }
}

sealed interface DetailUiState {
    data object Loading : DetailUiState
    data class MovieDetail(val movie: MovieReference) : DetailUiState
    data class SeriesDetail(val series: SeriesReference) : DetailUiState
    data class Error(val message: String) : DetailUiState
}
