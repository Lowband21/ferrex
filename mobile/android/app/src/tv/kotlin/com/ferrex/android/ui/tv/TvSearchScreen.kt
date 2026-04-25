package com.ferrex.android.ui.tv

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.search.SearchHit
import com.ferrex.android.ui.search.SearchUiState
import com.ferrex.android.ui.search.SearchViewModel

@Composable
fun TvSearchScreen(
    viewModel: SearchViewModel,
    onBack: () -> Unit,
    onMovieClick: (movieId: String) -> Unit,
    onSeriesClick: (seriesId: String) -> Unit,
) {
    BackHandler(onBack = onBack)
    val query by viewModel.query.collectAsState()
    val uiState by viewModel.uiState.collectAsState()
    val searchFocusRequester = remember { FocusRequester() }

    LaunchedEffect(Unit) {
        runCatching { searchFocusRequester.requestFocus() }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xFF070A12))
            .windowInsetsPadding(WindowInsets.safeDrawing),
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        listOf(Color(0xFF172554), Color(0xFF070A12), Color(0xFF070A12)),
                    ),
                ),
        )

        Column(modifier = Modifier.fillMaxSize()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 56.dp, vertical = 32.dp),
                horizontalArrangement = Arrangement.spacedBy(18.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Button(
                    onClick = onBack,
                    colors = ButtonDefaults.buttonColors(
                        containerColor = Color.Black.copy(alpha = 0.52f),
                        contentColor = Color.White,
                    ),
                    modifier = Modifier.semantics { contentDescription = "Back" },
                ) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("Back")
                }
                OutlinedTextField(
                    value = query,
                    onValueChange = viewModel::updateQuery,
                    label = { Text("Search movies and shows") },
                    singleLine = true,
                    leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
                    trailingIcon = {
                        if (query.isNotEmpty()) {
                            IconButton(onClick = { viewModel.updateQuery("") }) {
                                Icon(Icons.Default.Clear, contentDescription = "Clear search")
                            }
                        }
                    },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedTextColor = Color.White,
                        unfocusedTextColor = Color.White,
                        focusedLabelColor = Color.White,
                        unfocusedLabelColor = Color.White.copy(alpha = 0.72f),
                        focusedBorderColor = MaterialTheme.colorScheme.primary,
                        unfocusedBorderColor = Color.White.copy(alpha = 0.38f),
                        focusedContainerColor = Color.White.copy(alpha = 0.08f),
                        unfocusedContainerColor = Color.White.copy(alpha = 0.06f),
                        cursorColor = Color.White,
                    ),
                    modifier = Modifier
                        .weight(1f)
                        .focusRequester(searchFocusRequester)
                        .semantics { contentDescription = "Search movies and shows" },
                )
            }

            when (val state = uiState) {
                is SearchUiState.Idle -> TvSearchMessage("Type at least two characters to search")
                is SearchUiState.Loading -> TvSearchLoading()
                is SearchUiState.Error -> TvSearchMessage(state.message, isError = true)
                is SearchUiState.Results -> {
                    if (state.hits.isEmpty()) {
                        TvSearchMessage("No results found")
                    } else {
                        TvSearchResults(
                            hits = state.hits,
                            posterUrlForMovie = viewModel::posterUrlForMovie,
                            posterUrlForSeries = viewModel::posterUrlForSeries,
                            onMovieClick = onMovieClick,
                            onSeriesClick = onSeriesClick,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun TvSearchResults(
    hits: List<SearchHit>,
    posterUrlForMovie: (ferrex.media.MovieReference) -> String?,
    posterUrlForSeries: (ferrex.media.SeriesReference) -> String?,
    onMovieClick: (movieId: String) -> Unit,
    onSeriesClick: (seriesId: String) -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 190.dp),
        contentPadding = PaddingValues(horizontal = 56.dp, vertical = 20.dp),
        horizontalArrangement = Arrangement.spacedBy(28.dp),
        verticalArrangement = Arrangement.spacedBy(32.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        items(
            items = hits,
            key = { hit ->
                when (hit) {
                    is SearchHit.Movie -> "movie-${hit.mediaId}"
                    is SearchHit.Series -> "series-${hit.mediaId}"
                }
            },
        ) { hit ->
            when (hit) {
                is SearchHit.Movie -> {
                    TvPosterCard(
                        item = TvPosterItem(
                            id = hit.mediaId,
                            title = hit.movie.title,
                            subtitle = "Movie",
                            posterUrl = posterUrlForMovie(hit.movie),
                        ),
                        style = TvPosterCardStyle.Poster,
                        onClick = { onMovieClick(hit.mediaId) },
                        onFocused = {},
                    )
                }
                is SearchHit.Series -> {
                    TvPosterCard(
                        item = TvPosterItem(
                            id = hit.mediaId,
                            title = hit.series.title,
                            subtitle = "Series",
                            posterUrl = posterUrlForSeries(hit.series),
                        ),
                        style = TvPosterCardStyle.Poster,
                        onClick = { onSeriesClick(hit.mediaId) },
                        onFocused = {},
                    )
                }
            }
        }
    }
}

@Composable
private fun TvSearchMessage(
    message: String,
    isError: Boolean = false,
) {
    Box(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = message,
            style = MaterialTheme.typography.headlineSmall,
            color = if (isError) MaterialTheme.colorScheme.error else Color.White.copy(alpha = 0.72f),
            fontWeight = FontWeight.Medium,
        )
    }
}

@Composable
private fun TvSearchLoading() {
    Box(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator()
            Text(
                text = "Searching…",
                style = MaterialTheme.typography.headlineSmall,
                color = Color.White,
            )
        }
    }
}
