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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import com.ferrex.android.ui.components.ErrorScreen
import com.ferrex.android.ui.components.LoadingScreen
import ferrex.media.MovieReference

/**
 * Movie detail screen — backdrop, metadata, cast, play button.
 *
 * Data comes from the locally cached batch data (zero-copy FlatBuffers),
 * so this screen loads instantly without a network call.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MovieDetailScreen(
    viewModel: DetailViewModel,
    onBack: () -> Unit,
    onPlay: (mediaId: String) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsState()

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
                backdropUrl = viewModel.backdropUrl(state.movie),
                posterUrl = viewModel.posterUrl(state.movie),
                castPhotoUrl = { member -> viewModel.castPhotoUrl(member) },
                onBack = onBack,
                onPlay = {
                    // Use media file ID for streaming, not movie ID
                    val url = viewModel.streamUrl(state.movie)
                    if (url != null) {
                        val fileId = state.movie.file?.id?.toUuidString()
                        if (fileId != null) onPlay(fileId)
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
    backdropUrl: String?,
    posterUrl: String?,
    castPhotoUrl: (ferrex.details.CastMember) -> String?,
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

            // Content
            Column(
                modifier = Modifier.padding(horizontal = 16.dp),
            ) {
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
                ) {
                    details?.releaseDate?.take(4)?.let { year ->
                        Text(
                            text = year,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    details?.let { d ->
                        if (d.runtime > 0u) {
                            Text(
                                text = "${d.runtime}min",
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

                // Play button
                Button(
                    onClick = onPlay,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Default.PlayArrow, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("Play")
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
                        text = "Genres",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        text = (0 until genreCount)
                            .mapNotNull { details?.genres(it)?.name }
                            .joinToString(", "),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(16.dp))
                }

                // Cast
                val castCount = details?.castLength ?: 0
                if (castCount > 0) {
                    // Sort cast: members with photos first, then those without,
                    // preserving original order within each group.
                    val sortedCastIndices = (0 until castCount.coerceAtMost(20))
                        .sortedBy { i ->
                            val m = details?.cast(i)
                            if (m != null && castPhotoUrl(m) != null) 0 else 1
                        }

                    Text(
                        text = "Cast",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                    )
                    Spacer(Modifier.height(8.dp))
                    LazyRow(
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        items(sortedCastIndices.size) { i ->
                            val member = details?.cast(sortedCastIndices[i])
                            if (member != null) {
                                val photoUrl = castPhotoUrl(member)
                                Column(
                                    modifier = Modifier.width(80.dp),
                                    horizontalAlignment = Alignment.CenterHorizontally,
                                ) {
                                    Box(
                                        modifier = Modifier
                                            .width(60.dp)
                                            .height(60.dp)
                                            .clip(RoundedCornerShape(30.dp))
                                            .background(MaterialTheme.colorScheme.surfaceVariant),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        if (photoUrl != null) {
                                            AsyncImage(
                                                model = photoUrl,
                                                contentDescription = member.name,
                                                contentScale = ContentScale.Crop,
                                                modifier = Modifier.fillMaxSize(),
                                            )
                                        } else {
                                            // Fallback initial when no photo available
                                            Text(
                                                text = member.name.take(1).uppercase(),
                                                style = MaterialTheme.typography.titleMedium,
                                            )
                                        }
                                    }
                                    Spacer(Modifier.height(4.dp))
                                    Text(
                                        text = member.name,
                                        style = MaterialTheme.typography.labelSmall,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    member.character?.let { character ->
                                        Text(
                                            text = character,
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                }
                            }
                        }
                    }
                    Spacer(Modifier.height(16.dp))
                }

                // Bottom spacing
                Spacer(Modifier.height(32.dp))
            }
        }
    }
}
