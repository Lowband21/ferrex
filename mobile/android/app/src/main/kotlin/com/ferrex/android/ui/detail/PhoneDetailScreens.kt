package com.ferrex.android.ui.detail

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil.ImageLoader
import com.ferrex.android.core.auth.AuthenticatedConnectionUi
import com.ferrex.android.core.detail.DetailImageSet
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.detail.DetailRouteContracts
import com.ferrex.android.core.detail.EpisodeDetail
import com.ferrex.android.core.detail.EpisodesAvailability
import com.ferrex.android.core.detail.MovieDetail
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.detail.SeriesBundleDetail
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.watch.WatchEpisodeState
import com.ferrex.android.core.watch.WatchMediaProgress
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.ui.components.FerrexAsyncImage
import com.ferrex.android.ui.components.FerrexImageFallback

@Composable
fun PhoneDetailScreen(
    detailResult: DetailLoadResult,
    watchState: WatchRepositoryState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    preparedPlaybackContract: PlaybackRouteContract?,
    connectionStatus: AuthenticatedConnectionUi,
    actionNotice: String?,
    onBack: () -> Unit,
    onRetryConnection: () -> Unit,
    onRetryCacheSync: () -> Unit,
    onClearSelectedCache: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onRetryWatch: () -> Unit,
    onRetryEpisodes: () -> Unit,
    onClearProgress: (String) -> Unit,
    onMarkMovieWatched: (String, Boolean) -> Unit,
    onMarkEpisodeWatched: (String, Boolean) -> Unit,
    onMarkSeriesWatched: (Long, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp, vertical = 32.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            item {
                DetailTopBar(
                    onBack = onBack,
                    preparedPlaybackContract = preparedPlaybackContract,
                    connectionStatus = connectionStatus,
                    actionNotice = actionNotice,
                    onRetryConnection = onRetryConnection,
                )
            }
            when (detailResult) {
                is DetailLoadResult.Movie -> item {
                    MovieDetailContent(
                        route = detailResult.route,
                        movie = detailResult.detail,
                        progress = watchState.mediaProgress(detailResult.detail.id),
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoaderAvailable,
                        imageLoader = imageLoader,
                        scope = scope,
                        networkActionsEnabled = connectionStatus.networkActionsEnabled,
                        networkActionMessage = connectionStatus.networkActionMessage,
                        onRetryWatch = onRetryWatch,
                        onClearProgress = onClearProgress,
                        onMarkMovieWatched = onMarkMovieWatched,
                        onPlaybackContract = onPlaybackContract,
                    )
                }
                is DetailLoadResult.Series -> {
                    item {
                        SeriesDetailContent(
                            route = detailResult.route,
                            detail = detailResult.detail,
                            watchState = watchState,
                            imageResolutions = imageResolutions,
                            imageLoaderAvailable = imageLoaderAvailable,
                            imageLoader = imageLoader,
                            scope = scope,
                            networkActionsEnabled = connectionStatus.networkActionsEnabled,
                            networkActionMessage = connectionStatus.networkActionMessage,
                            onRetryWatch = onRetryWatch,
                            onRetryEpisodes = onRetryEpisodes,
                            onMarkSeriesWatched = onMarkSeriesWatched,
                            onPlaybackContract = onPlaybackContract,
                        )
                    }
                    if (detailResult.detail.episodes.isNotEmpty()) {
                        items(detailResult.detail.episodes, key = { it.id }) { episode ->
                            EpisodeRow(
                                route = detailResult.route,
                                episode = episode,
                                progress = watchState.mediaProgress(episode.id),
                                statusProgress = detailResult.detail.series.tmdbId?.let { tmdbId ->
                                    watchState.seriesStatus(tmdbId)?.episodeStatus(episode.seasonNumber, episode.episodeNumber)
                                },
                                imageResolutions = imageResolutions,
                                imageLoaderAvailable = imageLoaderAvailable,
                                imageLoader = imageLoader,
                                scope = scope,
                                networkActionsEnabled = connectionStatus.networkActionsEnabled,
                                onClearProgress = onClearProgress,
                                onMarkEpisodeWatched = onMarkEpisodeWatched,
                                onPlaybackContract = onPlaybackContract,
                            )
                        }
                    }
                }
                is DetailLoadResult.Episode -> item {
                    EpisodeStandaloneContent(
                        route = detailResult.route,
                        episode = detailResult.detail,
                        parentTitle = detailResult.parentSeries?.title,
                        progress = watchState.mediaProgress(detailResult.detail.id),
                        imageResolutions = imageResolutions,
                        imageLoaderAvailable = imageLoaderAvailable,
                        imageLoader = imageLoader,
                        scope = scope,
                        networkActionsEnabled = connectionStatus.networkActionsEnabled,
                        networkActionMessage = connectionStatus.networkActionMessage,
                        onClearProgress = onClearProgress,
                        onMarkEpisodeWatched = onMarkEpisodeWatched,
                        onPlaybackContract = onPlaybackContract,
                    )
                }
                is DetailLoadResult.Missing -> item {
                    DetailRecoveryContent(
                        missing = detailResult,
                        onRetryCacheSync = onRetryCacheSync,
                        onClearSelectedCache = onClearSelectedCache,
                        onChangeServer = onChangeServer,
                        onResetConnection = onResetConnection,
                    )
                }
            }
        }
    }
}

