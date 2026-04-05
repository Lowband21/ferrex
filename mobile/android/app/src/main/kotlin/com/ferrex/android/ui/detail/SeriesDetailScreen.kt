package com.ferrex.android.ui.detail

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
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
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import ferrex.media.EpisodeReference
import ferrex.media.SeriesReference
import ferrex.watch.EpisodeWatchState

/**
 * Series detail screen — overview, season tabs, episode list with progress.
 *
 * Data comes from the locally cached batch data (zero-copy FlatBuffers).
 * Watch state is fetched asynchronously from the server.
 *
 * Season tabs are derived from [EnhancedSeriesDetails.numberOfSeasons] which
 * is always available in the series library batch. Episode data comes from
 * [SeasonReference]/[EpisodeReference] items in the batch when available,
 * or will be fetched from the per-series bundle endpoint once the server
 * adds FlatBuffers support for it.
 */
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
                viewModel = viewModel,
                series = state.series,
                backdropUrl = viewModel.seriesBackdropUrl(state.series),
                posterUrl = viewModel.seriesPosterUrl(state.series),
                onBack = onBack,
                onEpisodeClick = onEpisodeClick,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SeriesDetailContent(
    viewModel: DetailViewModel,
    series: SeriesReference,
    backdropUrl: String?,
    posterUrl: String?,
    onBack: () -> Unit,
    onEpisodeClick: (mediaId: String) -> Unit,
) {
    val details = series.details
    val scrollState = rememberScrollState()

    // Episode data from the batch cache (populated when the server's per-series
    // bundle endpoint returns season/episode items in FlatBuffers format).
    val episodes by viewModel.episodes.collectAsState()
    val selectedSeason by viewModel.selectedSeason.collectAsState()
    val episodeStates by viewModel.episodeStates.collectAsState()

    // Season count from metadata — always available, even without per-series bundle
    val metadataSeasonCount = details?.numberOfSeasons?.toInt() ?: 0
    // Actual season objects from the batch (may be empty until server support lands)
    val seasons by viewModel.seasons.collectAsState()
    // Use whichever source has data: batch seasons or metadata count
    val effectiveSeasonCount = if (seasons.isNotEmpty()) seasons.size else metadataSeasonCount

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
            // Backdrop area
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
                // Title
                Text(
                    text = series.title,
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.Bold,
                )

                Spacer(Modifier.height(4.dp))

                // Metadata row
                Row(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    details?.firstAirDate?.take(4)?.let { year ->
                        if (year.isNotBlank()) MetadataChip(year)
                    }
                    if (metadataSeasonCount > 0) {
                        MetadataChip(
                            "$metadataSeasonCount season${if (metadataSeasonCount != 1) "s" else ""}"
                        )
                    }
                    details?.let { d ->
                        if (d.voteAverage > 0f) {
                            MetadataChip("★ %.1f".format(d.voteAverage))
                        }
                    }
                    details?.status?.let { status ->
                        if (status.isNotBlank()) MetadataChip(status)
                    }
                }

                Spacer(Modifier.height(16.dp))

                // Tagline
                details?.tagline?.let { tagline ->
                    if (tagline.isNotBlank()) {
                        Text(
                            text = tagline,
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.Medium,
                            color = MaterialTheme.colorScheme.primary,
                        )
                        Spacer(Modifier.height(8.dp))
                    }
                }

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
                            .joinToString(" · "),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(16.dp))
                }

                // Cast
                val castCount = details?.castLength ?: 0
                if (castCount > 0) {
                    SectionHeader("Cast")
                    Spacer(Modifier.height(8.dp))
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        val count = castCount.coerceAtMost(20)
                        items(count) { i ->
                            val member = details?.cast(i)
                            if (member != null) {
                                CastMemberCard(
                                    name = member.name,
                                    character = member.character,
                                    photoUrl = viewModel.castPhotoUrl(member),
                                )
                            }
                        }
                    }
                    Spacer(Modifier.height(16.dp))
                }

                // Season tabs + episode list
                if (effectiveSeasonCount > 0) {
                    SectionHeader("Seasons")
                    Spacer(Modifier.height(8.dp))

                    ScrollableTabRow(
                        selectedTabIndex = (selectedSeason - 1).coerceIn(
                            0, effectiveSeasonCount - 1
                        ),
                        edgePadding = 0.dp,
                    ) {
                        (1..effectiveSeasonCount).forEach { seasonNum ->
                            // Use season name from batch data if available,
                            // fall back to "Season N"
                            val seasonName = seasons.getOrNull(seasonNum - 1)
                                ?.details?.name
                                ?: "Season $seasonNum"
                            Tab(
                                selected = seasonNum == selectedSeason,
                                onClick = { viewModel.selectSeason(seasonNum) },
                                text = { Text(seasonName) },
                            )
                        }
                    }

                    Spacer(Modifier.height(12.dp))

                    if (episodes.isNotEmpty()) {
                        // Full episode list from batch data
                        episodes.forEach { episode ->
                            val key = DetailViewModel.episodeKey(
                                episode.seasonNumber.toInt(),
                                episode.episodeNumber.toInt(),
                            )
                            val watchInfo = episodeStates[key]
                            EpisodeCard(
                                episode = episode,
                                watchInfo = watchInfo,
                                stillUrl = viewModel.episodeStillUrl(episode),
                                onClick = {
                                    val fileId = viewModel.episodeStreamFileId(episode)
                                    if (fileId != null) onEpisodeClick(fileId)
                                },
                            )
                            Spacer(Modifier.height(8.dp))
                        }
                    } else {
                        // No episode data in batch — show summary from metadata
                        val totalEpisodes = details?.numberOfEpisodes?.toInt() ?: 0
                        val availableEpisodes = details?.availableEpisodes?.toInt() ?: 0
                        Text(
                            text = buildString {
                                if (availableEpisodes > 0) {
                                    append("$availableEpisodes episodes available")
                                } else if (totalEpisodes > 0) {
                                    append("$totalEpisodes episodes")
                                }
                            }.ifEmpty { "Episodes loading…" },
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(vertical = 8.dp),
                        )
                    }
                }

                Spacer(Modifier.height(32.dp))
            }
        }
    }
}

