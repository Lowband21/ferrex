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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
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
import com.ferrex.android.core.library.toUuidString
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import ferrex.media.EpisodeReference
import ferrex.media.SeriesReference
import ferrex.watch.EpisodeWatchState
import kotlinx.coroutines.delay

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
    onEpisodeClick: (mediaId: String, startPositionMs: Long?) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsState()
    val seriesWatchSummary by viewModel.seriesWatchSummary.collectAsState()
    val watchActionMessage by viewModel.watchActionMessage.collectAsState()
    val isSubmittingWatchAction by viewModel.isSubmittingWatchAction.collectAsState()
    val lifecycleOwner = LocalLifecycleOwner.current
    val snackbarHostState = remember { SnackbarHostState() }
    var pendingConfirmation by remember { mutableStateOf<SeriesWatchToggleAction?>(null) }
    var pendingEpisodeConfirmation by remember { mutableStateOf<EpisodeWatchToggleRequest?>(null) }

    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                viewModel.refreshWatchData()
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
            viewModel.refreshWatchData()
        }
    }

    LaunchedEffect(watchActionMessage) {
        watchActionMessage?.let {
            snackbarHostState.showSnackbar(it)
            viewModel.consumeWatchActionMessage()
        }
    }

    when (val state = uiState) {
        is DetailUiState.Loading -> LoadingScreen()
        is DetailUiState.Error -> ErrorScreen(
            message = state.message,
            onRetry = onBack,
        )
        is DetailUiState.MovieDetail -> ErrorScreen(message = "Expected series, got movie")
        is DetailUiState.SeriesDetail -> {
            val isFullyWatched = seriesWatchSummary?.isFullyWatched == true
            val requiresConfirmation = seriesWatchSummary?.hasExistingProgress == true

            if (pendingConfirmation != null) {
                AlertDialog(
                    onDismissRequest = { pendingConfirmation = null },
                    title = {
                        Text(
                            if (pendingConfirmation == SeriesWatchToggleAction.MarkWatched) {
                                "Mark series watched?"
                            } else {
                                "Mark series unwatched?"
                            }
                        )
                    },
                    text = {
                        Text(
                            "This series already has watch progress. Are you sure you want to ${if (pendingConfirmation == SeriesWatchToggleAction.MarkWatched) "mark the whole series watched" else "mark the whole series unwatched"}?"
                        )
                    },
                    confirmButton = {
                        Button(
                            onClick = {
                                val markWatched = pendingConfirmation == SeriesWatchToggleAction.MarkWatched
                                pendingConfirmation = null
                                viewModel.setSeriesWatched(markWatched)
                            },
                        ) {
                            Text("Confirm")
                        }
                    },
                    dismissButton = {
                        OutlinedButton(onClick = { pendingConfirmation = null }) {
                            Text("Cancel")
                        }
                    },
                )
            }

            pendingEpisodeConfirmation?.let { request ->
                AlertDialog(
                    onDismissRequest = { pendingEpisodeConfirmation = null },
                    title = {
                        Text(
                            if (request.markWatched) {
                                "Mark episode watched?"
                            } else {
                                "Mark episode unwatched?"
                            }
                        )
                    },
                    text = {
                        Text(
                            "${request.episodeLabel} already has watch progress. Are you sure you want to ${if (request.markWatched) "mark it watched" else "mark it unwatched"}?"
                        )
                    },
                    confirmButton = {
                        Button(
                            onClick = {
                                val pending = pendingEpisodeConfirmation ?: return@Button
                                pendingEpisodeConfirmation = null
                                viewModel.setEpisodeWatched(pending.episodeId, pending.markWatched)
                            },
                        ) {
                            Text("Confirm")
                        }
                    },
                    dismissButton = {
                        OutlinedButton(onClick = { pendingEpisodeConfirmation = null }) {
                            Text("Cancel")
                        }
                    },
                )
            }

            SeriesDetailContent(
                viewModel = viewModel,
                series = state.series,
                backdropUrl = viewModel.seriesBackdropUrl(state.series),
                posterUrl = viewModel.seriesPosterUrl(state.series),
                snackbarHostState = snackbarHostState,
                seriesWatchSummary = seriesWatchSummary,
                isSubmittingWatchAction = isSubmittingWatchAction,
                onToggleWatched = {
                    val action = if (isFullyWatched) {
                        SeriesWatchToggleAction.MarkUnwatched
                    } else {
                        SeriesWatchToggleAction.MarkWatched
                    }
                    if (requiresConfirmation) {
                        pendingConfirmation = action
                    } else {
                        viewModel.setSeriesWatched(action == SeriesWatchToggleAction.MarkWatched)
                    }
                },
                onToggleEpisodeWatched = { request ->
                    if (request.requiresConfirmation) {
                        pendingEpisodeConfirmation = request
                    } else {
                        viewModel.setEpisodeWatched(request.episodeId, request.markWatched)
                    }
                },
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
    snackbarHostState: SnackbarHostState,
    seriesWatchSummary: SeriesWatchSummary?,
    isSubmittingWatchAction: Boolean,
    onToggleWatched: () -> Unit,
    onToggleEpisodeWatched: (EpisodeWatchToggleRequest) -> Unit,
    onBack: () -> Unit,
    onEpisodeClick: (mediaId: String, startPositionMs: Long?) -> Unit,
) {
    val details = series.details
    val scrollState = rememberScrollState()

    // Episode data from the batch cache (populated when the server's per-series
    // bundle endpoint returns season/episode items in FlatBuffers format).
    val episodes by viewModel.episodes.collectAsState()
    val selectedSeason by viewModel.selectedSeason.collectAsState()
    val episodeStates by viewModel.episodeStates.collectAsState()
    val seriesPrimaryAction by viewModel.seriesPrimaryAction.collectAsState()
    val seriesStartOverAction by viewModel.seriesStartOverAction.collectAsState()

    // Season count from metadata — always available, even without per-series bundle
    val metadataSeasonCount = details?.numberOfSeasons?.toInt() ?: 0
    // Actual season objects from the batch (may be empty until server support lands)
    val seasons by viewModel.seasons.collectAsState()
    // Use whichever source has data: batch seasons or metadata count
    val effectiveSeasonCount = if (seasons.isNotEmpty()) seasons.size else metadataSeasonCount

    Scaffold(
        snackbarHost = { SnackbarHost(snackbarHostState) },
        topBar = {
            TopAppBar(
                title = {},
                navigationIcon = {
                    IconButton(
                        onClick = onBack,
                        modifier = Modifier
                            .clip(RoundedCornerShape(24.dp))
                            .background(Color.Black.copy(alpha = 0.36f)),
                    ) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                            tint = Color.White,
                        )
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
            SeriesHero(
                title = series.title,
                tagline = details?.tagline,
                backdropUrl = backdropUrl,
                posterUrl = posterUrl,
            )

            Column(modifier = Modifier.padding(horizontal = 16.dp)) {
                Spacer(Modifier.height(14.dp))

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

                SeriesPlaybackActionsSection(
                    primaryAction = seriesPrimaryAction,
                    startOverAction = seriesStartOverAction,
                    onPlay = { action ->
                        onEpisodeClick(action.mediaId, action.startPositionMs)
                    },
                )

                if (seriesPrimaryAction != null || seriesStartOverAction != null) {
                    Spacer(Modifier.height(12.dp))
                }

                SeriesWatchActionsSection(
                    summary = seriesWatchSummary,
                    isSubmittingWatchAction = isSubmittingWatchAction,
                    onToggleWatched = onToggleWatched,
                )

                if (seriesPrimaryAction != null || seriesStartOverAction != null || seriesWatchSummary != null) {
                    Spacer(Modifier.height(16.dp))
                }

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
                                isSubmittingWatchAction = isSubmittingWatchAction,
                                onPlay = { startPositionMs ->
                                    val fileId = viewModel.episodeStreamFileId(episode)
                                    if (fileId != null) onEpisodeClick(fileId, startPositionMs)
                                },
                                onToggleWatched = { request ->
                                    onToggleEpisodeWatched(request)
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

@Composable
private fun SeriesPlaybackActionsSection(
    primaryAction: SeriesPlaybackAction?,
    startOverAction: SeriesPlaybackAction?,
    onPlay: (SeriesPlaybackAction) -> Unit,
) {
    if (primaryAction == null && startOverAction == null) return

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        primaryAction?.let { action ->
            Button(
                onClick = { onPlay(action) },
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp),
                shape = RoundedCornerShape(14.dp),
            ) {
                Icon(Icons.Default.PlayArrow, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text(seriesPlaybackActionLabel(action))
            }
        }

        startOverAction
            ?.takeIf { startAction ->
                primaryAction?.let {
                    it.mediaId != startAction.mediaId || it.startPositionMs != startAction.startPositionMs
                } ?: true
            }
            ?.let { action ->
                OutlinedButton(
                    onClick = { onPlay(action) },
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(48.dp),
                    shape = RoundedCornerShape(14.dp),
                ) {
                    Text(seriesPlaybackActionLabel(action))
                }
            }
    }
}

private fun seriesPlaybackActionLabel(action: SeriesPlaybackAction): String =
    action.subtitle?.let { "${action.label} • $it" } ?: action.label

@Composable
private fun SeriesWatchActionsSection(
    summary: SeriesWatchSummary?,
    isSubmittingWatchAction: Boolean,
    onToggleWatched: () -> Unit,
) {
    val isFullyWatched = summary?.isFullyWatched == true
    val statusText = summary?.let {
        when {
            it.totalEpisodes > 0 -> "${it.watchedEpisodes}/${it.totalEpisodes} episodes watched"
            else -> null
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        statusText?.let {
            Text(
                text = it,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        OutlinedButton(
            onClick = onToggleWatched,
            modifier = Modifier
                .fillMaxWidth()
                .height(48.dp),
            enabled = !isSubmittingWatchAction,
            shape = RoundedCornerShape(14.dp),
        ) {
            Text(
                if (isSubmittingWatchAction) {
                    "Saving…"
                } else if (isFullyWatched) {
                    "Mark unwatched"
                } else {
                    "Mark watched"
                }
            )
        }
    }
}

/**
 * Series hero — backdrop, poster, title overlay. Mirrors MovieHero layout.
 */
@Composable
private fun SeriesHero(
    title: String,
    tagline: String?,
    backdropUrl: String?,
    posterUrl: String?,
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(16f / 10f)
            .background(MaterialTheme.colorScheme.surfaceVariant),
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
                            Color.Black.copy(alpha = 0.12f),
                            Color.Transparent,
                            MaterialTheme.colorScheme.background,
                        ),
                    ),
                ),
        )
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.horizontalGradient(
                        colors = listOf(
                            Color.Black.copy(alpha = 0.62f),
                            Color.Transparent,
                        ),
                    ),
                ),
        )

        Row(
            modifier = Modifier
                .align(Alignment.BottomStart)
                .padding(start = 16.dp, end = 16.dp, bottom = 18.dp),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            if (posterUrl != null) {
                AsyncImage(
                    model = posterUrl,
                    contentDescription = title,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier
                        .width(86.dp)
                        .aspectRatio(2f / 3f)
                        .clip(RoundedCornerShape(10.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant),
                )
            }

            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.Bold,
                    color = Color.White,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                if (!tagline.isNullOrBlank()) {
                    Spacer(Modifier.height(6.dp))
                    Text(
                        text = tagline,
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.Medium,
                        color = Color.White.copy(alpha = 0.86f),
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
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
    isSubmittingWatchAction: Boolean,
    onPlay: (Long?) -> Unit,
    onToggleWatched: (EpisodeWatchToggleRequest) -> Unit,
) {
    val details = episode.details
    val episodeId = episode.id?.toUuidString()
    val isWatched = watchInfo?.isCompleted == true
    val requiresConfirmation = watchInfo?.state?.let { it != EpisodeWatchState.Unwatched } == true
    val episodeLabel = buildString {
        append(DetailViewModel.formatEpisodeLabel(
            episode.seasonNumber.toInt(),
            episode.episodeNumber.toInt(),
        ))
        details?.name?.takeIf { it.isNotBlank() }?.let {
            append(" • ")
            append(it)
        }
    }

    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = { onPlay(null) }),
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

                Spacer(Modifier.height(6.dp))
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (watchInfo?.isInProgress == true) {
                        androidx.compose.material3.TextButton(
                            onClick = { onPlay(0L) },
                            contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 8.dp, vertical = 0.dp),
                        ) {
                            Text(text = "Start From Beginning")
                        }
                    }

                    if (episodeId != null) {
                        androidx.compose.material3.TextButton(
                            onClick = {
                                onToggleWatched(
                                    EpisodeWatchToggleRequest(
                                        episodeId = episodeId,
                                        episodeLabel = episodeLabel,
                                        markWatched = !isWatched,
                                        requiresConfirmation = requiresConfirmation,
                                    )
                                )
                            },
                            enabled = !isSubmittingWatchAction,
                            contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 8.dp, vertical = 0.dp),
                        ) {
                            Text(
                                text = if (isSubmittingWatchAction) {
                                    "Saving…"
                                } else if (isWatched) {
                                    "Mark unwatched"
                                } else {
                                    "Mark watched"
                                }
                            )
                        }
                    }
                }
            }
        }
    }
}

private data class EpisodeWatchToggleRequest(
    val episodeId: String,
    val episodeLabel: String,
    val markWatched: Boolean,
    val requiresConfirmation: Boolean,
)

private enum class SeriesWatchToggleAction {
    MarkWatched,
    MarkUnwatched,
}
