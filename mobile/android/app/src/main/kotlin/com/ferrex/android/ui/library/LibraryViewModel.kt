package com.ferrex.android.ui.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.MediaAccessor
import com.ferrex.android.core.library.SyncState
import com.ferrex.android.core.library.toUuidString
import dagger.hilt.android.lifecycle.HiltViewModel
import ferrex.common.LibraryType
import ferrex.media.MovieReference
import ferrex.media.SeriesReference
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

@HiltViewModel
class LibraryViewModel @Inject constructor(
    private val repository: LibraryRepository,
    private val serverConfig: ServerConfig,
) : ViewModel() {

    val libraries: StateFlow<List<LibraryInfo>> = repository.libraries
    val syncState: StateFlow<SyncState> = repository.syncState

    private val _selectedLibraryId = MutableStateFlow<String?>(null)
    val selectedLibraryId: StateFlow<String?> = _selectedLibraryId.asStateFlow()

    val currentMedia: StateFlow<MediaAccessor?> = repository.currentMedia

    init {
        loadLibraries()
    }

    fun loadLibraries() {
        viewModelScope.launch {
            repository.loadLibraries()
        }
    }

    private val _selectedLibraryType = MutableStateFlow<Byte>(LibraryType.Movies)
    val selectedLibraryType: StateFlow<Byte> = _selectedLibraryType.asStateFlow()

    fun selectLibrary(libraryId: String, libraryType: Byte = LibraryType.Movies) {
        _selectedLibraryId.value = libraryId
        _selectedLibraryType.value = libraryType
        viewModelScope.launch {
            repository.syncAndFetch(libraryId, libraryType)
        }
    }

    /**
     * Build a poster URL from a movie reference.
     * Fallback chain: primary_poster_iid (server-cached) → poster_path (TMDB CDN).
     */
    fun posterUrlForMovie(movie: MovieReference): String? {
        movie.details?.primaryPosterIid?.let { iid ->
            return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
        }
        movie.details?.posterPath?.let { path ->
            return "https://image.tmdb.org/t/p/w342$path"
        }
        return null
    }

    /**
     * Build a poster URL from a series reference.
     * Same fallback chain as movies: IID → TMDB CDN.
     */
    fun posterUrlForSeries(series: SeriesReference): String? {
        series.details?.primaryPosterIid?.let { iid ->
            return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
        }
        series.details?.posterPath?.let { path ->
            return "https://image.tmdb.org/t/p/w342$path"
        }
        return null
    }
}
