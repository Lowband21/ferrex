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
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
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
import com.ferrex.android.core.watch.WatchProgress
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import ferrex.details.CastMember
import ferrex.media.MovieReference
import kotlinx.coroutines.delay

/**
 * Movie detail screen — backdrop, metadata, cast, play button, watch status.
 *
 * Data comes from the locally cached batch data (zero-copy FlatBuffers),
 * so this screen loads instantly without a network call. Watch state is
 * fetched asynchronously from the server.
 */
@Composable
fun MovieDetailScreen(
    viewModel: DetailViewModel,
    onBack: () -> Unit,
    onPlay: (mediaId: String, startPositionMs: Long?) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsState()
    val watchProgress by viewModel.watchProgress.collectAsState()
    val watchActionMessage by viewModel.watchActionMessage.collectAsState()
    val isSubmittingWatchAction by viewModel.isSubmittingWatchAction.collectAsState()
    val lifecycleOwner = LocalLifecycleOwner.current
    val snackbarHostState = remember { SnackbarHostState() }
    var pendingConfirmation by remember { mutableStateOf<MovieWatchToggleAction?>(null) }

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
        is DetailUiState.SeriesDetail -> ErrorScreen(message = "Expected movie, got series")
        is DetailUiState.MovieDetail -> {
            val isWatched = watchProgress?.completed == true
            val requiresConfirmation = watchProgress?.let {
                it.progress > 0f || it.completed
            } == true

            if (pendingConfirmation != null) {
                AlertDialog(
                    onDismissRequest = { pendingConfirmation = null },
                    title = {
                        Text(
                            if (pendingConfirmation == MovieWatchToggleAction.MarkWatched) {
                                "Mark watched?"
                            } else {
                                "Mark unwatched?"
                            }
                        )
                    },
                    text = {
                        Text(
                            "This movie already has watch progress. Are you sure you want to ${if (pendingConfirmation == MovieWatchToggleAction.MarkWatched) "mark it watched" else "mark it unwatched"}?"
                        )
                    },
                    confirmButton = {
                        Button(
                            onClick = {
                                val markWatched = pendingConfirmation == MovieWatchToggleAction.MarkWatched
                                pendingConfirmation = null
                                viewModel.setMovieWatched(markWatched)
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

            MovieDetailContent(
                movie = state.movie,
                watchProgress = watchProgress,
                backdropUrl = viewModel.backdropUrl(state.movie),
                posterUrl = viewModel.posterUrl(state.movie),
                castPhotoUrl = { member -> viewModel.castPhotoUrl(member) },
                snackbarHostState = snackbarHostState,
                isSubmittingWatchAction = isSubmittingWatchAction,
                isWatched = isWatched,
                onToggleWatched = {
                    val action = if (isWatched) {
                        MovieWatchToggleAction.MarkUnwatched
                    } else {
                        MovieWatchToggleAction.MarkWatched
                    }
                    if (requiresConfirmation) {
                        pendingConfirmation = action
                    } else {
                        viewModel.setMovieWatched(action == MovieWatchToggleAction.MarkWatched)
                    }
                },
                onBack = onBack,
                onPlay = { forcedStartMs ->
                    val fileId = state.movie.file?.id?.toUuidString()
                    if (fileId != null) {
                        onPlay(fileId, forcedStartMs)
                    }
                },
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MovieDetailContent(
    movie: MovieReference,
    watchProgress: WatchProgress?,
    backdropUrl: String?,
    posterUrl: String?,
    castPhotoUrl: (CastMember) -> String?,
    snackbarHostState: SnackbarHostState,
    isSubmittingWatchAction: Boolean,
    isWatched: Boolean,
    onToggleWatched: () -> Unit,
    onBack: () -> Unit,
    onPlay: (Long?) -> Unit,
) {
    val details = movie.details
    val scrollState = rememberScrollState()

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
            MovieHero(
                title = movie.title,
                tagline = details?.tagline,
                backdropUrl = backdropUrl,
                posterUrl = posterUrl,
            )

            Column(modifier = Modifier.padding(horizontal = 16.dp)) {
                Spacer(Modifier.height(14.dp))

                Row(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    details?.releaseDate?.take(4)?.let { year ->
                        if (year.isNotBlank()) {
                            MetadataChip(year)
                        }
                    }
                    details?.let { d ->
                        if (d.runtime > 0u) {
                            MetadataChip(formatRuntime(d.runtime.toInt()))
                        }
                    }
                    details?.let { d ->
                        if (d.voteAverage > 0f) {
                            MetadataChip("★ %.1f".format(d.voteAverage))
                        }
                    }
                    details?.contentRating?.let { rating ->
                        if (rating.isNotBlank()) {
                            MetadataChip(rating)
                        }
                    }
                }

                Spacer(Modifier.height(16.dp))

                WatchStatusSection(
                    watchProgress = watchProgress,
                    isSubmittingWatchAction = isSubmittingWatchAction,
                    isWatched = isWatched,
                    onToggleWatched = onToggleWatched,
                    onPlay = onPlay,
                )

                Spacer(Modifier.height(16.dp))

                details?.overview?.let { overview ->
                    if (overview.isNotBlank()) {
                        Text(
                            text = overview,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Spacer(Modifier.height(16.dp))
                    }
                }

                val genreCount = details?.genresLength ?: 0
                if (genreCount > 0) {
                    SectionHeader("Genres")
                    Spacer(Modifier.height(4.dp))
                    Text(
                        text = (0 until genreCount)
                            .mapNotNull { details?.genres(it)?.name }
                            .joinToString(" · "),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(16.dp))
                }

                val castCount = details?.castLength ?: 0
                if (castCount > 0) {
                    CastSection(
                        details = details,
                        castCount = castCount.coerceAtMost(20),
                        castPhotoUrl = castPhotoUrl,
                    )
                    Spacer(Modifier.height(16.dp))
                }

                TechnicalMetadataSection(movie = movie)

                Spacer(Modifier.height(32.dp))
            }
        }
    }
}

@Composable
private fun MovieHero(
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

@Composable
private fun WatchStatusSection(
    watchProgress: WatchProgress?,
    isSubmittingWatchAction: Boolean,
    isWatched: Boolean,
    onToggleWatched: () -> Unit,
    onPlay: (Long?) -> Unit,
) {
    val isInProgress = watchProgress?.let {
        !it.completed && it.progress > 0f && it.duration > 0.0
    } == true
    val showStartFromBeginning = watchProgress?.let {
        it.completed || (it.progress > 0f && it.duration > 0.0)
    } == true

    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        when {
            watchProgress?.completed == true -> {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier
                        .clip(RoundedCornerShape(12.dp))
                        .background(
                            MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.55f),
                        )
                        .padding(horizontal = 12.dp, vertical = 10.dp),
                ) {
                    Icon(
                        Icons.Default.CheckCircle,
                        contentDescription = "Watched",
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(20.dp),
                    )
                    Text(
                        text = "Watched",
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.Medium,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
            }
            isInProgress -> {
                val progress = checkNotNull(watchProgress)
                val remaining = formatTime(
                    (progress.duration - progress.position).coerceAtLeast(0.0),
                )
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(12.dp))
                        .background(
                            MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.55f),
                        )
                        .padding(12.dp),
                ) {
                    Text(
                        text = "Resume from ${formatTime(progress.position)}",
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.Medium,
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        text = "$remaining remaining",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(8.dp))
                    LinearProgressIndicator(
                        progress = { progress.progress },
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(4.dp)
                            .clip(RoundedCornerShape(2.dp)),
                        trackColor = MaterialTheme.colorScheme.surface,
                    )
                }
            }
        }

        Button(
            onClick = { onPlay(null) },
            modifier = Modifier
                .fillMaxWidth()
                .height(52.dp),
            shape = RoundedCornerShape(14.dp),
        ) {
            Icon(Icons.Default.PlayArrow, contentDescription = null)
            Spacer(Modifier.width(8.dp))
            Text(
                text = if (isInProgress) "Resume" else "Play movie",
                style = MaterialTheme.typography.labelLarge,
            )
        }

        if (showStartFromBeginning) {
            OutlinedButton(
                onClick = { onPlay(0L) },
                modifier = Modifier
                    .fillMaxWidth()
                    .height(48.dp),
                shape = RoundedCornerShape(14.dp),
            ) {
                Text("Start From Beginning")
            }
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
                } else if (isWatched) {
                    "Mark unwatched"
                } else {
                    "Mark watched"
                },
            )
        }
    }
}

@Composable
private fun CastSection(
    details: ferrex.details.EnhancedMovieDetails?,
    castCount: Int,
    castPhotoUrl: (CastMember) -> String?,
) {
    val sortedCastIndices = (0 until castCount)
        .sortedBy { i ->
            val m = details?.cast(i)
            if (m != null && castPhotoUrl(m) != null) 0 else 1
        }

    SectionHeader("Cast")
    Spacer(Modifier.height(8.dp))
    LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        items(sortedCastIndices.size) { i ->
            val member = details?.cast(sortedCastIndices[i])
            if (member != null) {
                CastMemberCard(
                    name = member.name,
                    character = member.character,
                    photoUrl = castPhotoUrl(member),
                )
            }
        }
    }
}

@Composable
internal fun CastMemberCard(
    name: String,
    character: String?,
    photoUrl: String?,
) {
    Column(
        modifier = Modifier.width(80.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            modifier = Modifier
                .size(60.dp)
                .clip(RoundedCornerShape(30.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant),
            contentAlignment = Alignment.Center,
        ) {
            if (photoUrl != null) {
                AsyncImage(
                    model = photoUrl,
                    contentDescription = name,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize(),
                )
            } else {
                Text(
                    text = name.take(1).uppercase(),
                    style = MaterialTheme.typography.titleMedium,
                )
            }
        }
        Spacer(Modifier.height(4.dp))
        Text(
            text = name,
            style = MaterialTheme.typography.labelSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        character?.let {
            Text(
                text = it,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun TechnicalMetadataSection(movie: MovieReference) {
    val file = movie.file ?: return
    val metadata = file.metadata

    val items = buildList {
        metadata?.let { meta ->
            val width = meta.width.toInt()
            val height = meta.height.toInt()
            if (width > 0 && height > 0) {
                val label = when {
                    height >= 2160 || width >= 3840 -> "4K"
                    height >= 1080 || width >= 1920 -> "1080p"
                    height >= 720 || width >= 1280 -> "720p"
                    height >= 480 -> "480p"
                    else -> "${width}×${height}"
                }
                add("Resolution" to "${width}×${height} ($label)")
            }

            meta.videoCodec?.let { if (it.isNotBlank()) add("Video" to it) }
            meta.audioCodec?.let { if (it.isNotBlank()) add("Audio" to it) }
        }

        file.path?.let { path ->
            val ext = path.substringAfterLast('.', "").uppercase()
            if (ext.isNotBlank()) add("Container" to ext)
        }

        val sizeBytes = file.size.toLong()
        if (sizeBytes > 0) {
            add("Size" to formatFileSize(sizeBytes))
        }
    }

    if (items.isEmpty()) return

    SectionHeader("Technical Info")
    Spacer(Modifier.height(4.dp))
    items.forEach { (label, value) ->
        Row(
            modifier = Modifier.padding(vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.width(80.dp),
            )
            Text(
                text = value,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

// ── Shared composables ──────────────────────────────────────────────

@Composable
internal fun SectionHeader(text: String) {
    Text(
        text = text,
        style = MaterialTheme.typography.titleSmall,
        fontWeight = FontWeight.Bold,
    )
}

@Composable
internal fun MetadataChip(text: String) {
    Text(
        text = text,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier
            .clip(RoundedCornerShape(50.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.72f))
            .padding(horizontal = 10.dp, vertical = 5.dp),
    )
}

// ── Formatting helpers ──────────────────────────────────────────────

internal fun formatRuntime(minutes: Int): String {
    val h = minutes / 60
    val m = minutes % 60
    return if (h > 0) "${h}h ${m}m" else "${m}m"
}

internal fun formatTime(seconds: Double): String {
    val totalSecs = seconds.toInt()
    val h = totalSecs / 3600
    val m = (totalSecs % 3600) / 60
    val s = totalSecs % 60
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
}

internal fun formatFileSize(bytes: Long): String {
    return when {
        bytes >= 1_073_741_824 -> "%.1f GB".format(bytes / 1_073_741_824.0)
        bytes >= 1_048_576 -> "%.0f MB".format(bytes / 1_048_576.0)
        bytes >= 1024 -> "%.0f KB".format(bytes / 1024.0)
        else -> "$bytes B"
    }
}

private enum class MovieWatchToggleAction {
    MarkWatched,
    MarkUnwatched,
}
