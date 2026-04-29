package com.ferrex.android.ui.tv

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.core.watch.WatchProgress
import com.ferrex.android.ui.detail.DetailUiState
import com.ferrex.android.ui.detail.DetailViewModel
import com.ferrex.android.ui.detail.EpisodeWatchInfo
import com.ferrex.android.ui.detail.SeriesPlaybackAction
import com.ferrex.android.ui.detail.SeriesWatchSummary
import com.ferrex.android.ui.detail.formatRuntime
import com.ferrex.android.ui.detail.formatTime
import ferrex.media.EpisodeReference
import ferrex.media.MovieReference
import ferrex.media.SeriesReference

@Composable
fun TvMovieDetailScreen(
    viewModel: DetailViewModel,
    onBack: () -> Unit,
    onPlay: (mediaId: String, startPositionMs: Long?) -> Unit,
) {
    BackHandler(onBack = onBack)
    val uiState by viewModel.uiState.collectAsState()
    val watchProgress by viewModel.watchProgress.collectAsState()

    when (val state = uiState) {
        is DetailUiState.Loading -> TvLoadingScreen("Loading movie…")
        is DetailUiState.Error -> TvErrorScreen(message = state.message, onBack = onBack)
        is DetailUiState.SeriesDetail -> TvErrorScreen(message = "Expected a movie", onBack = onBack)
        is DetailUiState.MovieDetail -> TvMovieDetailContent(
            movie = state.movie,
            watchProgress = watchProgress,
            backdropUrl = viewModel.backdropUrl(state.movie),
            posterUrl = viewModel.posterUrl(state.movie),
            onBack = onBack,
            onPlay = { startPositionMs ->
                state.movie.file?.id?.toUuidString()?.let { mediaId ->
                    onPlay(mediaId, startPositionMs)
                }
            },
            onToggleWatched = viewModel::setMovieWatched,
        )
    }
}

@Composable
fun TvSeriesDetailScreen(
    viewModel: DetailViewModel,
    onBack: () -> Unit,
    onEpisodeClick: (mediaId: String, startPositionMs: Long?) -> Unit,
) {
    BackHandler(onBack = onBack)
    val uiState by viewModel.uiState.collectAsState()

    when (val state = uiState) {
        is DetailUiState.Loading -> TvLoadingScreen("Loading series…")
        is DetailUiState.Error -> TvErrorScreen(message = state.message, onBack = onBack)
        is DetailUiState.MovieDetail -> TvErrorScreen(message = "Expected a series", onBack = onBack)
        is DetailUiState.SeriesDetail -> TvSeriesDetailContent(
            viewModel = viewModel,
            series = state.series,
            backdropUrl = viewModel.seriesBackdropUrl(state.series),
            posterUrl = viewModel.seriesPosterUrl(state.series),
            onBack = onBack,
            onEpisodeClick = onEpisodeClick,
            onToggleSeriesWatched = viewModel::setSeriesWatched,
        )
    }
}

