package com.ferrex.android.ui.detail

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.core.watch.WatchProgress
import com.ferrex.android.core.watch.WatchService
import dagger.hilt.android.lifecycle.HiltViewModel
import ferrex.details.CastMember
import ferrex.media.EpisodeReference
import ferrex.media.MovieReference
import ferrex.media.SeasonReference
import ferrex.media.SeriesReference
import ferrex.watch.EpisodeWatchState
import ferrex.watch.SeasonWatchStatus
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

/**
 * ViewModel for movie and series detail screens.
 *
 * Loads media data from the [LibraryRepository]'s in-memory batch cache
 * rather than making a network call. The batch sync already fetched full
 * [EnhancedMovieDetails] / [EnhancedSeriesDetails] as part of the
 * FlatBuffers batch data, so detail views are instant.
 *
 * Watch state (progress, completion) is fetched asynchronously from the
 * server and merged into the UI state.
 */
@HiltViewModel
class DetailViewModel @Inject constructor(
    savedStateHandle: SavedStateHandle,
    private val serverConfig: ServerConfig,
    private val repository: LibraryRepository,
    private val watchService: WatchService,
) : ViewModel() {

    val mediaId: String = savedStateHandle.get<String>("movieId")
        ?: savedStateHandle.get<String>("seriesId")
        ?: ""

    private val isMovie: Boolean = savedStateHandle.get<String>("movieId") != null

    private val _uiState = MutableStateFlow<DetailUiState>(DetailUiState.Loading)
    val uiState: StateFlow<DetailUiState> = _uiState.asStateFlow()

    /** Watch progress for the current movie (keyed by media file ID). */
    private val _watchProgress = MutableStateFlow<WatchProgress?>(null)
    val watchProgress: StateFlow<WatchProgress?> = _watchProgress.asStateFlow()

    /** Seasons for the current series. */
    private val _seasons = MutableStateFlow<List<SeasonReference>>(emptyList())
    val seasons: StateFlow<List<SeasonReference>> = _seasons.asStateFlow()

    /** Episodes for the currently selected season. */
    private val _episodes = MutableStateFlow<List<EpisodeReference>>(emptyList())
    val episodes: StateFlow<List<EpisodeReference>> = _episodes.asStateFlow()

    /** Episode watch states, keyed by "S{season}E{episode}". */
    private val _episodeStates = MutableStateFlow<Map<String, EpisodeWatchInfo>>(emptyMap())
    val episodeStates: StateFlow<Map<String, EpisodeWatchInfo>> = _episodeStates.asStateFlow()

    /** Currently selected season number. */
    private val _selectedSeason = MutableStateFlow(1)
    val selectedSeason: StateFlow<Int> = _selectedSeason.asStateFlow()

    init {
        if (mediaId.isNotEmpty()) {
            loadDetails()
            loadWatchState()
        }
    }

    private fun loadDetails() {
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
                loadSeriesEpisodes(series)
            } else {
                _uiState.value = DetailUiState.Error("Series not found in cache")
            }
        }
    }

    private fun loadSeriesEpisodes(series: SeriesReference) {
        val accessor = repository.currentMedia.value
        val seriesUuid = series.id?.toUuidString()

        // Try to load seasons/episodes from batch cache.
        // The series library grid bundle currently only contains SeriesReference
        // items. Season/episode data will be available once the server's
        // per-series bundle endpoint (GET /libraries/{id}/series-bundles/{series_id})
        // adds FlatBuffers support.
        if (accessor != null && seriesUuid != null) {
            val seasonList = accessor.seasonsForSeries(seriesUuid)
            _seasons.value = seasonList

            if (seasonList.isNotEmpty()) {
                val firstSeason = seasonList.first().seasonNumber.toInt()
                _selectedSeason.value = firstSeason
                _episodes.value = accessor.episodesForSeason(seriesUuid, firstSeason)
            }
        }

        // Default season tab to 1 (from metadata) if batch had no season items
        if (_seasons.value.isEmpty()) {
            _selectedSeason.value = 1
        }

        // Fetch series watch status from server
        val tmdbId = series.tmdbId.toLong()
        if (tmdbId > 0) {
            viewModelScope.launch {
                when (val result = watchService.getSeriesWatchStatus(tmdbId)) {
                    is ApiResult.Success -> parseSeriesWatchStatus(result.data)
                    else -> {} // Watch state is optional — don't block the UI
                }
            }
        }
    }

    fun selectSeason(seasonNumber: Int) {
        _selectedSeason.value = seasonNumber
        val state = _uiState.value
        if (state is DetailUiState.SeriesDetail) {
            val seriesUuid = state.series.id?.toUuidString() ?: return
            val accessor = repository.currentMedia.value ?: return
            _episodes.value = accessor.episodesForSeason(seriesUuid, seasonNumber)
        }
    }

    private fun parseSeriesWatchStatus(status: ferrex.watch.SeriesWatchStatus) {
        val states = mutableMapOf<String, EpisodeWatchInfo>()
        for (s in 0 until status.seasonsLength) {
            val season = status.seasons(s) ?: continue
            val seasonNum = season.key?.seasonNumber?.toInt() ?: continue
            for (e in 0 until season.episodesLength) {
                val ep = season.episodes(e) ?: continue
                val key = episodeKey(seasonNum, ep.episodeNumber.toInt())
                states[key] = EpisodeWatchInfo(
                    state = ep.state,
                    progress = ep.progress,
                )
            }
        }
        _episodeStates.value = states
    }

    private fun loadWatchState() {
        viewModelScope.launch {
            when (val result = watchService.getWatchState()) {
                is ApiResult.Success -> {
                    // For movies, match by media file ID
                    if (isMovie) {
                        val movie = (uiState.value as? DetailUiState.MovieDetail)?.movie
                        val fileId = movie?.file?.id?.toUuidString()
                        if (fileId != null) {
                            _watchProgress.value = result.data[fileId]
                        }
                    }
                }
                else -> {} // Watch state is optional
            }
        }
    }

    // ── Image URL helpers ───────────────────────────────────────────

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

    fun castPhotoUrl(member: CastMember): String? {
        member.imageId?.let { iid ->
            return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
        }
        member.profilePath?.let { path ->
            return "https://image.tmdb.org/t/p/w185$path"
        }
        return null
    }

    fun episodeStillUrl(episode: EpisodeReference): String? {
        episode.details?.primaryStillIid?.let { iid ->
            return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
        }
        episode.details?.stillPath?.let { path ->
            return "https://image.tmdb.org/t/p/w300$path"
        }
        return null
    }

    fun streamUrl(movie: MovieReference): String? {
        val fileId = movie.file?.id?.toUuidString() ?: return null
        return "${serverConfig.serverUrl}/api/v1/stream/$fileId"
    }

    fun episodeStreamFileId(episode: EpisodeReference): String? {
        return episode.file?.id?.toUuidString()
    }

    companion object {
        fun episodeKey(season: Int, episode: Int): String = "S${season}E${episode}"
    }
}

data class EpisodeWatchInfo(
    val state: Byte,
    val progress: Float,
) {
    val isCompleted: Boolean get() = state == EpisodeWatchState.Completed
    val isInProgress: Boolean get() = state == EpisodeWatchState.InProgress
}

sealed interface DetailUiState {
    data object Loading : DetailUiState
    data class MovieDetail(val movie: MovieReference) : DetailUiState
    data class SeriesDetail(val series: SeriesReference) : DetailUiState
    data class Error(val message: String) : DetailUiState
}
