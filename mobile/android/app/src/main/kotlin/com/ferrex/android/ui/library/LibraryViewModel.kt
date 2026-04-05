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
import ferrex.media.MovieReference
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

    fun selectLibrary(libraryId: String) {
        _selectedLibraryId.value = libraryId
        viewModelScope.launch {
            repository.syncAndFetch(libraryId)
        }
    }

    /**
     * Build a poster URL from a movie reference.
     * Uses the primary_poster_iid from the movie's details.
     * The /images/iid/ endpoint resolves the IID to the blob token server-side.
     */
    fun posterUrlForMovie(movie: MovieReference): String? {
        val iid = movie.details?.primaryPosterIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/iid/${iid.toUuidString()}"
    }
}
