package com.ferrex.android.ui.detail

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.text.font.FontWeight
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
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.image.TmdbImageFallbackPolicy
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.watch.WatchEpisodeState
import com.ferrex.android.core.watch.WatchMediaProgress
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.ui.components.FerrexActionButton
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexAsyncImage
import com.ferrex.android.ui.components.FerrexImageFallback
import com.ferrex.android.ui.components.FerrexStatusAction
import com.ferrex.android.ui.components.FerrexStatusCard
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.colors
import com.ferrex.android.ui.theme.FerrexDesignTokens

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
    onOpenDiagnostics: () -> Unit = {},
) {
    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(
                    horizontal = FerrexDesignTokens.Space.ScreenPhoneHorizontal,
                    vertical = FerrexDesignTokens.Space.ScreenPhoneVertical,
                ),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xl),
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
                        onRetryWatch = onRetryWatch,
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
                        onOpenDiagnostics = onOpenDiagnostics,
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
    val resumeContract = DetailRouteContracts.movieResume(movie, progress, route)
    val startOverContract = DetailRouteContracts.movieStartOver(movie, route)
    val primaryContract = resumeContract ?: startOverContract
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg)) {
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
        GenreRow(movie.genres)
        WatchStatusCard(
            title = "Movie watch state",
            progress = progress,
            watched = progress?.isCompleted == true,
            onRetryWatch = onRetryWatch,
        )
        NetworkActionStatus(networkActionsEnabled, networkActionMessage)
        DetailPlaybackActionRow(
            primaryLabel = when {
                resumeContract != null -> "Resume"
                progress?.isCompleted == true -> "Play again"
                else -> "Play"
            },
            primaryContract = primaryContract,
            secondaryLabel = "Start over",
            secondaryContract = startOverContract.takeIf { resumeContract != null },
            networkActionsEnabled = networkActionsEnabled,
            unavailableCopy = "Playback is unavailable because this movie does not have a playable file in the cache.",
            onPlaybackContract = onPlaybackContract,
        )
        WatchMutationButtons(
            watched = progress?.isCompleted == true,
            pending = progress?.pendingMutation == true,
            networkActionsEnabled = networkActionsEnabled,
            showClearProgress = progress?.hasServerState == true,
            onClearProgress = { onClearProgress(movie.id) },
            onSetWatched = { onMarkMovieWatched(movie.id, it) },
        )
        movie.fileName?.let {
            Text("Target file: $it", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
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
    val nextEpisode = seriesStatus?.nextEpisode ?: series.tmdbId?.let { watchState.nextEpisodes[it] }
    val nextContract = DetailRouteContracts.seriesNext(detail, nextEpisode, route)
    val startOverContract = DetailRouteContracts.seriesStartOver(detail, route)
    val primaryContract = nextContract ?: startOverContract
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg)) {
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
        GenreRow(series.genres)
        when (detail.episodesAvailability) {
            is EpisodesAvailability.Available -> StateCard(
                title = "Episodes ready",
                body = "${detail.episodesAvailability.episodeCount} cached episode(s) parsed from the current SeriesBundleData root.",
                tone = FerrexStatusTone.Cache,
            )
            is EpisodesAvailability.Unavailable -> StateCard(
                title = "Episodes unavailable",
                body = detail.episodesAvailability.message,
                action = detail.episodesAvailability.retryLabel to onRetryEpisodes,
                tone = FerrexStatusTone.Error,
            )
        }
        SeriesNextEpisodeCard(detail = detail, nextEpisode = nextEpisode)
        SeriesWatchCard(seriesStatus = seriesStatus, lastError = watchState.lastError, onRetryWatch = onRetryWatch)
        NetworkActionStatus(networkActionsEnabled, networkActionMessage)
        DetailPlaybackActionRow(
            primaryLabel = when {
                nextEpisode?.reason.equals("resume_in_progress", ignoreCase = true) -> "Resume next"
                nextEpisode != null -> "Play next"
                else -> "Play series"
            },
            primaryContract = primaryContract,
            secondaryLabel = "Start over",
            secondaryContract = startOverContract.takeIf { it != null && it != primaryContract },
            networkActionsEnabled = networkActionsEnabled,
            unavailableCopy = "Playback is unavailable because this series cache does not include a playable episode file.",
            onPlaybackContract = onPlaybackContract,
        )
        series.tmdbId?.let { tmdbId ->
            WatchMutationButtons(
                watched = seriesStatus?.isCompleted == true,
                pending = seriesStatus?.pendingMutation == true,
                networkActionsEnabled = networkActionsEnabled,
                onSetWatched = { onMarkSeriesWatched(tmdbId, it) },
            )
        }
        SeasonSummary(seasons = detail.seasons, seriesStatus = seriesStatus)
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
    val resumeContract = DetailRouteContracts.episodeResume(episode, progress, route)
    val startOverContract = DetailRouteContracts.episodeStartOver(episode, route)
    Card(
        shape = FerrexDesignTokens.Shapes.Card,
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, MaterialTheme.colorScheme.outline.copy(alpha = 0.45f)),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(FerrexDesignTokens.Space.Md),
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
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
                WatchStatePills(
                    watched = watched,
                    progressRatio = ratio,
                    pending = progress?.pendingMutation == true,
                    label = episode.title,
                )
                episode.overview?.let { Text(it, style = MaterialTheme.typography.bodySmall, maxLines = 3, overflow = TextOverflow.Ellipsis) }
                DeterminateProgressBar(progress = ratio, label = "${episode.title} watch progress")
                DetailPlaybackActionRow(
                    primaryLabel = if (resumeContract != null) "Resume" else "Play episode",
                    primaryContract = resumeContract ?: startOverContract,
                    secondaryLabel = "Start over",
                    secondaryContract = startOverContract.takeIf { resumeContract != null },
                    networkActionsEnabled = networkActionsEnabled,
                    unavailableCopy = "Playback is unavailable because this episode does not have a playable file in the cache.",
                    onPlaybackContract = onPlaybackContract,
                )
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
    onRetryWatch: () -> Unit,
    onClearProgress: (String) -> Unit,
    onMarkEpisodeWatched: (String, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    val resumeContract = DetailRouteContracts.episodeResume(episode, progress, route)
    val startOverContract = DetailRouteContracts.episodeStartOver(episode, route)
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg)) {
        DetailArtwork(episode.images, episode.title, imageResolutions, imageLoaderAvailable, imageLoader, scope)
        TitleBlock(
            title = episode.title,
            subtitle = parentTitle?.let { "$it • S${episode.seasonNumber} E${episode.episodeNumber}" },
            body = episode.overview,
        )
        FactRow(
            facts = listOfNotNull(
                episode.airDate?.take(4),
                episode.runtimeMinutes?.let { "$it min" },
                "Season ${episode.seasonNumber}",
                "Episode ${episode.episodeNumber}",
            ),
        )
        WatchStatusCard(
            title = "Episode watch state",
            progress = progress,
            watched = progress?.isCompleted == true,
            onRetryWatch = onRetryWatch,
        )
        NetworkActionStatus(networkActionsEnabled, networkActionMessage)
        DetailPlaybackActionRow(
            primaryLabel = if (resumeContract != null) "Resume" else "Play episode",
            primaryContract = resumeContract ?: startOverContract,
            secondaryLabel = "Start over",
            secondaryContract = startOverContract.takeIf { resumeContract != null },
            networkActionsEnabled = networkActionsEnabled,
            unavailableCopy = "Playback is unavailable because this episode does not have a playable file in the cache.",
            onPlaybackContract = onPlaybackContract,
        )
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
    onOpenDiagnostics: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        StateCard(title = missing.title, body = missing.message)
        FerrexActionButton(
            label = "Retry cache sync",
            role = FerrexActionRole.Retry,
            onClick = onRetryCacheSync,
            modifier = Modifier.fillMaxWidth(),
        )
        if (missing.selectedLibraryId != null) {
            FerrexActionButton(
                label = "Clear selected cache",
                role = FerrexActionRole.Cache,
                onClick = onClearSelectedCache,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            FerrexActionButton(
                label = "Change server",
                role = FerrexActionRole.Secondary,
                onClick = onChangeServer,
                modifier = Modifier.weight(1f),
            )
            FerrexActionButton(
                label = "Reset connection",
                role = FerrexActionRole.DestructiveReset,
                onClick = onResetConnection,
                modifier = Modifier.weight(1f),
            )
        }
        FerrexActionButton(
            label = "Diagnostics / Export diagnostics",
            role = FerrexActionRole.Secondary,
            onClick = onOpenDiagnostics,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun WatchStatusCard(
    title: String,
    progress: WatchMediaProgress?,
    watched: Boolean,
    onRetryWatch: () -> Unit,
) {
    val ratio = progress?.progressRatio ?: 0f
    val percent = (ratio * 100f).toInt()
    val tone = when {
        progress == null -> FerrexStatusTone.Retry
        watched -> FerrexStatusTone.Primary
        ratio > 0f -> FerrexStatusTone.Cache
        else -> FerrexStatusTone.Secondary
    }
    val body = when {
        progress == null -> "Watch state has not loaded yet. Retry keeps this detail page recoverable."
        watched -> "Completed. Start over or mark unwatched if this state is wrong."
        ratio > 0f -> "Resume from ${formatSeconds(progress.positionSeconds)} ($percent% watched) or start over."
        else -> "Unwatched. Play starts from the beginning."
    }
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        WatchStatePills(
            watched = watched,
            progressRatio = ratio,
            pending = progress?.pendingMutation == true,
            label = title,
        )
        StateCard(
            title = title,
            body = body,
            action = if (progress == null) "Retry watch state" to onRetryWatch else null,
            tone = tone,
        )
        DeterminateProgressBar(progress = ratio, label = title)
    }
}

@Composable
private fun SeriesNextEpisodeCard(
    detail: SeriesBundleDetail,
    nextEpisode: com.ferrex.android.core.watch.WatchNextEpisode?,
) {
    val body = nextEpisode?.let { next ->
        val episode = detail.episodes.firstOrNull { candidate ->
            candidate.seasonNumber == next.key.seasonNumber && candidate.episodeNumber == next.key.episodeNumber
        }
        val label = episode?.let { "S${it.seasonNumber} E${it.episodeNumber}: ${it.title}" }
            ?: "S${next.key.seasonNumber} E${next.key.episodeNumber}"
        val reason = when (next.reason) {
            "resume_in_progress" -> "Server says to resume the in-progress episode."
            "next_unwatched" -> "Server selected the next unwatched episode."
            else -> "Server reason: ${next.reason}."
        }
        "$label. $reason"
    } ?: detail.firstPlayableEpisode?.let {
        "No next-episode response yet; Play next falls back to ${it.episodeKey}: ${it.title}."
    } ?: "No playable episodes are cached yet. Retry episodes or cache sync to recover."
    StateCard(
        title = "Next episode",
        body = body,
        tone = if (nextEpisode != null) FerrexStatusTone.Primary else FerrexStatusTone.Cache,
    )
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
            tone = FerrexStatusTone.Retry,
        )
        return
    }
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        WatchStatePills(
            watched = seriesStatus.isCompleted,
            progressRatio = seriesStatus.progressRatio,
            pending = seriesStatus.pendingMutation,
            label = "Series watch state",
        )
        StateCard(
            title = "Series watch state",
            body = "${seriesStatus.watched} of ${seriesStatus.totalEpisodes} watched; ${seriesStatus.inProgress} in progress.",
            tone = if (seriesStatus.isCompleted) FerrexStatusTone.Primary else FerrexStatusTone.Secondary,
        )
        DeterminateProgressBar(progress = seriesStatus.progressRatio, label = "Series watch progress")
    }
}

