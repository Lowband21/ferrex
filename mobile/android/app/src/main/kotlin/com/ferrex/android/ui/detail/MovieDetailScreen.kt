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
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
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
import com.ferrex.android.core.watch.WatchProgress
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import ferrex.details.CastMember
import ferrex.media.MovieReference

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
    onPlay: (mediaId: String) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsState()
    val watchProgress by viewModel.watchProgress.collectAsState()

    when (val state = uiState) {
        is DetailUiState.Loading -> LoadingScreen()
        is DetailUiState.Error -> ErrorScreen(
            message = state.message,
            onRetry = onBack,
        )
        is DetailUiState.SeriesDetail -> ErrorScreen(message = "Expected movie, got series")
        is DetailUiState.MovieDetail -> {
            MovieDetailContent(
                movie = state.movie,
                watchProgress = watchProgress,
                backdropUrl = viewModel.backdropUrl(state.movie),
                posterUrl = viewModel.posterUrl(state.movie),
                castPhotoUrl = { member -> viewModel.castPhotoUrl(member) },
                onBack = onBack,
                onPlay = {
                    val fileId = state.movie.file?.id?.toUuidString()
                    if (fileId != null) onPlay(fileId)
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
    onBack: () -> Unit,
    onPlay: () -> Unit,
) {
    val details = movie.details
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
            // Backdrop image
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .aspectRatio(16f / 9f),
            ) {
                if (backdropUrl != null) {
                    AsyncImage(
                        model = backdropUrl,
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
                // Gradient overlay
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
                    text = movie.title,
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.Bold,
                )

                Spacer(Modifier.height(4.dp))

                // Metadata row: year, runtime, rating
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

                // Watch status + play button
                WatchStatusSection(
                    watchProgress = watchProgress,
                    onPlay = onPlay,
                )

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

                // Cast
                val castCount = details?.castLength ?: 0
                if (castCount > 0) {
                    CastSection(
                        details = details,
                        castCount = castCount.coerceAtMost(20),
                        castPhotoUrl = castPhotoUrl,
                    )
                    Spacer(Modifier.height(16.dp))
                }

                // Technical metadata
                TechnicalMetadataSection(movie = movie)

                // Bottom spacing
                Spacer(Modifier.height(32.dp))
            }
        }
    }
}

@Composable
private fun WatchStatusSection(
    watchProgress: WatchProgress?,
    onPlay: () -> Unit,
) {
    Column {
        if (watchProgress != null) {
            if (watchProgress.completed) {
                // Completed indicator
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.padding(bottom = 8.dp),
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
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            } else if (watchProgress.progress > 0f) {
                // In-progress indicator
                Column(modifier = Modifier.padding(bottom = 8.dp)) {
                    Text(
                        text = "${formatTime(watchProgress.position)} / ${formatTime(watchProgress.duration)}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(4.dp))
                    LinearProgressIndicator(
                        progress = { watchProgress.progress },
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(3.dp)
                            .clip(RoundedCornerShape(2.dp)),
                        trackColor = MaterialTheme.colorScheme.surfaceVariant,
                    )
                }
            }
        }

        Button(
            onClick = onPlay,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Icon(Icons.Default.PlayArrow, contentDescription = null)
            Spacer(Modifier.width(8.dp))
            Text(
                if (watchProgress?.progress?.let { it > 0f && !watchProgress.completed } == true)
                    "Resume"
                else
                    "Play"
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
    // Sort cast: members with photos first, preserving original order within each group
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
            // Video resolution
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

            // Video codec
            meta.videoCodec?.let { if (it.isNotBlank()) add("Video" to it) }

            // Audio codec
            meta.audioCodec?.let { if (it.isNotBlank()) add("Audio" to it) }
        }

        // Container format from filename
        file.path?.let { path ->
            val ext = path.substringAfterLast('.', "").uppercase()
            if (ext.isNotBlank()) add("Container" to ext)
        }

        // File size
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
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
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
