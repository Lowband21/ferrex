package com.ferrex.android.ui.detail

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import coil.ImageLoader
import com.ferrex.android.core.auth.AuthenticatedConnectionUi
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.detail.DetailPageAction
import com.ferrex.android.core.detail.DetailPageActionKind
import com.ferrex.android.core.detail.DetailPageKind
import com.ferrex.android.core.detail.DetailPageMapper
import com.ferrex.android.core.detail.DetailPageModel
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.detail.DetailRail
import com.ferrex.android.core.detail.DetailRailItem
import com.ferrex.android.core.detail.EpisodesAvailability
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.ui.components.FerrexActionButton
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theaterplate.FerrexStageSurface
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceTone
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceVariant
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
    libraryFreshness: LibraryFreshness? = null,
    onOpenDetail: (MediaRouteArgs) -> Unit = {},
) {
    val surfacedFreshness = remember(libraryFreshness) {
        libraryFreshness?.takeUnless { it is LibraryFreshness.Fresh }
    }
    val page = remember(
        detailResult,
        watchState,
        imageResolutions,
        surfacedFreshness,
        connectionStatus.networkActionsEnabled,
        connectionStatus.networkActionMessage,
    ) {
        DetailPageMapper.toPage(
            result = detailResult,
            watchState = watchState,
            libraryFreshness = surfacedFreshness,
            imageResolutions = imageResolutions,
            networkActionsEnabled = connectionStatus.networkActionsEnabled,
            networkActionMessage = connectionStatus.networkActionMessage,
        )
    }
    val effectiveImageLoader = imageLoader.takeIf { imageLoaderAvailable }
    val episodeUnavailable = detailResult.episodeUnavailableNotice()

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = FerrexDesignTokens.Palette.SlateCanvas,
    ) {
        BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
            val interactionMode = remember(maxWidth, maxHeight) {
                if (maxWidth > maxHeight || maxWidth >= 700.dp) {
                    DetailSurfaceInteractionMode.PhoneLandscapeTouch
                } else {
                    DetailSurfaceInteractionMode.PhoneTouch
                }
            }
            val callbacks = DetailPrimitiveCallbacks(
                onAction = { action ->
                    action.dispatchPhoneDetailAction(
                        page = page,
                        onBack = onBack,
                        onRetryCacheSync = onRetryCacheSync,
                        onClearSelectedCache = onClearSelectedCache,
                        onChangeServer = onChangeServer,
                        onResetConnection = onResetConnection,
                        onRetryWatch = onRetryWatch,
                        onClearProgress = onClearProgress,
                        onMarkMovieWatched = onMarkMovieWatched,
                        onMarkEpisodeWatched = onMarkEpisodeWatched,
                        onMarkSeriesWatched = onMarkSeriesWatched,
                        onPlaybackContract = onPlaybackContract,
                        onOpenDiagnostics = onOpenDiagnostics,
                    )
                },
                onPlaybackContract = onPlaybackContract,
                onRailItemActivated = { rail, item ->
                    handleRailActivation(
                        rail = rail,
                        item = item,
                        onPlaybackContract = onPlaybackContract,
                        onOpenDetail = onOpenDetail,
                    )
                },
            )

            FerrexDetailStage(
                page = page,
                imageResolutions = imageResolutions,
                imageLoader = effectiveImageLoader,
                serverUrl = scope.canonicalServerUrl,
                interactionMode = interactionMode,
                callbacks = callbacks,
                header = {
                    PhoneDetailChrome(
                        preparedPlaybackContract = preparedPlaybackContract,
                        connectionStatus = connectionStatus,
                        actionNotice = actionNotice,
                        episodeUnavailable = episodeUnavailable,
                        interactionMode = interactionMode,
                        onBack = onBack,
                        onRetryConnection = onRetryConnection,
                        onRetryEpisodes = onRetryEpisodes,
                    )
                },
            )
        }
    }
}