@Composable
private fun NetworkActionStatus(
    networkActionsEnabled: Boolean,
    networkActionMessage: String?,
) {
    if (!networkActionsEnabled && networkActionMessage != null) {
        StateCard(
            title = "Playback and watch updates paused",
            body = "$networkActionMessage Actions stay visible but disabled until the connection recovers.",
            tone = FerrexStatusTone.StaleOffline,
        )
    }
}

@Composable
private fun DetailPlaybackActionRow(
    primaryLabel: String,
    primaryContract: PlaybackRouteContract?,
    secondaryLabel: String?,
    secondaryContract: PlaybackRouteContract?,
    networkActionsEnabled: Boolean,
    unavailableCopy: String,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
) {
    if (primaryContract == null) {
        StateCard(
            title = "Playback unavailable",
            body = unavailableCopy,
            tone = FerrexStatusTone.Error,
        )
        return
    }
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            FerrexActionButton(
                label = primaryLabel,
                role = FerrexActionRole.Primary,
                enabled = networkActionsEnabled,
                onClick = { onPlaybackContract(primaryContract) },
                modifier = Modifier.weight(1f),
            )
            if (secondaryLabel != null && secondaryContract != null) {
                FerrexActionButton(
                    label = secondaryLabel,
                    role = FerrexActionRole.Secondary,
                    enabled = networkActionsEnabled,
                    onClick = { onPlaybackContract(secondaryContract) },
                    modifier = Modifier.weight(1f),
                )
            }
        }
        if (!networkActionsEnabled) {
            StatusPill(
                label = "Actions disabled until reconnect",
                tone = FerrexStatusTone.StaleOffline,
                contentDescription = "Playback and watch actions disabled until the connection recovers",
            )
        }
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
    val enabled = !pending && networkActionsEnabled
    Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        FerrexActionButton(
            label = when {
                pending -> "Updating…"
                watched -> "Mark unwatched"
                else -> "Mark watched"
            },
            role = if (watched) FerrexActionRole.Secondary else FerrexActionRole.Primary,
            enabled = enabled,
            onClick = { onSetWatched(!watched) },
            modifier = Modifier.weight(1f),
        )
        if (showClearProgress && onClearProgress != null) {
            FerrexActionButton(
                label = "Clear progress",
                role = FerrexActionRole.Cache,
                enabled = enabled,
                onClick = onClearProgress,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun TitleBlock(title: String, subtitle: String?, body: String?) {
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        Text(
            text = title,
            style = MaterialTheme.typography.headlineMedium,
            color = MaterialTheme.colorScheme.primary,
            fontWeight = FontWeight.Bold,
        )
        subtitle?.let { Text(it, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.secondary) }
        body?.let { Text(it, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface) }
    }
}

@Composable
private fun FactRow(facts: List<String>) {
    if (facts.isEmpty()) return
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
    ) {
        facts.forEach { fact ->
            StatusPill(label = fact, tone = FerrexStatusTone.Secondary)
        }
    }
}