/**
 * Episode card — still image, title, overview, runtime, watch progress.
 */
@Composable
private fun EpisodeCard(
    episode: EpisodeReference,
    watchInfo: EpisodeWatchInfo?,
    stillUrl: String?,
    onClick: () -> Unit,
) {
    val details = episode.details

    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(8.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
        ),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(100.dp),
        ) {
            // Episode still image
            Box(
                modifier = Modifier
                    .width(160.dp)
                    .height(100.dp)
                    .clip(RoundedCornerShape(topStart = 8.dp, bottomStart = 8.dp)),
            ) {
                if (stillUrl != null) {
                    AsyncImage(
                        model = stillUrl,
                        contentDescription = null,
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
                            text = "E${episode.episodeNumber}",
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }

                // Watch status overlay
                if (watchInfo != null) {
                    if (watchInfo.isCompleted) {
                        Box(
                            modifier = Modifier
                                .fillMaxSize()
                                .background(Color.Black.copy(alpha = 0.4f)),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                Icons.Default.CheckCircle,
                                contentDescription = "Watched",
                                tint = Color.White,
                                modifier = Modifier.size(32.dp),
                            )
                        }
                    } else if (watchInfo.isInProgress) {
                        Box(modifier = Modifier.fillMaxSize()) {
                            LinearProgressIndicator(
                                progress = { watchInfo.progress },
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(3.dp)
                                    .align(Alignment.BottomCenter),
                                color = MaterialTheme.colorScheme.primary,
                                trackColor = Color.Black.copy(alpha = 0.5f),
                            )
                        }
                    }
                }

                // Play icon overlay
                if (watchInfo?.isCompleted != true) {
                    Box(
                        modifier = Modifier.fillMaxSize(),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            Icons.Default.PlayArrow,
                            contentDescription = "Play",
                            tint = Color.White.copy(alpha = 0.8f),
                            modifier = Modifier.size(36.dp),
                        )
                    }
                }
            }

            // Episode info
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                Column {
                    Text(
                        text = "Episode ${episode.episodeNumber}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    details?.name?.let { name ->
                        Text(
                            text = name,
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    details?.overview?.let { overview ->
                        if (overview.isNotBlank()) {
                            Text(
                                text = overview,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.padding(top = 2.dp),
                            )
                        }
                    }
                }

                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    details?.let { d ->
                        if (d.runtime > 0u) {
                            Text(
                                text = formatRuntime(d.runtime.toInt()),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    details?.let { d ->
                        if (d.voteAverage > 0f) {
                            Text(
                                text = "★ %.1f".format(d.voteAverage),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
        }
    }
}
