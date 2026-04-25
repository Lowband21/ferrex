package com.ferrex.android.ui.home

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import com.ferrex.android.core.watch.ContinueWatchingActionHint
import com.ferrex.android.core.watch.ContinueWatchingItem
import com.ferrex.android.ui.detail.formatTime
import com.ferrex.android.ui.library.LibraryGridScreen
import com.ferrex.android.ui.library.LibraryViewModel
import kotlinx.coroutines.delay

/**
 * Home screen with continue watching carousel, library tabs, and poster grid.
 *
 * Structure:
 * - Top bar: app title + search icon
 * - Continue watching carousel (if items exist)
 * - Tab row: one tab per library
 * - Content: [LibraryGridScreen] for the selected library
 */
@Composable
fun HomeScreen(
    libraryViewModel: LibraryViewModel,
    homeViewModel: HomeViewModel,
    onMovieClick: (movieId: String) -> Unit,
    onSeriesClick: (seriesId: String) -> Unit,
    onSearchClick: () -> Unit,
    onContinueWatchingClick: (mediaId: String) -> Unit,
) {
    val libraries by libraryViewModel.libraries.collectAsState()
    val continueWatching by homeViewModel.continueWatching.collectAsState()
    var selectedTabIndex by remember { mutableIntStateOf(0) }
    val lifecycleOwner = LocalLifecycleOwner.current

    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                homeViewModel.refresh()
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }

    LaunchedEffect(Unit) {
        while (true) {
            delay(30_000)
            homeViewModel.refresh()
        }
    }

    // Auto-select first library on load
    LaunchedEffect(libraries) {
        if (libraries.isNotEmpty() && libraryViewModel.selectedLibraryId.value == null) {
            libraryViewModel.selectLibrary(libraries[0].id, libraries[0].libraryType)
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        // Compact title row
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surface)
                .windowInsetsPadding(WindowInsets.statusBars)
                .padding(start = 16.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Ferrex",
                style = MaterialTheme.typography.titleLarge,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = onSearchClick) {
                Icon(
                    Icons.Default.Search,
                    contentDescription = "Search",
                    tint = MaterialTheme.colorScheme.onSurface,
                )
            }
        }

        // Continue watching carousel
        if (continueWatching.isNotEmpty()) {
            ContinueWatchingSection(
                items = continueWatching,
                posterUrl = { homeViewModel.posterUrl(it) },
                onClick = { item -> onContinueWatchingClick(item.mediaId) },
            )
        }

        // Library tabs
        if (libraries.size > 1) {
            ScrollableTabRow(
                selectedTabIndex = selectedTabIndex.coerceIn(
                    0, (libraries.size - 1).coerceAtLeast(0)
                ),
                edgePadding = 16.dp,
            ) {
                libraries.forEachIndexed { index, library ->
                    Tab(
                        selected = index == selectedTabIndex,
                        onClick = {
                            selectedTabIndex = index
                            libraryViewModel.selectLibrary(library.id, library.libraryType)
                        },
                        text = { Text(library.name) },
                    )
                }
            }
        }

        // Library grid
        val selectedLibrary = libraries.getOrNull(selectedTabIndex)
        if (selectedLibrary != null) {
            LibraryGridScreen(
                viewModel = libraryViewModel,
                onMovieClick = onMovieClick,
                onSeriesClick = onSeriesClick,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

/**
 * Horizontal continue watching carousel.
 * Shows landscape poster cards with progress bars and play buttons.
 */
@Composable
private fun ContinueWatchingSection(
    items: List<ContinueWatchingItem>,
    posterUrl: (ContinueWatchingItem) -> String?,
    onClick: (ContinueWatchingItem) -> Unit,
) {
    Column(modifier = Modifier.padding(vertical = 8.dp)) {
        Text(
            text = "Continue Watching",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = 16.dp),
        )
        Spacer(Modifier.height(8.dp))
        LazyRow(
            contentPadding = PaddingValues(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            items(items.size) { index ->
                val item = items[index]
                ContinueWatchingCard(
                    item = item,
                    posterUrl = posterUrl(item),
                    onClick = { onClick(item) },
                )
            }
        }
    }
}

/**
 * Individual continue watching card — landscape format with progress overlay.
 */
@Composable
private fun ContinueWatchingCard(
    item: ContinueWatchingItem,
    posterUrl: String?,
    onClick: () -> Unit,
) {
    Card(
        modifier = Modifier
            .width(220.dp)
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(8.dp),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Column {
            // Poster image (landscape crop)
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .aspectRatio(16f / 9f),
            ) {
                if (posterUrl != null) {
                    AsyncImage(
                        model = posterUrl,
                        contentDescription = item.title,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize(),
                    )
                } else {
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .background(MaterialTheme.colorScheme.surfaceVariant),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = item.title.take(1).uppercase(),
                            style = MaterialTheme.typography.headlineLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }

                // Play button overlay
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(24.dp))
                            .background(Color.Black.copy(alpha = 0.6f))
                            .padding(8.dp),
                    ) {
                        Icon(
                            Icons.Default.PlayArrow,
                            contentDescription = continueWatchingActionLabel(item),
                            tint = Color.White,
                        )
                    }
                }

                // Time remaining overlay
                val remaining = item.duration - item.position
                if (remaining > 0) {
                    Box(
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .padding(4.dp)
                            .clip(RoundedCornerShape(4.dp))
                            .background(Color.Black.copy(alpha = 0.7f))
                            .padding(horizontal = 6.dp, vertical = 2.dp),
                    ) {
                        Text(
                            text = "${formatTime(remaining)} left",
                            style = MaterialTheme.typography.labelSmall,
                            color = Color.White,
                        )
                    }
                }

                // Progress bar at bottom
                if (item.progress > 0f) {
                    LinearProgressIndicator(
                        progress = { item.progress },
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(3.dp)
                            .align(Alignment.BottomCenter),
                        color = MaterialTheme.colorScheme.primary,
                        trackColor = Color.Black.copy(alpha = 0.5f),
                    )
                }
            }

            Column(
                modifier = Modifier.padding(horizontal = 8.dp, vertical = 6.dp),
            ) {
                Text(
                    text = item.title,
                    style = MaterialTheme.typography.bodySmall,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )

                item.subtitle?.let { subtitle ->
                    Spacer(Modifier.height(2.dp))
                    Text(
                        text = subtitle,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }

                Spacer(Modifier.height(4.dp))
                Text(
                    text = continueWatchingActionLabel(item),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

private fun continueWatchingActionLabel(item: ContinueWatchingItem): String = when (item.actionHint) {
    ContinueWatchingActionHint.NextEpisode -> "Next episode"
    ContinueWatchingActionHint.Resume -> "Resume"
    null -> if (item.progress > 0f) "Resume" else "Play"
}
