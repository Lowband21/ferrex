package com.ferrex.android.ui.detail

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.library.toUuidString
import dagger.hilt.android.lifecycle.HiltViewModel
import ferrex.library.BatchFetchResponse
import ferrex.media.MediaVariant
import ferrex.media.MovieReference
import ferrex.media.SeriesReference
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.nio.ByteBuffer
import java.nio.ByteOrder
import javax.inject.Inject

@HiltViewModel
class DetailViewModel @Inject constructor(
    savedStateHandle: SavedStateHandle,
    private val serverConfig: ServerConfig,
    private val httpClient: OkHttpClient,
) : ViewModel() {

    val mediaId: String = savedStateHandle.get<String>("movieId")
        ?: savedStateHandle.get<String>("seriesId")
        ?: ""

    private val _uiState = MutableStateFlow<DetailUiState>(DetailUiState.Loading)
    val uiState: StateFlow<DetailUiState> = _uiState.asStateFlow()

    init {
        if (mediaId.isNotEmpty()) {
            loadDetails()
        }
    }

    private fun loadDetails() {
        viewModelScope.launch {
            try {
                // Query the server for full details
                // POST /api/v1/media/query with the media ID
                val jsonBody = """{"id":"$mediaId"}"""

                val request = Request.Builder()
                    .url("${serverConfig.serverUrl}/api/v1/media/query")
                    .addHeader("Accept", "application/x-flatbuffers")
                    .post(jsonBody.toByteArray().toRequestBody("application/json".toMediaType()))
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    _uiState.value = DetailUiState.Error("Failed to load details")
                    return@launch
                }

                val bytes = response.body?.bytes()
                if (bytes == null) {
                    _uiState.value = DetailUiState.Error("Empty response")
                    return@launch
                }

                val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
                val fetchResponse = BatchFetchResponse.getRootAsBatchFetchResponse(buffer)

                // Extract the first media item
                if (fetchResponse.batchesLength > 0) {
                    val batch = fetchResponse.batches(0)
                    if (batch != null && batch.itemsLength > 0) {
                        val item = batch.items(0)
                        if (item != null) {
                            when (item.variantType) {
                                MediaVariant.MovieReference -> {
                                    val movie = item.variant(MovieReference()) as MovieReference
                                    _uiState.value = DetailUiState.MovieDetail(movie, buffer)
                                }
                                MediaVariant.SeriesReference -> {
                                    val series = item.variant(SeriesReference()) as SeriesReference
                                    _uiState.value = DetailUiState.SeriesDetail(series, buffer)
                                }
                                else -> {
                                    _uiState.value = DetailUiState.Error("Unsupported media type")
                                }
                            }
                            return@launch
                        }
                    }
                }

                _uiState.value = DetailUiState.Error("Media not found")
            } catch (e: Exception) {
                _uiState.value = DetailUiState.Error(
                    e.localizedMessage ?: "Failed to load details"
                )
            }
        }
    }

    fun backdropUrl(movie: MovieReference): String? {
        val iid = movie.details?.primaryBackdropIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/blob/${iid.toUuidString()}"
    }

    fun posterUrl(movie: MovieReference): String? {
        val iid = movie.details?.primaryPosterIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/blob/${iid.toUuidString()}"
    }

    fun seriesPosterUrl(series: SeriesReference): String? {
        val iid = series.details?.primaryPosterIid ?: return null
        return "${serverConfig.serverUrl}/api/v1/images/blob/${iid.toUuidString()}"
    }

    fun streamUrl(): String =
        "${serverConfig.serverUrl}/api/v1/stream/$mediaId"
}

sealed interface DetailUiState {
    data object Loading : DetailUiState
    data class MovieDetail(
        val movie: MovieReference,
        val buffer: ByteBuffer, // Keep buffer alive for zero-copy access
    ) : DetailUiState
    data class SeriesDetail(
        val series: SeriesReference,
        val buffer: ByteBuffer,
    ) : DetailUiState
    data class Error(val message: String) : DetailUiState
}