@Composable
private fun PhoneDetailChrome(
    preparedPlaybackContract: PlaybackRouteContract?,
    connectionStatus: AuthenticatedConnectionUi,
    actionNotice: String?,
    episodeUnavailable: EpisodeUnavailableNotice?,
    interactionMode: DetailSurfaceInteractionMode,
    onBack: () -> Unit,
    onRetryConnection: () -> Unit,
    onRetryEpisodes: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        FerrexStageSurface(
            variant = FerrexStageSurfaceVariant.ControlShelf,
            density = interactionMode.density,
            tone = FerrexStageSurfaceTone.Primary,
            modifier = Modifier.fillMaxWidth(),
            contentDescription = "Phone detail navigation actions",
            testTag = FerrexQaTags.namespaced("phone", "detail", "chrome"),
        ) {
            Row(
                modifier = Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                FerrexActionButton(
                    label = "Back",
                    role = FerrexActionRole.Secondary,
                    onClick = onBack,
                    modifier = Modifier.widthIn(min = interactionMode.actionMinWidth),
                    testTag = FerrexQaTags.namespaced("phone", "detail", "action", "back"),
                    contentDescription = "Back to the previous phone screen",
                )
            }
        }
        if (connectionStatus.visible) {
            PhoneDetailNotice(
                title = connectionStatus.title,
                body = connectionStatus.message,
                tone = FerrexStageSurfaceTone.StaleOffline,
                actionLabel = connectionStatus.retryLabel,
                actionEnabled = connectionStatus.retryEnabled,
                onAction = onRetryConnection,
                interactionMode = interactionMode,
                tagKey = "connection",
            )
        }
        episodeUnavailable?.let { notice ->
            PhoneDetailNotice(
                title = "Episodes unavailable",
                body = notice.message,
                tone = FerrexStageSurfaceTone.Warning,
                actionLabel = notice.retryLabel,
                actionEnabled = true,
                onAction = onRetryEpisodes,
                interactionMode = interactionMode,
                tagKey = "episodes-unavailable",
            )
        }
        actionNotice?.let { notice ->
            PhoneDetailNotice(
                title = "Action unavailable",
                body = notice,
                tone = FerrexStageSurfaceTone.Warning,
                interactionMode = interactionMode,
                tagKey = "action-unavailable",
            )
        }
        preparedPlaybackContract?.let { contract ->
            PhoneDetailNotice(
                title = "Prepared playback contract",
                body = contract.toDisplayString(),
                tone = FerrexStageSurfaceTone.Cache,
                interactionMode = interactionMode,
                tagKey = "prepared-playback",
            )
        }
    }
}

@Composable
private fun PhoneDetailNotice(
    title: String,
    body: String,
    tone: FerrexStageSurfaceTone,
    interactionMode: DetailSurfaceInteractionMode,
    tagKey: String,
    actionLabel: String? = null,
    actionEnabled: Boolean = true,
    onAction: (() -> Unit)? = null,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.StatusSlab,
        density = interactionMode.density,
        tone = tone,
        modifier = Modifier.fillMaxWidth(),
        contentDescription = "$title. $body",
        testTag = FerrexQaTags.namespaced("phone", "detail", "notice", tagKey),
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            TheaterPlateText(
                text = title,
                role = TheaterPlateTypographyRole.StatusTitle,
                densityRole = interactionMode.phoneTypographyDensity(),
            )
            TheaterPlateText(
                text = body,
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = interactionMode.phoneTypographyDensity(),
            )
            if (actionLabel != null && onAction != null) {
                FerrexActionButton(
                    label = actionLabel,
                    role = FerrexActionRole.Retry,
                    enabled = actionEnabled,
                    onClick = onAction,
                    modifier = Modifier.fillMaxWidth(),
                    contentDescription = actionLabel,
                )
            }
        }
    }
}