@Composable
private fun TvMovieDetailContent(
    movie: MovieReference,
    watchProgress: WatchProgress?,
    backdropUrl: String?,
    posterUrl: String?,
    onBack: () -> Unit,
    onPlay: (startPositionMs: Long?) -> Unit,
    onToggleWatched: (Boolean) -> Unit,
) {
    val details = movie.details
    val isResumable = watchProgress?.let { it.progress > 0f && !it.completed } == true
    val isWatched = watchProgress?.completed == true
    var pendingMovieToggle by remember { mutableStateOf<Boolean?>(null) }
    val castItems = remember(details) {
        val count = details?.castLength ?: 0
        (0 until count.coerceAtMost(20)).mapNotNull { index ->
            details?.cast(index)?.let { member ->
                TvCastItem(
                    name = member.name,
                    character = member.character,
                )
            }
        }
    }

    pendingMovieToggle?.let { markWatched ->
        AlertDialog(
            onDismissRequest = { pendingMovieToggle = null },
            title = { Text(if (markWatched) "Mark movie watched?" else "Mark movie unwatched?") },
            text = { Text("This movie already has watch progress. Confirm that you want to update its watched state.") },
            confirmButton = {
                Button(
                    onClick = {
                        pendingMovieToggle = null
                        onToggleWatched(markWatched)
                    },
                ) { Text("Confirm") }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingMovieToggle = null }) { Text("Cancel") }
            },
        )
    }

    TvDetailScaffold(
        title = movie.title,
        subtitle = movieSubtitle(movie),
        overview = details?.overview,
        backdropUrl = backdropUrl,
        posterUrl = posterUrl,
        onBack = onBack,
        primaryAction = {
            TvActionColumn {
                TvPlayButton(
                    label = if (isResumable) "Resume" else "Play",
                    onClick = { onPlay(null) },
                )
                if (isResumable) {
                    TvSecondaryActionButton(
                        label = "Start from beginning",
                        onClick = { onPlay(0L) },
                    )
                }
                TvSecondaryActionButton(
                    label = if (isWatched) "Mark unwatched" else "Mark watched",
                    onClick = {
                        val markWatched = !isWatched
                        if (watchProgress != null) {
                            pendingMovieToggle = markWatched
                        } else {
                            onToggleWatched(markWatched)
                        }
                    },
                )
            }
        },
        extraContent = {
            if (watchProgress != null && !watchProgress.completed && watchProgress.progress > 0f) {
                item {
                    TvProgressPanel(watchProgress = watchProgress)
                }
            }
            if (castItems.isNotEmpty()) {
                item {
                    TvCastRow(castItems)
                }
            }
        },
    )
}