@Composable
private fun GenreRow(genres: List<String>) {
    if (genres.isEmpty()) return
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
    ) {
        genres.take(8).forEach { genre ->
            StatusPill(label = genre, tone = FerrexStatusTone.Cache)
        }
    }
}

@Composable
private fun WatchStatePills(
    watched: Boolean,
    progressRatio: Float,
    pending: Boolean,
    label: String,
) {
    val percent = (progressRatio.coerceIn(0f, 1f) * 100f).toInt()
    val stateLabel = when {
        watched -> "Watched"
        progressRatio > 0f -> "In progress $percent%"
        else -> "Unwatched"
    }
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
    ) {
        StatusPill(
            label = stateLabel,
            tone = when {
                watched -> FerrexStatusTone.Primary
                progressRatio > 0f -> FerrexStatusTone.Cache
                else -> FerrexStatusTone.Secondary
            },
            contentDescription = "$label $stateLabel",
        )
        if (pending) {
            StatusPill(
                label = "Syncing",
                tone = FerrexStatusTone.Retry,
                contentDescription = "$label watch-state update pending",
            )
        }
    }
}

@Composable
private fun StatusPill(
    label: String,
    tone: FerrexStatusTone,
    modifier: Modifier = Modifier,
    contentDescription: String = label,
) {
    val colors = tone.colors()
    Surface(
        modifier = modifier.semantics { this.contentDescription = contentDescription },
        shape = FerrexDesignTokens.Shapes.Pill,
        color = colors.container,
        contentColor = colors.content,
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, colors.border.copy(alpha = 0.72f)),
    ) {
        Text(
            modifier = Modifier.padding(horizontal = FerrexDesignTokens.Space.Md, vertical = FerrexDesignTokens.Space.Xs),
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = colors.accent,
            fontWeight = FontWeight.SemiBold,
        )
    }
}

