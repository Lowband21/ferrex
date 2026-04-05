package com.ferrex.android.ui.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.library.SyncState
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import ferrex.common.LibraryType

/**
 * Library poster grid — the primary browsing interface.
 *
 * Uses [LazyVerticalGrid] with adaptive columns (min 120dp) for responsive
 * layout across phones, tablets, and foldables. The grid reads movie data
 * from [MediaAccessor] which provides zero-copy FlatBuffer field access
 * from memory-mapped disk cache.
 *
 * This composable is a pure content panel — the top app bar and library
 * tabs live in [HomeScreen] so there is no duplicated chrome.
 *
 * Performance target: 60fps scroll on Pixel 6 with 1000+ movies.
 */
@Composable
fun LibraryGridScreen(
    viewModel: LibraryViewModel,
    onMovieClick: (movieId: String) -> Unit,
    onSeriesClick: (seriesId: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val syncState by viewModel.syncState.collectAsState()
    val media by viewModel.currentMedia.collectAsState()
    val libraryType by viewModel.selectedLibraryType.collectAsState()

    Box(modifier = modifier.fillMaxSize()) {
        when (syncState) {
            is SyncState.Idle, is SyncState.Syncing -> {
                LoadingScreen(message = "Loading library…")
            }
            is SyncState.Error -> {
                ErrorScreen(
                    message = (syncState as SyncState.Error).message,
                    onRetry = { viewModel.loadLibraries() },
                )
            }
            is SyncState.Ready -> {
                val accessor = media
                if (libraryType == LibraryType.Series) {
                    // Series library grid
                    if (accessor == null || accessor.seriesCount == 0) {
                        ErrorScreen(message = "No series found")
                    } else {
                        LazyVerticalGrid(
                            columns = GridCells.Adaptive(minSize = 120.dp),
                            contentPadding = PaddingValues(8.dp),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            items(
                                count = accessor.seriesCount,
                                key = { index ->
                                    accessor.seriesAt(index)?.id?.toUuidString() ?: index
                                },
                            ) { index ->
                                val series = accessor.seriesAt(index)
                                if (series != null) {
                                    PosterCard(
                                        title = series.title,
                                        posterUrl = viewModel.posterUrlForSeries(series),
                                        onClick = {
                                            onSeriesClick(series.id.toUuidString())
                                        },
                                    )
                                }
                            }
                        }
                    }
                } else {
                    // Movie library grid
                    if (accessor == null || accessor.movieCount == 0) {
                        ErrorScreen(message = "No movies found")
                    } else {
                        LazyVerticalGrid(
                            columns = GridCells.Adaptive(minSize = 120.dp),
                            contentPadding = PaddingValues(8.dp),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            items(
                                count = accessor.movieCount,
                                key = { index ->
                                    accessor.movieAt(index)?.id?.toUuidString() ?: index
                                },
                            ) { index ->
                                val movie = accessor.movieAt(index)
                                if (movie != null) {
                                    PosterCard(
                                        title = movie.title,
                                        posterUrl = viewModel.posterUrlForMovie(movie),
                                        onClick = {
                                            onMovieClick(movie.id.toUuidString())
                                        },
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