@Composable
private fun TvSeriesDetailContent(
    viewModel: DetailViewModel,
    series: SeriesReference,
    backdropUrl: String?,
    posterUrl: String?,
    onBack: () -> Unit,
    onEpisodeClick: (mediaId: String, startPositionMs: Long?) -> Unit,
    onToggleSeriesWatched: (Boolean) -> Unit,
) {
    val details = series.details
    val episodes by viewModel.episodes.collectAsState()
    val seasons by viewModel.seasons.collectAsState()
    val selectedSeason by viewModel.selectedSeason.collectAsState()
    val episodeStates by viewModel.episodeStates.collectAsState()
    val seriesPrimaryAction by viewModel.seriesPrimaryAction.collectAsState()
    val seriesStartOverAction by viewModel.seriesStartOverAction.collectAsState()
    val seriesWatchSummary by viewModel.seriesWatchSummary.collectAsState()
    val isSubmittingWatchAction by viewModel.isSubmittingWatchAction.collectAsState()
    val isFullyWatched = seriesWatchSummary?.isFullyWatched == true
    val requiresSeriesConfirmation = seriesWatchSummary?.hasExistingProgress == true
    var pendingSeriesToggle by remember { mutableStateOf<Boolean?>(null) }
    val metadataSeasonCount = details?.numberOfSeasons?.toInt() ?: 0
    val effectiveSeasonCount = if (seasons.isNotEmpty()) seasons.size else metadataSeasonCount

    pendingSeriesToggle?.let { markWatched ->
        AlertDialog(
            onDismissRequest = { pendingSeriesToggle = null },
            title = { Text(if (markWatched) "Mark series watched?" else "Mark series unwatched?") },
            text = { Text("This series already has watch progress. Confirm that you want to update every known episode in the series.") },
            confirmButton = {
                Button(
                    onClick = {
                        pendingSeriesToggle = null
                        onToggleSeriesWatched(markWatched)
                    },
                ) { Text("Confirm") }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingSeriesToggle = null }) { Text("Cancel") }
            },
        )
    }

    TvDetailScaffold(
        title = series.title,
        subtitle = seriesSubtitle(series),
        overview = details?.overview,
        backdropUrl = backdropUrl,
        posterUrl = posterUrl,
        onBack = onBack,
        primaryAction = {
            TvActionColumn {
                seriesPrimaryAction?.let { action ->
                    TvPlayButton(
                        label = seriesPlaybackActionLabel(action),
                        onClick = { onEpisodeClick(action.mediaId, action.startPositionMs) },
                    )
                }
                seriesStartOverAction
                    ?.takeIf { startAction ->
                        seriesPrimaryAction?.let {
                            it.mediaId != startAction.mediaId || it.startPositionMs != startAction.startPositionMs
                        } ?: true
                    }
                    ?.let { action ->
                        TvSecondaryActionButton(
                            label = seriesPlaybackActionLabel(action),
                            onClick = { onEpisodeClick(action.mediaId, action.startPositionMs) },
                        )
                    }
                TvSecondaryActionButton(
                    label = if (isSubmittingWatchAction) {
                        "Saving…"
                    } else if (isFullyWatched) {
                        "Mark series unwatched"
                    } else {
                        "Mark series watched"
                    },
                    enabled = !isSubmittingWatchAction,
                    onClick = {
                        val markWatched = !isFullyWatched
                        if (requiresSeriesConfirmation) {
                            pendingSeriesToggle = markWatched
                        } else {
                            onToggleSeriesWatched(markWatched)
                        }
                    },
                )
            }
        },
        extraContent = {
            seriesWatchSummary?.let { summary ->
                item {
                    TvSeriesWatchSummaryPanel(summary)
                }
            }

            if (effectiveSeasonCount > 0) {
                item {
                    TvSeasonRow(
                        seasonCount = effectiveSeasonCount,
                        selectedSeason = selectedSeason,
                        seasonName = { seasonNumber ->
                            seasons.getOrNull(seasonNumber - 1)?.details?.name ?: "Season $seasonNumber"
                        },
                        onSeasonClick = viewModel::selectSeason,
                    )
                }
            }

            if (episodes.isNotEmpty()) {
                item {
                    LazyRow(
                        contentPadding = PaddingValues(horizontal = 56.dp, vertical = 12.dp),
                        horizontalArrangement = Arrangement.spacedBy(24.dp),
                    ) {
                        items(
                            items = episodes,
                            key = { episode ->
                                "S${episode.seasonNumber}E${episode.episodeNumber}"
                            },
                        ) { episode ->
                            val key = DetailViewModel.episodeKey(
                                episode.seasonNumber.toInt(),
                                episode.episodeNumber.toInt(),
                            )
                            TvEpisodeCard(
                                episode = episode,
                                watchInfo = episodeStates[key],
                                stillUrl = viewModel.episodeStillUrl(episode),
                                onClick = {
                                    viewModel.episodeStreamFileId(episode)?.let { mediaId ->
                                        onEpisodeClick(mediaId, null)
                                    }
                                },
                                onStartOver = {
                                    viewModel.episodeStreamFileId(episode)?.let { mediaId ->
                                        onEpisodeClick(mediaId, 0L)
                                    }
                                },
                            )
                        }
                    }
                }
            } else {
                item {
                    Text(
                        text = seriesEpisodeSummary(series),
                        style = MaterialTheme.typography.titleLarge,
                        color = Color.White.copy(alpha = 0.72f),
                        modifier = Modifier.padding(horizontal = 56.dp, vertical = 8.dp),
                    )
                }
            }
        },
    )
}

