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
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import com.ferrex.android.core.library.LibraryInfo
import com.ferrex.android.core.watch.ContinueWatchingActionHint
import com.ferrex.android.core.watch.ContinueWatchingItem
import com.ferrex.android.ui.detail.formatTime
import com.ferrex.android.ui.library.LibraryGridScreen
import com.ferrex.android.ui.library.LibraryViewModel
import kotlinx.coroutines.delay

/**
 * Resume-first mobile landing screen.
 *
 * The first authenticated destination focuses on Continue Watching, then gives
 * clear routes into the user's libraries and Search without depending on
 * discovery/explore backend endpoints that are not available yet.
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
    val isLoadingResume by homeViewModel.isLoading.collectAsState()
    val hasLoadedResume by homeViewModel.hasLoaded.collectAsState()
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

    LaunchedEffect(libraries) {
        if (libraries.isNotEmpty()) {
            val selectedLibraryId = libraryViewModel.selectedLibraryId.value
            val selectedIndex = libraries.indexOfFirst { it.id == selectedLibraryId }
            if (selectedIndex >= 0) {
                selectedTabIndex = selectedIndex
            } else {
                selectedTabIndex = 0
                libraryViewModel.selectLibrary(libraries[0].id, libraries[0].libraryType)
            }
        } else {
            selectedTabIndex = 0
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        ResumeTopBar(onSearchClick = onSearchClick)

        ContinueWatchingSection(
            items = continueWatching,
            isLoading = isLoadingResume && !hasLoadedResume,
            posterUrl = { homeViewModel.posterUrl(it) },
            onSearchClick = onSearchClick,
            onClick = { item -> onContinueWatchingClick(item.mediaId) },
        )

        LibraryNavigationSection(
            libraries = libraries,
            selectedTabIndex = selectedTabIndex,
            onSearchClick = onSearchClick,
            onLibrarySelected = { index, library ->
                selectedTabIndex = index
                libraryViewModel.selectLibrary(library.id, library.libraryType)
            },
        )

        if (libraries.isNotEmpty()) {
            LibraryGridScreen(
                viewModel = libraryViewModel,
                onMovieClick = onMovieClick,
                onSeriesClick = onSeriesClick,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun ResumeTopBar(onSearchClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
            .windowInsetsPadding(WindowInsets.statusBars)
            .padding(start = 16.dp, end = 6.dp, top = 10.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = "Resume",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                text = "Continue playback or jump back into your libraries.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        IconButton(onClick = onSearchClick) {
            Icon(
                Icons.Default.Search,
                contentDescription = "Search movies and shows",
                tint = MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

/**
 * Compact horizontal resume shelf. Empty state is intentionally useful: it
 * explains what will appear here and points to Libraries/Search instead of
 * leaving a blank landing surface.
 */
@Composable
private fun ContinueWatchingSection(
    items: List<ContinueWatchingItem>,
    isLoading: Boolean,
    posterUrl: (ContinueWatchingItem) -> String?,
    onSearchClick: () -> Unit,
    onClick: (ContinueWatchingItem) -> Unit,
) {
    Column(
        modifier = Modifier
            .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.24f))
            .padding(vertical = 12.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "Continue watching",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    text = "Resume movies and next episodes from your server.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Spacer(Modifier.height(10.dp))

        when {
            items.isNotEmpty() -> {
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
            isLoading -> ContinueWatchingLoadingRow()
            else -> EmptyContinueWatchingCard(onSearchClick = onSearchClick)
        }
    }
}

@Composable
private fun ContinueWatchingLoadingRow() {
    LazyRow(
        contentPadding = PaddingValues(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        items(2) {
            Card(
                modifier = Modifier.width(220.dp),
                shape = RoundedCornerShape(12.dp),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            ) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .aspectRatio(16f / 9f)
                        .background(MaterialTheme.colorScheme.surfaceVariant),
                )
                Column(modifier = Modifier.padding(10.dp)) {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth(0.72f)
                            .height(14.dp)
                            .clip(RoundedCornerShape(7.dp))
                            .background(MaterialTheme.colorScheme.surfaceVariant),
                    )
                    Spacer(Modifier.height(8.dp))
                    Box(
                        modifier = Modifier
                            .fillMaxWidth(0.46f)
                            .height(12.dp)
                            .clip(RoundedCornerShape(6.dp))
                            .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.7f)),
                    )
                }
            }
        }
    }
}

