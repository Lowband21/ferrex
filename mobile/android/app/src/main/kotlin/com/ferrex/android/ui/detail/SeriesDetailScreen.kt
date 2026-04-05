package com.ferrex.android.ui.detail

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import ferrex.media.SeriesReference

/**
 * Series detail screen — overview, season tabs, episode list.
 *
 * Data comes from the locally cached batch data (zero-copy FlatBuffers).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SeriesDetailScreen(
    viewModel: DetailViewModel,
    onBack: () -> Unit,
    onEpisodeClick: (mediaId: String) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsState()

    when (val state = uiState) {
        is DetailUiState.Loading -> LoadingScreen()
        is DetailUiState.Error -> ErrorScreen(
            message = state.message,
            onRetry = onBack,
        )
        is DetailUiState.MovieDetail -> ErrorScreen(message = "Expected series, got movie")
        is DetailUiState.SeriesDetail -> {
            SeriesDetailContent(
                series = state.series,
                backdropUrl = viewModel.seriesBackdropUrl(state.series),
                posterUrl = viewModel.seriesPosterUrl(state.series),
                onBack = onBack,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SeriesDetailContent(
    series: SeriesReference,
    backdropUrl: String?,
    posterUrl: String?,
    onBack: () -> Unit,
) {
    val details = series.details
    val scrollState = rememberScrollState()

    Scaffold(
        topBar = {
            TopAppBar(
                title = {},
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = Color.Transparent,
                ),
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(scrollState),
        ) {
            // Backdrop area (prefer backdrop, fall back to poster)
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .aspectRatio(16f / 9f),
            ) {
                val heroUrl = backdropUrl ?: posterUrl
                if (heroUrl != null) {
                    AsyncImage(
                        model = heroUrl,
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(
                            Brush.verticalGradient(
                                colors = listOf(
                                    Color.Transparent,
                                    MaterialTheme.colorScheme.background,
                                ),
                                startY = 200f,
                            ),
                        ),
                )
            }

            Column(modifier = Modifier.padding(horizontal = 16.dp)) {
                Text(
                    text = series.title,
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.Bold,
                )

                Spacer(Modifier.height(4.dp))

                // Metadata
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    details?.firstAirDate?.take(4)?.let { year ->
                        Text(
                            text = year,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    details?.let { d ->
                        if (d.numberOfSeasons > 0u) {
                            Text(
                                text = "${d.numberOfSeasons} season${if (d.numberOfSeasons > 1u) "s" else ""}",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    details?.let { d ->
                        if (d.voteAverage > 0f) {
                            Text(
                                text = "★ %.1f".format(d.voteAverage),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }

                Spacer(Modifier.height(16.dp))

                // Overview
                details?.overview?.let { overview ->
                    if (overview.isNotBlank()) {
                        Text(
                            text = overview,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Spacer(Modifier.height(16.dp))
                    }
                }

                // Genres
                val genreCount = details?.genresLength ?: 0
                if (genreCount > 0) {
                    Text(
                        text = (0 until genreCount)
                            .mapNotNull { details?.genres(it)?.name }
                            .joinToString(", "),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(16.dp))
                }

                // Season tabs placeholder
                val seasonCount = details?.numberOfSeasons?.toInt() ?: 0
                if (seasonCount > 0) {
                    var selectedSeason by remember { mutableIntStateOf(1) }
                    Text(
                        text = "Seasons",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                    )
                    Spacer(Modifier.height(8.dp))
                    ScrollableTabRow(
                        selectedTabIndex = (selectedSeason - 1).coerceIn(0, seasonCount - 1),
                        edgePadding = 0.dp,
                    ) {
                        (1..seasonCount).forEach { season ->
                            Tab(
                                selected = season == selectedSeason,
                                onClick = { selectedSeason = season },
                                text = { Text("Season $season") },
                            )
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                    Text(
                        text = "Episode list — coming in Phase 6",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }

                Spacer(Modifier.height(32.dp))
            }
        }
    }
}