@Composable
private fun TvDetailScaffold(
    title: String,
    subtitle: String?,
    overview: String?,
    backdropUrl: String?,
    posterUrl: String?,
    onBack: () -> Unit,
    primaryAction: (@Composable () -> Unit)?,
    extraContent: LazyColumnScopeContent,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xFF070A12))
            .windowInsetsPadding(WindowInsets.safeDrawing),
    ) {
        if (backdropUrl != null) {
            AsyncImage(
                model = backdropUrl,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
        }
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.horizontalGradient(
                        listOf(
                            Color(0xFF070A12),
                            Color(0xEE070A12),
                            Color(0x66070A12),
                        ),
                    ),
                ),
        )
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        listOf(Color.Transparent, Color(0xFF070A12)),
                        startY = 460f,
                    ),
                ),
        )

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(28.dp),
        ) {
            item {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 56.dp, vertical = 36.dp),
                    horizontalArrangement = Arrangement.spacedBy(36.dp),
                    verticalAlignment = Alignment.Top,
                ) {
                    Column(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.spacedBy(20.dp),
                    ) {
                        Button(
                            onClick = onBack,
                            colors = ButtonDefaults.buttonColors(
                                containerColor = Color.Black.copy(alpha = 0.54f),
                                contentColor = Color.White,
                            ),
                            modifier = Modifier.semantics { contentDescription = "Back" },
                        ) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null)
                            Spacer(Modifier.width(8.dp))
                            Text("Back")
                        }

                        Spacer(Modifier.height(48.dp))
                        Text(
                            text = title,
                            style = MaterialTheme.typography.displayLarge,
                            color = Color.White,
                            fontWeight = FontWeight.Bold,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                        if (!subtitle.isNullOrBlank()) {
                            Text(
                                text = subtitle,
                                style = MaterialTheme.typography.titleLarge,
                                color = Color.White.copy(alpha = 0.78f),
                            )
                        }
                        if (!overview.isNullOrBlank()) {
                            Text(
                                text = overview,
                                style = MaterialTheme.typography.titleLarge,
                                color = Color.White.copy(alpha = 0.86f),
                                maxLines = 4,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        primaryAction?.invoke()
                    }

                    TvPosterPreview(title = title, posterUrl = posterUrl)
                }
            }
            extraContent()
            item { Spacer(Modifier.height(56.dp)) }
        }
    }
}

private typealias LazyColumnScopeContent = androidx.compose.foundation.lazy.LazyListScope.() -> Unit

private data class TvCastItem(
    val name: String,
    val character: String?,
)

