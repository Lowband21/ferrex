package com.ferrex.android.ui.search

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SearchBar
import androidx.compose.material3.SearchBarDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ferrex.android.ui.components.LoadingScreen
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.ui.library.PosterCard
import ferrex.library.BatchFetchResponse
import ferrex.media.Media
import ferrex.media.MediaVariant
import ferrex.media.MovieReference

/**
 * Search screen with debounced query input and results grid.
 *
 * Uses Material3 SearchBar with results displayed as poster cards
 * (reusing the same PosterCard composable from the library grid).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SearchScreen(
    viewModel: SearchViewModel,
    onBack: () -> Unit,
    onMovieClick: (movieId: String) -> Unit,
) {
    val query by viewModel.query.collectAsState()
    val uiState by viewModel.uiState.collectAsState()

    Scaffold { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding),
        ) {
            SearchBar(
                inputField = {
                    SearchBarDefaults.InputField(
                        query = query,
                        onQueryChange = viewModel::updateQuery,
                        onSearch = {},
                        expanded = false,
                        onExpandedChange = {},
                        placeholder = { Text("Search movies and shows…") },
                        leadingIcon = {
                            IconButton(onClick = onBack) {
                                Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back")
                            }
                        },
                        trailingIcon = {
                            if (query.isNotEmpty()) {
                                IconButton(onClick = { viewModel.updateQuery("") }) {
                                    Icon(Icons.Default.Clear, "Clear")
                                }
                            }
                        },
                    )
                },
                expanded = false,
                onExpandedChange = {},
                modifier = Modifier.fillMaxWidth(),
            ) {}

            when (val state = uiState) {
                is SearchUiState.Idle -> {
                    // Empty state
                    Column(
                        modifier = Modifier.fillMaxSize(),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                    ) {
                        Text(
                            text = "Search for movies and shows",
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                is SearchUiState.Loading -> {
                    LoadingScreen(message = "Searching…")
                }
                is SearchUiState.Error -> {
                    Column(
                        modifier = Modifier.fillMaxSize(),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                    ) {
                        Text(
                            text = state.message,
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                }
                is SearchUiState.Results -> {
                    // Parse results from the buffer
                    // The search response format matches BatchFetchResponse
                    val response = try {
                        BatchFetchResponse.getRootAsBatchFetchResponse(state.buffer)
                    } catch (_: Exception) {
                        null
                    }

                    if (response == null || response.batchesLength == 0) {
                        Column(
                            modifier = Modifier.fillMaxSize(),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.Center,
                        ) {
                            Text("No results found")
                        }
                    } else {
                        // Collect all movie results
                        val movies = buildList {
                            for (b in 0 until response.batchesLength) {
                                val batch = response.batches(b) ?: continue
                                for (i in 0 until batch.itemsLength) {
                                    val item = batch.items(i) ?: continue
                                    if (item.variantType == MediaVariant.MovieReference) {
                                        val movie = item.variant(MovieReference()) as? MovieReference
                                        if (movie != null) add(movie)
                                    }
                                }
                            }
                        }

                        LazyVerticalGrid(
                            columns = GridCells.Adaptive(minSize = 120.dp),
                            contentPadding = PaddingValues(8.dp),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            items(movies.size) { index ->
                                val movie = movies[index]
                                PosterCard(
                                    title = movie.title,
                                    posterUrl = viewModel.posterUrlForMovie(movie),
                                    onClick = {
                                        movie.id?.let { uuid ->
                                            onMovieClick(uuid.toUuidString())
                                        }
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