@Composable
private fun SeasonSummary(
    seasons: List<com.ferrex.android.core.detail.SeasonDetail>,
    seriesStatus: com.ferrex.android.core.watch.WatchSeriesStatus?,
) {
    if (seasons.isEmpty()) return
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        Text("Seasons", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
        seasons.forEach { season ->
            val seasonStatus = seriesStatus?.seasons?.get(season.seasonNumber)
            Card(
                modifier = Modifier.fillMaxWidth(),
                shape = FerrexDesignTokens.Shapes.Card,
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
            ) {
                Column(
                    modifier = Modifier.padding(FerrexDesignTokens.Space.Md),
                    verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
                ) {
                    Text(season.title, style = MaterialTheme.typography.titleSmall)
                    Text(
                        text = buildString {
                            append(season.episodeCount ?: seasonStatus?.total ?: 0)
                            append(" episode(s)")
                            season.airDate?.take(4)?.let { append(" • $it") }
                            season.runtimeMinutes?.let { append(" • $it min") }
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    seasonStatus?.let {
                        WatchStatePills(
                            watched = it.isCompleted,
                            progressRatio = if (it.total > 0) it.watched.toFloat() / it.total.toFloat() else 0f,
                            pending = false,
                            label = season.title,
                        )
                    }
                }
            }
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
    val heroKey = images.backdrop ?: images.still ?: images.poster
    val supportingArt = listOfNotNull(
        images.poster?.takeIf { it != heroKey }?.let { DetailArtworkSpec("Poster", it, images.posterFallbackPath) },
        images.still?.takeIf { it != heroKey }?.let { DetailArtworkSpec("Still", it, images.stillFallbackPath) },
        images.backdrop?.takeIf { it != heroKey }?.let { DetailArtworkSpec("Backdrop", it, images.backdropFallbackPath) },
    )
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = FerrexDesignTokens.Shapes.Card,
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, MaterialTheme.colorScheme.outline.copy(alpha = 0.45f)),
    ) {
        Column(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Sm),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        ) {
            DetailImage(
                key = heroKey,
                title = title,
                fallbackPath = images.fallbackPathFor(heroKey),
                imageResolutions = imageResolutions,
                imageLoaderAvailable = imageLoaderAvailable,
                imageLoader = imageLoader,
                scope = scope,
                modifier = Modifier.fillMaxWidth().clip(FerrexDesignTokens.Shapes.PosterImage),
            )
            if (supportingArt.isNotEmpty()) {
                Row(horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                    supportingArt.take(2).forEach { art ->
                        Column(
                            modifier = Modifier.width(112.dp),
                            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
                        ) {
                            DetailImage(
                                key = art.key,
                                title = "$title ${art.label}",
                                fallbackPath = art.fallbackPath,
                                imageResolutions = imageResolutions,
                                imageLoaderAvailable = imageLoaderAvailable,
                                imageLoader = imageLoader,
                                scope = scope,
                                modifier = Modifier.fillMaxWidth().clip(FerrexDesignTokens.Shapes.PosterImage),
                            )
                            Text(art.label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }
    }
}

private data class DetailArtworkSpec(
    val label: String,
    val key: ImageRequestKey,
    val fallbackPath: String?,
)

private fun DetailImageSet.fallbackPathFor(key: ImageRequestKey?): String? = when (key) {
    backdrop -> backdropFallbackPath
    poster -> posterFallbackPath
    still -> stillFallbackPath
    else -> null
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
    val fallback = if (resolution !is ImageResolution.Ready) {
        PosterOnlyIidFallback.url(scope.canonicalServerUrl, key)?.let { FerrexImageFallback(it, "Poster IID fallback") }
            ?: TmdbImageFallbackPolicy.publicCdnUrl(
                publicPath = fallbackPath,
                category = key.category,
                productCopyAllowsPublicCdn = false,
            )?.let { FerrexImageFallback(it, "TMDB fallback") }
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
}

private fun formatSeconds(seconds: Double): String {
    val total = seconds.toLong().coerceAtLeast(0L)
    val minutes = total / 60L
    val remainingSeconds = total % 60L
    return if (minutes >= 60L) {
        val hours = minutes / 60L
        val hourMinutes = minutes % 60L
        "${hours}h ${hourMinutes}m"
    } else {
        "${minutes}m ${remainingSeconds}s"
    }
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
    tone: FerrexStatusTone = FerrexStatusTone.Secondary,
) {
    FerrexStatusCard(
        title = title,
        body = body,
        tone = tone,
        action = action?.let { (label, callback) ->
            FerrexStatusAction(
                label = label,
                role = FerrexActionRole.Retry,
                enabled = actionEnabled,
                onClick = callback,
            )
        },
    )
}