@Composable
private fun DetailTopBar(
    onBack: () -> Unit,
    preparedPlaybackContract: PlaybackRouteContract?,
    connectionStatus: AuthenticatedConnectionUi,
    actionNotice: String?,
    onRetryConnection: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        TextButton(onClick = onBack) { Text("Back") }
        if (connectionStatus.visible) {
            StateCard(
                title = connectionStatus.title,
                body = connectionStatus.message,
                action = connectionStatus.retryLabel to onRetryConnection,
                actionEnabled = connectionStatus.retryEnabled,
            )
        }
        actionNotice?.let {
            StateCard(title = "Action unavailable", body = it)
        }
        preparedPlaybackContract?.let {
            StateCard(
                title = "Prepared playback contract",
                body = it.toDisplayString(),
            )
        }
    }
}

@Composable
private fun MovieDetailContent(
    route: com.ferrex.android.core.browse.MediaRouteArgs,
    movie: MovieDetail,
    progress: WatchMediaProgress?,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
    onRetryWatch: () -> Unit,
    onClearProgress: (String) -> Unit,
    onMarkMovieWatched: (String, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        DetailArtwork(movie.images, movie.title, imageResolutions, imageLoaderAvailable, imageLoader, scope)
        TitleBlock(title = movie.title, subtitle = movie.tagline, body = movie.overview)
        FactRow(
            facts = listOfNotNull(
                movie.releaseDate?.take(4),
                movie.runtimeMinutes?.let { "$it min" },
                movie.contentRating,
                movie.voteAverage?.let { "★ ${"%.1f".format(it)}" },
            ),
        )
        if (movie.genres.isNotEmpty()) Text(movie.genres.joinToString(" • "), style = MaterialTheme.typography.bodyMedium)
        WatchStatusCard(
            title = "Movie watch state",
            progress = progress,
            watched = progress?.isCompleted == true,
            onRetryWatch = onRetryWatch,
        )
        NetworkActionStatus(networkActionsEnabled, networkActionMessage)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            DetailRouteContracts.movieResume(movie, progress, route)?.let { contract ->
                Button(enabled = networkActionsEnabled, onClick = { onPlaybackContract(contract) }) { Text("Resume") }
            }
            DetailRouteContracts.movieStartOver(movie, route)?.let { contract ->
                OutlinedButton(enabled = networkActionsEnabled, onClick = { onPlaybackContract(contract) }) { Text("Start over") }
            }
        }
        WatchMutationButtons(
            watched = progress?.isCompleted == true,
            pending = progress?.pendingMutation == true,
            networkActionsEnabled = networkActionsEnabled,
            showClearProgress = progress?.hasServerState == true,
            onClearProgress = { onClearProgress(movie.id) },
            onSetWatched = { onMarkMovieWatched(movie.id, it) },
        )
        movie.fileName?.let { Text("Target file: $it", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
    }
}

@Composable
private fun SeriesDetailContent(
    route: com.ferrex.android.core.browse.MediaRouteArgs,
    detail: SeriesBundleDetail,
    watchState: WatchRepositoryState,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
    onRetryWatch: () -> Unit,
    onRetryEpisodes: () -> Unit,
    onMarkSeriesWatched: (Long, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    val series = detail.series
    val seriesStatus = watchState.seriesStatus(series.tmdbId)
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        DetailArtwork(series.images, series.title, imageResolutions, imageLoaderAvailable, imageLoader, scope)
        TitleBlock(title = series.title, subtitle = series.tagline, body = series.overview)
        FactRow(
            facts = listOfNotNull(
                series.firstAirDate?.take(4),
                series.availableSeasons?.let { "$it season(s)" } ?: series.numberOfSeasons?.let { "$it season(s)" },
                series.availableEpisodes?.let { "$it episode(s)" } ?: series.numberOfEpisodes?.let { "$it episode(s)" },
                series.contentRating,
                series.voteAverage?.let { "★ ${"%.1f".format(it)}" },
            ),
        )
        if (series.genres.isNotEmpty()) Text(series.genres.joinToString(" • "), style = MaterialTheme.typography.bodyMedium)
        when (detail.episodesAvailability) {
            is EpisodesAvailability.Available -> StateCard(
                title = "Episodes ready",
                body = "${detail.episodesAvailability.episodeCount} cached episode(s) parsed from the current SeriesBundleData root.",
            )
            is EpisodesAvailability.Unavailable -> StateCard(
                title = "Episodes unavailable",
                body = detail.episodesAvailability.message,
                action = detail.episodesAvailability.retryLabel to onRetryEpisodes,
            )
        }
        SeriesWatchCard(seriesStatus = seriesStatus, lastError = watchState.lastError, onRetryWatch = onRetryWatch)
        NetworkActionStatus(networkActionsEnabled, networkActionMessage)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            DetailRouteContracts.seriesNext(detail, seriesStatus?.nextEpisode ?: series.tmdbId?.let { watchState.nextEpisodes[it] }, route)?.let { contract ->
                Button(enabled = networkActionsEnabled, onClick = { onPlaybackContract(contract) }) { Text("Play next") }
            }
            DetailRouteContracts.seriesStartOver(detail, route)?.let { contract ->
                OutlinedButton(enabled = networkActionsEnabled, onClick = { onPlaybackContract(contract) }) { Text("Start over") }
            }
        }
        series.tmdbId?.let { tmdbId ->
            WatchMutationButtons(
                watched = seriesStatus?.isCompleted == true,
                pending = seriesStatus?.pendingMutation == true,
                networkActionsEnabled = networkActionsEnabled,
                onSetWatched = { onMarkSeriesWatched(tmdbId, it) },
            )
        }
        SeasonSummary(seasons = detail.seasons)
    }
}