private fun DetailPageAction.dispatchPhoneDetailAction(
    page: DetailPageModel,
    onBack: () -> Unit,
    onRetryCacheSync: () -> Unit,
    onClearSelectedCache: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onRetryWatch: () -> Unit,
    onClearProgress: (String) -> Unit,
    onMarkMovieWatched: (String, Boolean) -> Unit,
    onMarkEpisodeWatched: (String, Boolean) -> Unit,
    onMarkSeriesWatched: (Long, Boolean) -> Unit,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    playbackContract?.let {
        onPlaybackContract(it)
        return
    }
    when (kind) {
        DetailPageActionKind.Back -> onBack()
        DetailPageActionKind.RetryCache -> onRetryCacheSync()
        DetailPageActionKind.ClearSelectedCache -> onClearSelectedCache()
        DetailPageActionKind.ChangeServer -> onChangeServer()
        DetailPageActionKind.ResetConnection -> onResetConnection()
        DetailPageActionKind.Diagnostics -> onOpenDiagnostics()
        DetailPageActionKind.RetryWatch -> onRetryWatch()
        DetailPageActionKind.ClearProgress -> targetId?.let(onClearProgress)
        DetailPageActionKind.MarkWatched,
        DetailPageActionKind.MarkUnwatched -> dispatchWatchMutation(
            page = page,
            onMarkMovieWatched = onMarkMovieWatched,
            onMarkEpisodeWatched = onMarkEpisodeWatched,
            onMarkSeriesWatched = onMarkSeriesWatched,
        )
        DetailPageActionKind.Resume,
        DetailPageActionKind.Play,
        DetailPageActionKind.StartOver -> Unit
    }
}

private fun DetailPageAction.dispatchWatchMutation(
    page: DetailPageModel,
    onMarkMovieWatched: (String, Boolean) -> Unit,
    onMarkEpisodeWatched: (String, Boolean) -> Unit,
    onMarkSeriesWatched: (Long, Boolean) -> Unit,
) {
    val id = targetId ?: return
    val watched = targetWatched ?: (kind == DetailPageActionKind.MarkWatched)
    when (page.kind) {
        DetailPageKind.Movie -> onMarkMovieWatched(id, watched)
        DetailPageKind.Episode -> onMarkEpisodeWatched(id, watched)
        DetailPageKind.Series,
        DetailPageKind.Season -> id.toLongOrNull()?.let { onMarkSeriesWatched(it, watched) }
        DetailPageKind.MissingDetail -> Unit
    }
}

private fun handleRailActivation(
    rail: DetailRail,
    item: DetailRailItem,
    onPlaybackContract: (PlaybackRouteContract) -> Unit,
    onOpenDetail: (MediaRouteArgs) -> Unit,
) {
    item.playbackContract?.let {
        onPlaybackContract(it)
        return
    }
    if (rail.isAvailable) {
        item.route?.let(onOpenDetail)
    }
}

private data class EpisodeUnavailableNotice(
    val message: String,
    val retryLabel: String,
)

private fun DetailLoadResult.episodeUnavailableNotice(): EpisodeUnavailableNotice? = when (this) {
    is DetailLoadResult.Series -> (detail.episodesAvailability as? EpisodesAvailability.Unavailable)?.let {
        EpisodeUnavailableNotice(message = it.message, retryLabel = it.retryLabel)
    }
    is DetailLoadResult.Season -> if (episodes.isEmpty()) {
        EpisodeUnavailableNotice(
            message = "No episodes are cached for this season. Retry episodes to refresh the current series bundle.",
            retryLabel = "Retry episodes",
        )
    } else {
        null
    }
    is DetailLoadResult.Movie,
    is DetailLoadResult.Episode,
    is DetailLoadResult.Missing -> null
}

private fun DetailSurfaceInteractionMode.phoneTypographyDensity(): com.ferrex.android.ui.components.TheaterPlateDensityRole = when (this) {
    DetailSurfaceInteractionMode.PhoneTouch -> com.ferrex.android.ui.components.TheaterPlateDensityRole.PhonePortrait
    DetailSurfaceInteractionMode.PhoneLandscapeTouch -> com.ferrex.android.ui.components.TheaterPlateDensityRole.PhoneLandscape
    DetailSurfaceInteractionMode.TvDpad -> com.ferrex.android.ui.components.TheaterPlateDensityRole.Tv1080p
}
