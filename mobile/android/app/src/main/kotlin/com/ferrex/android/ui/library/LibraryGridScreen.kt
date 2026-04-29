package com.ferrex.android.ui.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.library.SyncState
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.PosterGridSkeleton
import ferrex.common.LibraryType

/**
 * Library poster grid — the primary browsing interface.
 *
 * Uses [LazyVerticalGrid] with adaptive columns for responsive layout across
 * phones, tablets, and foldables. This composable is a pure content panel; the
 * resume header and library/search navigation live on the parent screen.
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
                PosterGridSkeleton(modifier = Modifier.fillMaxSize())
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
                    if (accessor == null || accessor.seriesCount == 0) {
                        LibraryEmptyState(
                            title = "No shows in this library",
                            message = "When this library has series, posters will appear here for browsing.",
                        )
                    } else {
                        LazyVerticalGrid(
                            columns = GridCells.Adaptive(minSize = 132.dp),
                            contentPadding = PaddingValues(
                                start = 12.dp,
                                top = 12.dp,
                                end = 12.dp,
                                bottom = 24.dp,
                            ),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                            verticalArrangement = Arrangement.spacedBy(14.dp),
                            modifier = Modifier.fillMaxSize(),
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
                    if (accessor == null || accessor.movieCount == 0) {
                        LibraryEmptyState(
                            title = "No movies in this library",
                            message = "When this library has movies, posters will appear here for browsing.",
                        )
                    } else {
                        LazyVerticalGrid(
                            columns = GridCells.Adaptive(minSize = 132.dp),
                            contentPadding = PaddingValues(
                                start = 12.dp,
                                top = 12.dp,
                                end = 12.dp,
                                bottom = 24.dp,
                            ),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                            verticalArrangement = Arrangement.spacedBy(14.dp),
                            modifier = Modifier.fillMaxSize(),
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

@Composable
private fun LibraryEmptyState(
    title: String,
    message: String,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.onSurface,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            text = message,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}