@Composable
private fun EpisodeRow(
    route: com.ferrex.android.core.browse.MediaRouteArgs,
    episode: EpisodeDetail,
    progress: WatchMediaProgress?,
    statusProgress: com.ferrex.android.core.watch.WatchEpisodeStatus?,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    networkActionsEnabled: Boolean,
    onClearProgress: (String) -> Unit,
    onMarkEpisodeWatched: (String, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    val watched = progress?.isCompleted == true || statusProgress?.state == WatchEpisodeState.Completed
    val ratio = progress?.progressRatio?.takeIf { it > 0f } ?: statusProgress?.progress ?: 0f
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            DetailStill(episode.images, episode.title, imageResolutions, imageLoaderAvailable, imageLoader, scope)
            Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    WatchedIcon(watched = watched, label = episode.title)
                    Text(
                        text = "S${episode.seasonNumber} E${episode.episodeNumber}: ${episode.title}",
                        style = MaterialTheme.typography.titleSmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                episode.overview?.let { Text(it, style = MaterialTheme.typography.bodySmall, maxLines = 3, overflow = TextOverflow.Ellipsis) }
                DeterminateProgressBar(progress = ratio, label = "${episode.title} watch progress")
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    DetailRouteContracts.episodeResume(episode, progress, route)?.let { contract ->
                        Button(enabled = networkActionsEnabled, onClick = { onPlaybackContract(contract) }) { Text("Resume") }
                    }
                    DetailRouteContracts.episodeStartOver(episode, route)?.let { contract ->
                        OutlinedButton(enabled = networkActionsEnabled, onClick = { onPlaybackContract(contract) }) { Text("Start") }
                    }
                }
                WatchMutationButtons(
                    watched = watched,
                    pending = progress?.pendingMutation == true,
                    networkActionsEnabled = networkActionsEnabled,
                    showClearProgress = progress?.hasServerState == true,
                    onClearProgress = { onClearProgress(episode.id) },
                    onSetWatched = { onMarkEpisodeWatched(episode.id, it) },
                )
            }
        }
    }
}

