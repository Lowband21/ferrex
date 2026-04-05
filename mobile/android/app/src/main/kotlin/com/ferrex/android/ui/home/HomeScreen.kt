package com.ferrex.android.ui.home

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ferrex.android.ui.library.LibraryGridScreen
import com.ferrex.android.ui.library.LibraryViewModel

/**
 * Home screen with library tabs and poster grid.
 *
 * Structure:
 * - Top bar: app title + search icon
 * - Tab row: one tab per library (Movies, Series, etc.)
 * - Content: [LibraryGridScreen] for the selected library
 *
 * Library selection triggers batch sync → cache update → grid refresh.
 */
@Composable
fun HomeScreen(
    libraryViewModel: LibraryViewModel,
    onMovieClick: (movieId: String) -> Unit,
    onSearchClick: () -> Unit,
) {
    val libraries by libraryViewModel.libraries.collectAsState()
    var selectedTabIndex by remember { mutableIntStateOf(0) }

    // Auto-select first library on load
    LaunchedEffect(libraries) {
        if (libraries.isNotEmpty() && libraryViewModel.selectedLibraryId.value == null) {
            libraryViewModel.selectLibrary(libraries[0].id)
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        // Compact title row — just status-bar inset + a single
        // content-height row instead of the 64dp M3 TopAppBar.
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
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = onSearchClick) {
                Icon(Icons.Default.Search, contentDescription = "Search")
            }
        }

        if (libraries.size > 1) {
            ScrollableTabRow(
                selectedTabIndex = selectedTabIndex.coerceIn(0, (libraries.size - 1).coerceAtLeast(0)),
                edgePadding = 16.dp,
            ) {
                libraries.forEachIndexed { index, library ->
                    Tab(
                        selected = index == selectedTabIndex,
                        onClick = {
                            selectedTabIndex = index
                            libraryViewModel.selectLibrary(library.id)
                        },
                        text = { Text(library.name) },
                    )
                }
            }
        }

        val selectedLibrary = libraries.getOrNull(selectedTabIndex)
        if (selectedLibrary != null) {
            LibraryGridScreen(
                viewModel = libraryViewModel,
                onMovieClick = onMovieClick,
                modifier = Modifier.weight(1f),
            )
        }
    }
}