@Composable
private fun TvCastRow(items: List<TvCastItem>) {
    Column(modifier = Modifier.fillMaxWidth()) {
        Text(
            text = "Cast",
            style = MaterialTheme.typography.headlineSmall,
            color = Color.White,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = 56.dp),
        )
        Spacer(Modifier.height(16.dp))
        LazyRow(
            contentPadding = PaddingValues(horizontal = 56.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            items(items) { item ->
                Surface(
                    color = Color.White.copy(alpha = 0.10f),
                    shape = MaterialTheme.shapes.large,
                    modifier = Modifier.width(220.dp),
                ) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text(
                            text = item.name,
                            style = MaterialTheme.typography.titleMedium,
                            color = Color.White,
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        if (!item.character.isNullOrBlank()) {
                            Text(
                                text = item.character,
                                style = MaterialTheme.typography.bodyMedium,
                                color = Color.White.copy(alpha = 0.68f),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun TvPosterPreview(
    title: String,
    posterUrl: String?,
) {
    Box(
        modifier = Modifier
            .width(230.dp)
            .aspectRatio(2f / 3f)
            .shadow(26.dp, RoundedCornerShape(18.dp))
            .clip(RoundedCornerShape(18.dp))
            .background(Color(0xFF151922)),
        contentAlignment = Alignment.Center,
    ) {
        if (posterUrl != null) {
            AsyncImage(
                model = posterUrl,
                contentDescription = title,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
        } else {
            Text(
                text = title.take(1).uppercase(),
                style = MaterialTheme.typography.displayLarge,
                color = Color.White.copy(alpha = 0.8f),
                fontWeight = FontWeight.Bold,
            )
        }
    }
}

@Composable
private fun TvActionColumn(content: @Composable ColumnScope.() -> Unit) {
    Column(
        verticalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier.padding(top = 6.dp),
        content = content,
    )
}

@Composable
private fun TvPlayButton(
    label: String,
    onClick: () -> Unit,
) {
    Button(
        onClick = onClick,
        modifier = Modifier
            .width(320.dp)
            .semantics { contentDescription = label },
    ) {
        Icon(Icons.Default.PlayArrow, contentDescription = null)
        Spacer(Modifier.width(8.dp))
        Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun TvSecondaryActionButton(
    label: String,
    onClick: () -> Unit,
    enabled: Boolean = true,
) {
    OutlinedButton(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier
            .width(320.dp)
            .semantics { contentDescription = label },
    ) {
        Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun TvSeriesWatchSummaryPanel(summary: SeriesWatchSummary) {
    val text = when {
        summary.totalEpisodes > 0 -> "${summary.watchedEpisodes}/${summary.totalEpisodes} episodes watched"
        summary.inProgressEpisodes > 0 -> "${summary.inProgressEpisodes} episodes in progress"
        else -> "Watch status will update after playback"
    }
    Surface(
        color = Color.Black.copy(alpha = 0.30f),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier
            .padding(horizontal = 56.dp)
            .fillMaxWidth(),
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.titleMedium,
            color = Color.White,
            modifier = Modifier.padding(24.dp),
        )
    }
}

private fun seriesPlaybackActionLabel(action: SeriesPlaybackAction): String =
    action.subtitle?.let { "${action.label} • $it" } ?: action.label

@Composable
private fun TvProgressPanel(watchProgress: WatchProgress) {
    Surface(
        color = Color.Black.copy(alpha = 0.30f),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier
            .padding(horizontal = 56.dp)
            .fillMaxWidth(),
    ) {
        Column(modifier = Modifier.padding(24.dp)) {
            Text(
                text = "Resume at ${formatTime(watchProgress.position)} of ${formatTime(watchProgress.duration)}",
                style = MaterialTheme.typography.titleMedium,
                color = Color.White,
            )
            Spacer(Modifier.height(10.dp))
            LinearProgressIndicator(
                progress = { watchProgress.progress.coerceIn(0f, 1f) },
                modifier = Modifier
                    .fillMaxWidth()
                    .height(6.dp)
                    .clip(RoundedCornerShape(4.dp)),
            )
        }
    }
}

@Composable
private fun TvSeasonRow(
    seasonCount: Int,
    selectedSeason: Int,
    seasonName: (Int) -> String,
    onSeasonClick: (Int) -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth()) {
        Text(
            text = "Seasons",
            style = MaterialTheme.typography.headlineSmall,
            color = Color.White,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = 56.dp),
        )
        Spacer(Modifier.height(12.dp))
        LazyRow(
            contentPadding = PaddingValues(horizontal = 56.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            items((1..seasonCount).toList()) { seasonNumber ->
                Button(
                    onClick = { onSeasonClick(seasonNumber) },
                    colors = ButtonDefaults.buttonColors(
                        containerColor = if (seasonNumber == selectedSeason) {
                            MaterialTheme.colorScheme.primary
                        } else {
                            Color.White.copy(alpha = 0.14f)
                        },
                        contentColor = Color.White,
                    ),
                ) {
                    Text(seasonName(seasonNumber))
                }
            }
        }
    }
}

@Composable
private fun TvEpisodeCard(
    episode: EpisodeReference,
    watchInfo: EpisodeWatchInfo?,
    stillUrl: String?,
    onClick: () -> Unit,
    onStartOver: () -> Unit,
) {
    var isFocused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(14.dp)
    val title = episode.details?.name?.takeIf { it.isNotBlank() } ?: "Episode ${episode.episodeNumber}"

    Column(
        modifier = Modifier
            .width(360.dp)
            .semantics {
                role = Role.Button
                contentDescription = "Play $title"
                onClick(label = "Play $title") {
                    onClick()
                    true
                }
            }
            .onFocusChanged { isFocused = it.isFocused }
            .onPreviewKeyEvent { event ->
                if (event.type == KeyEventType.KeyUp &&
                    (event.key == Key.DirectionCenter || event.key == Key.Enter || event.key == Key.NumPadEnter)
                ) {
                    onClick()
                    true
                } else {
                    false
                }
            }
            .focusable()
            .graphicsLayer {
                scaleX = if (isFocused) 1.06f else 1f
                scaleY = if (isFocused) 1.06f else 1f
            }
            .border(
                width = if (isFocused) 4.dp else 1.dp,
                color = if (isFocused) MaterialTheme.colorScheme.primary else Color.White.copy(alpha = 0.16f),
                shape = shape,
            )
            .clip(shape)
            .background(Color(0xFF151922)),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .aspectRatio(16f / 9f),
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
                        .background(Color(0xFF242B3A)),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "E${episode.episodeNumber}",
                        style = MaterialTheme.typography.displaySmall,
                        color = Color.White.copy(alpha = 0.82f),
                        fontWeight = FontWeight.Bold,
                    )
                }
            }

            if (watchInfo?.isCompleted == true) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(Color.Black.copy(alpha = 0.44f)),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.Default.CheckCircle,
                        contentDescription = "Watched",
                        tint = Color.White,
                        modifier = Modifier.size(44.dp),
                    )
                }
            } else {
                Icon(
                    Icons.Default.PlayArrow,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier
                        .align(Alignment.Center)
                        .size(46.dp),
                )
            }

            if (watchInfo?.isInProgress == true) {
                LinearProgressIndicator(
                    progress = { watchInfo.progress.coerceIn(0f, 1f) },
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(5.dp)
                        .align(Alignment.BottomCenter),
                    trackColor = Color.Black.copy(alpha = 0.55f),
                )
            }
        }
        Column(modifier = Modifier.padding(14.dp)) {
            Text(
                text = "Episode ${episode.episodeNumber}",
                style = MaterialTheme.typography.labelLarge,
                color = Color.White.copy(alpha = 0.66f),
            )
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
                color = Color.White,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            episode.details?.overview?.takeIf { it.isNotBlank() }?.let { overview ->
                Text(
                    text = overview,
                    style = MaterialTheme.typography.bodyMedium,
                    color = Color.White.copy(alpha = 0.70f),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (watchInfo?.isInProgress == true) {
                Spacer(Modifier.height(10.dp))
                OutlinedButton(
                    onClick = onStartOver,
                    modifier = Modifier.semantics { contentDescription = "Start $title from beginning" },
                ) {
                    Text("Start from beginning")
                }
            }
        }
    }
}

@Composable
private fun TvLoadingScreen(message: String) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xFF070A12)),
        contentAlignment = Alignment.Center,
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator()
            Text(message, style = MaterialTheme.typography.titleLarge, color = Color.White)
        }
    }
}