@Composable
private fun EpisodeStandaloneContent(
    route: com.ferrex.android.core.browse.MediaRouteArgs,
    episode: EpisodeDetail,
    parentTitle: String?,
    progress: WatchMediaProgress?,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
    onClearProgress: (String) -> Unit,
    onMarkEpisodeWatched: (String, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        DetailArtwork(episode.images, episode.title, imageResolutions, imageLoaderAvailable, imageLoader, scope)
        TitleBlock(
            title = episode.title,
            subtitle = parentTitle?.let { "$it • S${episode.seasonNumber} E${episode.episodeNumber}" },
            body = episode.overview,
        )
        WatchStatusCard(
            title = "Episode watch state",
            progress = progress,
            watched = progress?.isCompleted == true,
            onRetryWatch = {},
        )
        NetworkActionStatus(networkActionsEnabled, networkActionMessage)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            DetailRouteContracts.episodeResume(episode, progress, route)?.let { contract ->
                Button(enabled = networkActionsEnabled, onClick = { onPlaybackContract(contract) }) { Text("Resume") }
            }
            DetailRouteContracts.episodeStartOver(episode, route)?.let { contract ->
                OutlinedButton(enabled = networkActionsEnabled, onClick = { onPlaybackContract(contract) }) { Text("Start over") }
            }
        }
        WatchMutationButtons(
            watched = progress?.isCompleted == true,
            pending = progress?.pendingMutation == true,
            networkActionsEnabled = networkActionsEnabled,
            showClearProgress = progress?.hasServerState == true,
            onClearProgress = { onClearProgress(episode.id) },
            onSetWatched = { onMarkEpisodeWatched(episode.id, it) },
        )
    }
}

@Composable
private fun DetailRecoveryContent(
    missing: DetailLoadResult.Missing,
    onRetryCacheSync: () -> Unit,
    onClearSelectedCache: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        StateCard(title = missing.title, body = missing.message)
        Button(onClick = onRetryCacheSync, modifier = Modifier.fillMaxWidth()) { Text("Retry cache sync") }
        if (missing.selectedLibraryId != null) {
            TextButton(onClick = onClearSelectedCache, modifier = Modifier.fillMaxWidth()) { Text("Clear selected cache") }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextButton(onClick = onChangeServer, modifier = Modifier.weight(1f)) { Text("Change server") }
            TextButton(onClick = onResetConnection, modifier = Modifier.weight(1f)) { Text("Reset connection") }
        }
    }
}

@Composable
private fun WatchStatusCard(
    title: String,
    progress: WatchMediaProgress?,
    watched: Boolean,
    onRetryWatch: () -> Unit,
) {
    val progressLabel = when {
        progress == null -> "Watch state has not loaded yet."
        watched -> "Watched"
        progress.isStarted -> "${(progress.progressRatio * 100).toInt()}% watched"
        else -> "Unwatched"
    }
    StateCard(
        title = title,
        body = progressLabel,
        action = if (progress == null) "Retry watch state" to onRetryWatch else null,
    )
    DeterminateProgressBar(progress = progress?.progressRatio ?: 0f, label = title)
}

@Composable
private fun SeriesWatchCard(
    seriesStatus: com.ferrex.android.core.watch.WatchSeriesStatus?,
    lastError: String?,
    onRetryWatch: () -> Unit,
) {
    if (seriesStatus == null) {
        StateCard(
            title = "Series watch state unavailable",
            body = lastError ?: "Retry to load /watch/series/{tmdb} and /watch/series/{tmdb}/next.",
            action = "Retry watch state" to onRetryWatch,
        )
        return
    }
    StateCard(
        title = "Series watch state",
        body = "${seriesStatus.watched} of ${seriesStatus.totalEpisodes} watched; ${seriesStatus.inProgress} in progress.",
    )
    DeterminateProgressBar(progress = seriesStatus.progressRatio, label = "Series watch progress")
}

@Composable
private fun NetworkActionStatus(
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
) {
    if (!networkActionsEnabled && networkActionMessage != null) {
        StateCard(
            title = "Playback and watch updates paused",
            body = networkActionMessage,
        )
    }
}

@Composable
private fun WatchMutationButtons(
    watched: Boolean,
    pending: Boolean,
    networkActionsEnabled: Boolean,
    showClearProgress: Boolean = false,
    onClearProgress: (() -> Unit)? = null,
    onSetWatched: (Boolean) -> Unit,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        val enabled = !pending && networkActionsEnabled
        if (watched) {
            OutlinedButton(enabled = enabled, onClick = { onSetWatched(false) }) { Text(if (pending) "Updating…" else "Mark unwatched") }
        } else {
            Button(enabled = enabled, onClick = { onSetWatched(true) }) { Text(if (pending) "Updating…" else "Mark watched") }
        }
        if (showClearProgress && onClearProgress != null) {
            TextButton(enabled = enabled, onClick = onClearProgress) { Text("Clear progress") }
        }
    }
}

