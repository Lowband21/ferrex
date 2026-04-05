package com.ferrex.android.ui.home

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
@OptIn(ExperimentalMaterial3Api::class)
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
        TopAppBar(
            title = { Text("Ferrex") },
            actions = {
                IconButton(onClick = onSearchClick) {
                    Icon(Icons.Default.Search, contentDescription = "Search")
                }
            },
        )

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