@Composable
private fun TvErrorScreen(
    message: String,
    onBack: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xFF070A12)),
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(18.dp)) {
            Text(message, style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.error)
            Button(onClick = onBack) { Text("Back") }
        }
    }
}

private fun movieSubtitle(movie: MovieReference): String {
    val details = movie.details
    return listOfNotNull(
        details?.releaseDate?.take(4)?.takeIf { it.isNotBlank() },
        details?.runtime?.toInt()?.takeIf { it > 0 }?.let(::formatRuntime),
        details?.voteAverage?.takeIf { it > 0f }?.let { "★ %.1f".format(it) },
        details?.contentRating?.takeIf { it.isNotBlank() },
    ).joinToString("  •  ")
}

private fun seriesSubtitle(series: SeriesReference): String {
    val details = series.details
    val seasons = details?.numberOfSeasons?.toInt() ?: 0
    return listOfNotNull(
        details?.firstAirDate?.take(4)?.takeIf { it.isNotBlank() },
        seasons.takeIf { it > 0 }?.let { "$it season${if (it == 1) "" else "s"}" },
        details?.voteAverage?.takeIf { it > 0f }?.let { "★ %.1f".format(it) },
        details?.status?.takeIf { it.isNotBlank() },
    ).joinToString("  •  ")
}

private fun seriesEpisodeSummary(series: SeriesReference): String {
    val details = series.details
    val availableEpisodes = details?.availableEpisodes?.toInt() ?: 0
    val totalEpisodes = details?.numberOfEpisodes?.toInt() ?: 0
    return when {
        availableEpisodes > 0 -> "$availableEpisodes episodes available"
        totalEpisodes > 0 -> "$totalEpisodes episodes"
        else -> "Episode details are not available yet. Try again after the library finishes syncing."
    }
}