@Composable
private fun TitleBlock(title: String, subtitle: String?, body: String?) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = MaterialTheme.typography.headlineMedium, color = MaterialTheme.colorScheme.primary)
        subtitle?.let { Text(it, style = MaterialTheme.typography.titleMedium) }
        body?.let { Text(it, style = MaterialTheme.typography.bodyLarge) }
    }
}

@Composable
private fun FactRow(facts: List<String>) {
    if (facts.isNotEmpty()) Text(facts.joinToString(" • "), style = MaterialTheme.typography.bodyMedium)
}

@Composable
private fun SeasonSummary(seasons: List<com.ferrex.android.core.detail.SeasonDetail>) {
    if (seasons.isEmpty()) return
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text("Seasons", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
        seasons.forEach { season ->
            Text(
                text = "${season.title}: ${season.episodeCount ?: 0} episode(s)",
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

@Composable
private fun DetailArtwork(
    images: DetailImageSet,
    title: String,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
) {
    val key = images.backdrop ?: images.poster ?: images.still
    DetailImage(
        key = key,
        title = title,
        fallbackPath = when (key) {
            images.backdrop -> images.backdropFallbackPath
            images.poster -> images.posterFallbackPath
            images.still -> images.stillFallbackPath
            else -> null
        },
        imageResolutions = imageResolutions,
        imageLoaderAvailable = imageLoaderAvailable,
        imageLoader = imageLoader,
        scope = scope,
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun DetailStill(
    images: DetailImageSet,
    title: String,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
) {
    DetailImage(
        key = images.still ?: images.poster,
        title = title,
        fallbackPath = images.stillFallbackPath ?: images.posterFallbackPath,
        imageResolutions = imageResolutions,
        imageLoaderAvailable = imageLoaderAvailable,
        imageLoader = imageLoader,
        scope = scope,
        modifier = Modifier.width(128.dp),
    )
}

@Composable
private fun DetailImage(
    key: ImageRequestKey?,
    title: String,
    fallbackPath: String?,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoaderAvailable: Boolean,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    modifier: Modifier,
) {
    if (key == null || !imageLoaderAvailable || imageLoader == null) {
        Box(
            modifier = modifier
                .height(180.dp)
                .background(MaterialTheme.colorScheme.surfaceVariant),
            contentAlignment = Alignment.Center,
        ) {
            Text(if (key == null) "No image" else "Images unavailable", style = MaterialTheme.typography.bodySmall)
        }
        return
    }
    val resolution = imageResolutions[key]
    val fallback = if (resolution is ImageResolution.Pending || resolution is ImageResolution.Failed) {
        PosterOnlyIidFallback.url(scope.canonicalServerUrl, key)?.let { FerrexImageFallback(it, "Poster IID fallback") }
    } else {
        null
    }
    FerrexAsyncImage(
        resolution = resolution,
        imageLoader = imageLoader,
        contentDescription = title,
        modifier = modifier,
        category = key.category,
        fallback = fallback,
    )
    @Suppress("UNUSED_EXPRESSION")
    fallbackPath
}

@Composable
private fun DeterminateProgressBar(progress: Float, label: String) {
    val coerced = progress.coerceIn(0f, 1f)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(8.dp)
            .clip(MaterialTheme.shapes.small)
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .semantics {
                contentDescription = label
                progressBarRangeInfo = ProgressBarRangeInfo(coerced, 0f..1f)
            },
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth(coerced)
                .height(8.dp)
                .background(MaterialTheme.colorScheme.primary),
        )
    }
}

@Composable
private fun WatchedIcon(watched: Boolean, label: String) {
    Text(
        modifier = Modifier
            .size(24.dp)
            .semantics { contentDescription = if (watched) "$label watched" else "$label unwatched" },
        text = if (watched) "✓" else "○",
        style = MaterialTheme.typography.titleMedium,
        color = if (watched) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun StateCard(
    title: String,
    body: String,
    action: Pair<String, () -> Unit>? = null,
    actionEnabled: Boolean = true,
) {
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
        Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(title, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
            Text(body, style = MaterialTheme.typography.bodyMedium)
            action?.let { (label, callback) -> Button(enabled = actionEnabled, onClick = callback) { Text(label) } }
        }
    }
}
