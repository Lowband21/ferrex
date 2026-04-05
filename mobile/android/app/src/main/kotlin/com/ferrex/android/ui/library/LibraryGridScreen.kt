package com.ferrex.android.ui.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.library.SyncState
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen

/**
 * Library poster grid screen — the primary browsing interface.
 *
 * Uses [LazyVerticalGrid] with adaptive columns (min 120dp) for responsive
 * layout across phones, tablets, and foldables. The grid reads movie data
 * from [MediaAccessor] which provides zero-copy FlatBuffer field access
 * from memory-mapped disk cache.
 *
 * Performance target: 60fps scroll on Pixel 6 with 1000+ movies.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryGridScreen(
    viewModel: LibraryViewModel,
    libraryName: String,
    onMovieClick: (movieId: String) -> Unit,
) {
    val syncState by viewModel.syncState.collectAsState()
    val media by viewModel.currentMedia.collectAsState()
    val scrollBehavior = TopAppBarDefaults.pinnedScrollBehavior()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(libraryName) },
                scrollBehavior = scrollBehavior,
            )
        },
    ) { padding ->
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
                if (accessor == null || accessor.movieCount == 0) {
                    ErrorScreen(message = "No movies found")
                } else {
                    LazyVerticalGrid(
                        columns = GridCells.Adaptive(minSize = 120.dp),
                        modifier = Modifier
                            .padding(padding)
                            .nestedScroll(scrollBehavior.nestedScrollConnection),
                        contentPadding = PaddingValues(8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        items(
                            count = accessor.movieCount,
                            key = { index ->
                                // Stable key from UUID — survives recomposition
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
