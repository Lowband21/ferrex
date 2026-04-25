package com.ferrex.android.ui.detail

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.api.ApiResult
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.MediaAccessor
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.core.watch.WatchProgress
import com.ferrex.android.core.watch.WatchService
import com.ferrex.android.core.watch.WatchStateCoordinator
import dagger.hilt.android.lifecycle.HiltViewModel
import ferrex.details.CastMember
import ferrex.media.EpisodeReference
import ferrex.media.MovieReference
import ferrex.media.SeasonReference
import ferrex.media.SeriesReference
import ferrex.watch.EpisodeWatchState
import ferrex.watch.NextReason
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
    private val watchStateCoordinator: WatchStateCoordinator,
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

    /** Primary series-level action resolved from server watch state. */
    private val _seriesPrimaryAction = MutableStateFlow<SeriesPlaybackAction?>(null)
    val seriesPrimaryAction: StateFlow<SeriesPlaybackAction?> = _seriesPrimaryAction.asStateFlow()

    /** Start-over action for the series, typically the pilot episode at 0ms. */
    private val _seriesStartOverAction = MutableStateFlow<SeriesPlaybackAction?>(null)
    val seriesStartOverAction: StateFlow<SeriesPlaybackAction?> = _seriesStartOverAction.asStateFlow()

    /** Summary counts for current series watch state. */
    private val _seriesWatchSummary = MutableStateFlow<SeriesWatchSummary?>(null)
    val seriesWatchSummary: StateFlow<SeriesWatchSummary?> = _seriesWatchSummary.asStateFlow()

    /** In-flight state for explicit watched/unwatched mutations. */
    private val _isSubmittingWatchAction = MutableStateFlow(false)
    val isSubmittingWatchAction: StateFlow<Boolean> = _isSubmittingWatchAction.asStateFlow()

    /** One-shot snackbar/toast style message after watch actions. */
    private val _watchActionMessage = MutableStateFlow<String?>(null)
    val watchActionMessage: StateFlow<String?> = _watchActionMessage.asStateFlow()

    init {
        if (mediaId.isNotEmpty()) {
            loadDetails()
            loadWatchState()

            viewModelScope.launch {
                watchStateCoordinator.events.collect {
                    refreshWatchData()
                }
            }
        }
    }

    fun refreshWatchData() {
        loadWatchState()
        val series = (uiState.value as? DetailUiState.SeriesDetail)?.series
        if (series != null) {
            refreshSeriesWatchStatus(series)
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

    /** MediaAccessor for the per-series bundle (seasons + episodes). */
    private var seriesBundleAccessor: MediaAccessor? = null

    private fun loadSeriesEpisodes(series: SeriesReference) {
        val seriesUuid = series.id?.toUuidString() ?: return
        val libraryUuid = series.libraryId?.toUuidString() ?: return

        // Default season tab to 1
        _selectedSeason.value = 1

        // Fetch the per-series bundle from the server — this contains
        // the full season and episode data that the library grid bundle
        // doesn't include.
        viewModelScope.launch {
            val accessor = repository.fetchSeriesBundle(libraryUuid, seriesUuid)
            if (accessor != null) {
                seriesBundleAccessor = accessor

                val seasonList = accessor.seasonsForSeries(seriesUuid)
                _seasons.value = seasonList

                if (seasonList.isNotEmpty()) {
                    val firstSeason = seasonList.first().seasonNumber.toInt()
                    _selectedSeason.value = firstSeason
                    _episodes.value = accessor.episodesForSeason(seriesUuid, firstSeason)
                }

                _seriesStartOverAction.value = findSeriesStartOverAction(seriesUuid)
            }

            refreshSeriesWatchStatus(series)
        }
    }

    private fun refreshSeriesWatchStatus(series: SeriesReference) {
        val tmdbId = series.tmdbId.toLong()
        if (tmdbId <= 0) return

        viewModelScope.launch {
            when (val result = watchService.getSeriesWatchStatus(tmdbId)) {
                is ApiResult.Success -> parseSeriesWatchStatus(series, result.data)
                else -> {}
            }
        }
    }

    fun selectSeason(seasonNumber: Int) {
        _selectedSeason.value = seasonNumber
        val state = _uiState.value
        if (state is DetailUiState.SeriesDetail) {
            val seriesUuid = state.series.id?.toUuidString() ?: return
            val accessor = seriesBundleAccessor ?: return
            _episodes.value = accessor.episodesForSeason(seriesUuid, seasonNumber)
        }
    }

    private fun parseSeriesWatchStatus(
        series: SeriesReference,
        status: ferrex.watch.SeriesWatchStatus,
    ) {
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

        val seriesUuid = series.id?.toUuidString()
        if (_seriesStartOverAction.value == null && seriesUuid != null) {
            _seriesStartOverAction.value = findSeriesStartOverAction(seriesUuid)
        }

        _seriesWatchSummary.value = SeriesWatchSummary(
            totalEpisodes = status.totalEpisodes.toInt(),
            watchedEpisodes = status.watched.toInt(),
            inProgressEpisodes = status.inProgress.toInt(),
        )

        val nextEpisode = status.nextEpisode
        _seriesPrimaryAction.value = nextEpisode?.playableMediaId?.toUuidString()?.let { playableId ->
            val seasonNumber = nextEpisode.key.seasonNumber.toInt()
            val episodeNumber = nextEpisode.key.episodeNumber.toInt()
            val subtitle = formatEpisodeLabel(seasonNumber, episodeNumber)
            val label = when (nextEpisode.reason) {
                NextReason.FirstUnwatched -> "Next episode"
                else -> "Resume episode"
            }
            SeriesPlaybackAction(
                mediaId = playableId,
                label = label,
                subtitle = subtitle,
                startPositionMs = null,
            )
        }
    }

    private fun loadWatchState() {
        viewModelScope.launch {
            when (val result = watchService.getWatchState()) {
                is ApiResult.Success -> {
                    // For movies, match by media file ID
                    if (isMovie) {
                        val movie = (uiState.value as? DetailUiState.MovieDetail)?.movie
                        val fileId = movie?.file?.id?.toUuidString()
                        _watchProgress.value = fileId?.let { result.data[it] }
                    }
                }
                else -> {} // Watch state is optional
            }
        }
    }

    fun consumeWatchActionMessage() {
        _watchActionMessage.value = null
    }

    fun setMovieWatched(markWatched: Boolean) {
        val movie = (uiState.value as? DetailUiState.MovieDetail)?.movie ?: return
        val movieId = movie.id?.toUuidString() ?: return

        submitWatchMutation(
            successMessage = if (markWatched) "Marked movie watched" else "Marked movie unwatched",
            request = {
                if (markWatched) {
                    watchService.markMovieWatched(movieId)
                } else {
                    watchService.markMovieUnwatched(movieId)
                }
            },
            onSuccess = {
                loadWatchState()
            },
        )
    }

    fun setSeriesWatched(markWatched: Boolean) {
        val series = (uiState.value as? DetailUiState.SeriesDetail)?.series ?: return
        val tmdbId = series.tmdbId.toLong()
        if (tmdbId <= 0) {
            _watchActionMessage.value = "Series is missing a valid TMDB id"
            return
        }

        submitWatchMutation(
            successMessage = if (markWatched) "Marked series watched" else "Marked series unwatched",
            request = {
                if (markWatched) {
                    watchService.markSeriesWatched(tmdbId)
                } else {
                    watchService.markSeriesUnwatched(tmdbId)
                }
            },
            onSuccess = {
                refreshWatchData()
            },
        )
    }

    fun setEpisodeWatched(episodeId: String, markWatched: Boolean) {
        submitWatchMutation(
            successMessage = if (markWatched) "Marked episode watched" else "Marked episode unwatched",
            request = {
                if (markWatched) {
                    watchService.markEpisodeWatched(episodeId)
                } else {
                    watchService.markEpisodeUnwatched(episodeId)
                }
            },
            onSuccess = {
                refreshWatchData()
            },
        )
    }

    private fun submitWatchMutation(
        successMessage: String,
        request: suspend () -> ApiResult<Unit>,
        onSuccess: () -> Unit,
    ) {
        if (_isSubmittingWatchAction.value) return

        viewModelScope.launch {
            _isSubmittingWatchAction.value = true
            try {
                when (val result = request()) {
                    is ApiResult.Success -> {
                        _watchActionMessage.value = successMessage
                        onSuccess()
                        watchStateCoordinator.notifyWatchStateChanged(successMessage)
                    }
                    is ApiResult.HttpError -> {
                        _watchActionMessage.value = result.message.ifBlank {
                            "Watch action failed (HTTP ${result.code})"
                        }
                    }
                    is ApiResult.NetworkError -> {
                        _watchActionMessage.value =
                            result.exception.localizedMessage ?: "Watch action failed"
                    }
                }
            } finally {
                _isSubmittingWatchAction.value = false
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

    private fun findSeriesStartOverAction(seriesUuid: String): SeriesPlaybackAction? {
        val accessor = seriesBundleAccessor ?: return null
        val firstSeason = accessor.seasonsForSeries(seriesUuid).firstOrNull() ?: return null
        val firstEpisode = accessor
            .episodesForSeason(seriesUuid, firstSeason.seasonNumber.toInt())
            .firstOrNull() ?: return null
        val mediaId = firstEpisode.file?.id?.toUuidString() ?: return null

        return SeriesPlaybackAction(
            mediaId = mediaId,
            label = "Start from beginning",
            subtitle = formatEpisodeLabel(
                firstEpisode.seasonNumber.toInt(),
                firstEpisode.episodeNumber.toInt(),
            ),
            startPositionMs = 0L,
        )
    }

    companion object {
        fun episodeKey(season: Int, episode: Int): String = "S${season}E${episode}"

        fun formatEpisodeLabel(season: Int, episode: Int): String =
            "S${season.toString().padStart(2, '0')}E${episode.toString().padStart(2, '0')}"
    }
}

data class EpisodeWatchInfo(
    val state: Byte,
    val progress: Float,
) {
    val isCompleted: Boolean get() = state == EpisodeWatchState.Completed
    val isInProgress: Boolean get() = state == EpisodeWatchState.InProgress
}

data class SeriesPlaybackAction(
    val mediaId: String,
    val label: String,
    val subtitle: String? = null,
    val startPositionMs: Long? = null,
)

data class SeriesWatchSummary(
    val totalEpisodes: Int,
    val watchedEpisodes: Int,
    val inProgressEpisodes: Int,
) {
    val hasExistingProgress: Boolean
        get() = watchedEpisodes > 0 || inProgressEpisodes > 0

    val isFullyWatched: Boolean
        get() = totalEpisodes > 0 && watchedEpisodes >= totalEpisodes && inProgressEpisodes == 0
}

sealed interface DetailUiState {
    data object Loading : DetailUiState
    data class MovieDetail(val movie: MovieReference) : DetailUiState
    data class SeriesDetail(val series: SeriesReference) : DetailUiState
    data class Error(val message: String) : DetailUiState
}
