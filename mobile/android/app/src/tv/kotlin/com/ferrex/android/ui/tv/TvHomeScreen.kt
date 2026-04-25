package com.ferrex.android.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.library.SyncState
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.ui.detail.formatTime
import com.ferrex.android.ui.home.HomeViewModel
import com.ferrex.android.ui.library.LibraryViewModel
import ferrex.common.LibraryType

/** Android TV home shell backed by shared ViewModels and TV-specific rows. */
@Composable
fun TvHomeScreen(
    libraryViewModel: LibraryViewModel,
    homeViewModel: HomeViewModel,
    onSearchClick: () -> Unit,
    onMovieClick: (movieId: String) -> Unit,
    onSeriesClick: (seriesId: String) -> Unit,
    onContinueWatchingClick: (mediaId: String) -> Unit,
) {
    val libraries by libraryViewModel.libraries.collectAsState()
    val selectedLibraryId by libraryViewModel.selectedLibraryId.collectAsState()
    val selectedLibraryType by libraryViewModel.selectedLibraryType.collectAsState()
    val syncState by libraryViewModel.syncState.collectAsState()
    val media by libraryViewModel.currentMedia.collectAsState()
    val continueWatching by homeViewModel.continueWatching.collectAsState()

    val selectedLibrary = libraries.firstOrNull { it.id == selectedLibraryId } ?: libraries.firstOrNull()

    LaunchedEffect(libraries) {
        if (libraries.isNotEmpty() && selectedLibraryId == null) {
            val first = libraries.first()
            libraryViewModel.selectLibrary(first.id, first.libraryType)
        }
    }

    val continueItems = remember(continueWatching) {
        continueWatching.map { item ->
            val remaining = (item.duration - item.position).takeIf { it > 0 }
            TvPosterItem(
                id = "continue-${item.mediaId}",
                title = item.title,
                subtitle = remaining?.let { "${formatTime(it)} left" } ?: "Resume",
                posterUrl = homeViewModel.posterUrl(item),
                progress = item.progress,
            )
        }
    }

    val libraryItems = remember(media, selectedLibraryType) {
        val accessor = media
        if (accessor == null) {
            emptyList()
        } else if (selectedLibraryType == LibraryType.Series) {
            (0 until accessor.seriesCount.coerceAtMost(30)).mapNotNull { index ->
                val series = accessor.seriesAt(index) ?: return@mapNotNull null
                TvPosterItem(
                    id = series.id.toUuidString(),
                    title = series.title,
                    subtitle = "Series",
                    posterUrl = libraryViewModel.posterUrlForSeries(series),
                )
            }
        } else {
            (0 until accessor.movieCount.coerceAtMost(30)).mapNotNull { index ->
                val movie = accessor.movieAt(index) ?: return@mapNotNull null
                TvPosterItem(
                    id = movie.id.toUuidString(),
                    title = movie.title,
                    subtitle = movie.details?.releaseDate?.take(4),
                    posterUrl = libraryViewModel.posterUrlForMovie(movie),
                )
            }
        }
    }

    var focusedItem by remember(continueItems, libraryItems) {
        mutableStateOf(continueItems.firstOrNull() ?: libraryItems.firstOrNull())
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
                        colors = listOf(
                            Color(0xFF172554),
                            Color(0xFF070A12),
                            Color(0xFF070A12),
                        ),
                    ),
                ),
        )

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(28.dp),
        ) {
            item {
                TvHeroHeader(
                    focusedItem = focusedItem,
                    libraryName = selectedLibrary?.name,
                    onSearchClick = onSearchClick,
                )
            }

            item {
                TvPosterRow(
                    title = "Continue Watching",
                    items = continueItems,
                    style = TvPosterCardStyle.Landscape,
                    onItemClick = { item ->
                        onContinueWatchingClick(item.id.removePrefix("continue-"))
                    },
                    onItemFocused = { focusedItem = it },
                    autoFocusFirst = continueItems.isNotEmpty(),
                )
            }

            item {
                when (syncState) {
                    is SyncState.Idle, is SyncState.Syncing -> {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 56.dp, vertical = 24.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(16.dp),
                        ) {
                            CircularProgressIndicator()
                            Text(
                                text = "Loading ${selectedLibrary?.name ?: "library"}…",
                                style = MaterialTheme.typography.titleLarge,
                                color = Color.White,
                            )
                        }
                    }
                    is SyncState.Error -> {
                        Text(
                            text = "Library error: ${(syncState as SyncState.Error).message}",
                            style = MaterialTheme.typography.titleLarge,
                            color = MaterialTheme.colorScheme.error,
                            modifier = Modifier.padding(horizontal = 56.dp),
                        )
                    }
                    is SyncState.Ready -> {
                        TvPosterRow(
                            title = selectedLibrary?.name ?: "Library",
                            items = libraryItems,
                            style = TvPosterCardStyle.Poster,
                            onItemClick = { item ->
                                if (selectedLibraryType == LibraryType.Series) {
                                    onSeriesClick(item.id)
                                } else {
                                    onMovieClick(item.id)
                                }
                            },
                            onItemFocused = { focusedItem = it },
                            autoFocusFirst = continueItems.isEmpty() && libraryItems.isNotEmpty(),
                        )
                    }
                }
            }

            item { Spacer(Modifier.height(48.dp)) }
        }
    }
}

@Composable
private fun TvHeroHeader(
    focusedItem: TvPosterItem?,
    libraryName: String?,
    onSearchClick: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 56.dp, vertical = 36.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Ferrex TV",
                style = MaterialTheme.typography.displaySmall,
                color = Color.White,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.weight(1f),
            )
            Button(
                onClick = onSearchClick,
                modifier = Modifier.semantics { contentDescription = "Search" },
                colors = ButtonDefaults.buttonColors(
                    containerColor = Color.White.copy(alpha = 0.12f),
                    contentColor = Color.White,
                ),
            ) {
                Icon(Icons.Default.Search, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text("Search")
            }
        }

        Spacer(Modifier.height(54.dp))

        Surface(
            color = Color.Black.copy(alpha = 0.22f),
            tonalElevation = 0.dp,
            shape = MaterialTheme.shapes.extraLarge,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(modifier = Modifier.padding(32.dp)) {
                Text(
                    text = focusedItem?.title ?: "Ready for the couch",
                    style = MaterialTheme.typography.displayMedium,
                    color = Color.White,
                    fontWeight = FontWeight.Bold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(12.dp))
                Text(
                    text = focusedItem?.subtitle
                        ?: "Use the D-pad to browse. ${libraryName ?: "Your libraries"} will appear below.",
                    style = MaterialTheme.typography.titleLarge,
                    color = Color.White.copy(alpha = 0.78f),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}