@Composable
private fun EmptyContinueWatchingCard(onSearchClick: () -> Unit) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(
                text = "Nothing to resume yet",
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                text = "Start a movie or episode from Libraries, and it will appear here when there is meaningful progress to continue.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OutlinedButton(onClick = onSearchClick) {
                    Icon(Icons.Default.Search, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("Search")
                }
                Text(
                    text = "Libraries are below",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun LibraryNavigationSection(
    libraries: List<LibraryInfo>,
    selectedTabIndex: Int,
    onSearchClick: () -> Unit,
    onLibrarySelected: (Int, LibraryInfo) -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
            .padding(top = 14.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "Libraries",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    text = if (libraries.isEmpty()) {
                        "Media libraries will appear after sync."
                    } else {
                        "Browse movies and shows from this server."
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            OutlinedButton(onClick = onSearchClick) {
                Text("Search")
            }
        }

        Spacer(Modifier.height(10.dp))

        when {
            libraries.size > 1 -> {
                ScrollableTabRow(
                    selectedTabIndex = selectedTabIndex.coerceIn(
                        0, (libraries.size - 1).coerceAtLeast(0),
                    ),
                    edgePadding = 16.dp,
                    divider = {},
                ) {
                    libraries.forEachIndexed { index, library ->
                        Tab(
                            selected = index == selectedTabIndex,
                            onClick = { onLibrarySelected(index, library) },
                            text = {
                                Text(
                                    text = library.name,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            },
                        )
                    }
                }
            }
            libraries.size == 1 -> {
                Text(
                    text = libraries[0].name,
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier
                        .padding(horizontal = 16.dp)
                        .clip(RoundedCornerShape(50.dp))
                        .background(MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.6f))
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                )
                Spacer(Modifier.height(10.dp))
            }
            else -> Spacer(Modifier.height(8.dp))
        }
    }
}

/**
 * Individual continue watching card — compact landscape format with progress
 * and action-hint-aware copy.
 */
@Composable
private fun ContinueWatchingCard(
    item: ContinueWatchingItem,
    posterUrl: String?,
    onClick: () -> Unit,
) {
    Card(
        modifier = Modifier
            .width(214.dp)
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Column {
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

                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(24.dp))
                            .background(Color.Black.copy(alpha = 0.62f))
                            .padding(9.dp),
                    ) {
                        Icon(
                            Icons.Default.PlayArrow,
                            contentDescription = continueWatchingActionLabel(item),
                            tint = Color.White,
                        )
                    }
                }

                val remaining = item.duration - item.position
                if (remaining > 0 && item.actionHint != ContinueWatchingActionHint.NextEpisode) {
                    Box(
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .padding(6.dp)
                            .clip(RoundedCornerShape(6.dp))
                            .background(Color.Black.copy(alpha = 0.72f))
                            .padding(horizontal = 7.dp, vertical = 3.dp),
                    ) {
                        Text(
                            text = "${formatTime(remaining)} left",
                            style = MaterialTheme.typography.labelSmall,
                            color = Color.White,
                        )
                    }
                }

                if (item.progress > 0f) {
                    LinearProgressIndicator(
                        progress = { item.progress.coerceIn(0f, 1f) },
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(4.dp)
                            .align(Alignment.BottomCenter),
                        color = MaterialTheme.colorScheme.primary,
                        trackColor = Color.Black.copy(alpha = 0.5f),
                    )
                }
            }

            Column(
                modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp),
            ) {
                Text(
                    text = item.title,
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.SemiBold,
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

                Spacer(Modifier.height(5.dp))
                Text(
                    text = continueWatchingActionLabel(item),
                    style = MaterialTheme.typography.labelSmall,
                    fontWeight = FontWeight.SemiBold,
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
    ContinueWatchingActionHint.Resume -> if (item.position > 0.0) {
        "Resume at ${formatTime(item.position)}"
    } else {
        "Resume"
    }
    null -> if (item.progress > 0f) "Resume" else "Play"
}
