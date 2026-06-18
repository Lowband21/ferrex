package com.ferrex.android.tv.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import coil.ImageLoader
import com.ferrex.android.core.auth.AuthenticatedConnectionUi
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.detail.DetailActionRole
import com.ferrex.android.core.detail.DetailPageAction
import com.ferrex.android.core.detail.DetailPageActionKind
import com.ferrex.android.core.detail.DetailPageKind
import com.ferrex.android.core.detail.DetailPageMapper
import com.ferrex.android.core.detail.DetailPageModel
import com.ferrex.android.core.detail.DetailRail
import com.ferrex.android.core.detail.DetailRailActivationPolicy
import com.ferrex.android.core.detail.DetailRailItem
import com.ferrex.android.core.detail.DetailRailState
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.tvfocus.TvDetailFocusPolicy
import com.ferrex.android.core.watch.WatchRepositoryState
import com.ferrex.android.tv.ui.foundation.TvFocusableButton
import com.ferrex.android.tv.ui.foundation.TvFocusableStyle
import com.ferrex.android.tv.ui.foundation.TvFocusRestorer
import com.ferrex.android.tv.ui.foundation.TvScaffold
import com.ferrex.android.tv.ui.foundation.rememberTvFocusRestorer
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.components.TheaterPlateDensityRole
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.detail.DetailActionPresentation
import com.ferrex.android.ui.detail.DetailPrimitivePresenter
import com.ferrex.android.ui.detail.DetailRailItemPresentation
import com.ferrex.android.ui.detail.DetailRailPresentation
import com.ferrex.android.ui.detail.DetailSlabPresentation
import com.ferrex.android.ui.detail.DetailSurfaceInteractionMode
import com.ferrex.android.ui.detail.FerrexDetailHero
import com.ferrex.android.ui.detail.FerrexDetailMediaObject
import com.ferrex.android.ui.detail.FerrexDetailMetadataBand
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theaterplate.FerrexStageSurface
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceTone
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceVariant
import com.ferrex.android.ui.theme.FerrexDesignTokens
import com.ferrex.android.ui.theme.TvFocusTreatmentRole

@Composable
fun TvMediaDetailScreen(
    detailResult: DetailLoadResult?,
    watchState: WatchRepositoryState,
    libraryFreshness: LibraryFreshness,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    playbackNotice: String?,
    connectionStatus: AuthenticatedConnectionUi,
    onBack: () -> Unit,
    onRetryConnection: () -> Unit,
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
    onOpenDetail: (MediaRouteArgs) -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    BackHandler(onBack = onBack)

    val page = remember(
        detailResult,
        watchState,
        libraryFreshness,
        imageResolutions,
        connectionStatus.networkActionsEnabled,
        connectionStatus.networkActionMessage,
    ) {
        detailResult?.let { result ->
            DetailPageMapper.toPage(
                result = result,
                watchState = watchState,
                libraryFreshness = libraryFreshness,
                imageResolutions = imageResolutions,
                networkActionsEnabled = connectionStatus.networkActionsEnabled,
                networkActionMessage = connectionStatus.networkActionMessage,
            )
        }
    }
    val focusPageKey = page?.stableKey ?: detailResult?.route?.stableKey ?: "loading"
    val focusRestorer = rememberTvFocusRestorer(TvDetailFocusPolicy.screen(focusPageKey))

    TvScaffold(
        modifier = Modifier.testTag(FerrexQaTags.Tv.Detail),
        contentMaxWidth = FerrexDesignTokens.Tv.DetailMaxWidth,
        horizontalPadding = FerrexDesignTokens.Space.ScreenTvHorizontal,
        verticalPadding = FerrexDesignTokens.Tv.DetailVerticalPadding,
        verticalArrangement = Arrangement.Top,
        scrollable = true,
    ) {
        key(focusPageKey) {
            TvDetailActionShelf(
                buttons = buildList {
                    add(
                        TvDetailButton(
                            key = TvDetailFocusPolicy.ITEM_BACK,
                            label = "Back",
                            role = DetailActionRole.Back,
                            enabled = true,
                            testTag = FerrexQaTags.Tv.action(TvDetailFocusPolicy.SURFACE_BACK, TvDetailFocusPolicy.ITEM_BACK),
                            contentDescription = "Back to the previous TV screen",
                            onSelect = onBack,
                        ),
                    )
                    if (connectionStatus.visible) {
                        add(
                            TvDetailButton(
                                key = "retry-connection",
                                label = connectionStatus.retryLabel,
                                role = DetailActionRole.Retry,
                                enabled = connectionStatus.retryEnabled,
                                testTag = FerrexQaTags.Tv.action(TvDetailFocusPolicy.SURFACE_BACK, "retry-connection"),
                                contentDescription = connectionStatus.retryLabel,
                                onSelect = onRetryConnection,
                            ),
                        )
                    }
                },
                surfaceKey = TvDetailFocusPolicy.SURFACE_BACK,
                focusRestorer = focusRestorer,
                autoFocus = true,
            )
        }
        TvDetailConnectionAndNotice(
            focusPageKey = focusPageKey,
            connectionStatus = connectionStatus,
            playbackNotice = playbackNotice,
        )
        Spacer(Modifier.height(FerrexDesignTokens.Space.Md))
        if (page == null) {
            TvDetailLoadingState(onRetryCacheSync = onRetryCacheSync)
        } else {
            TvDetailPageContent(
                page = page,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                scope = scope,
                focusRestorer = focusRestorer,
                onAction = { action ->
                    action.dispatchDetailAction(
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
                onRailItemActivated = { rail, item ->
                    when (rail.activationPolicy) {
                        DetailRailActivationPolicy.Play -> item.playbackContract?.let(onPlaybackContract)
                        DetailRailActivationPolicy.Navigate -> item.route?.let(onOpenDetail)
                        DetailRailActivationPolicy.Disabled -> Unit
                    }
                },
            )
        }
    }
}

@Composable
private fun TvDetailConnectionAndNotice(
    focusPageKey: String,
    connectionStatus: AuthenticatedConnectionUi,
    playbackNotice: String?,
) {
    if (connectionStatus.visible) {
        TvDetailStatusSurface(
            title = connectionStatus.title,
            body = connectionStatus.message,
            tone = FerrexStageSurfaceTone.StaleOffline,
            testTag = FerrexQaTags.namespaced("tv", "theater-plate", "status", focusPageKey, "connection"),
            contentDescription = "${connectionStatus.title}. ${connectionStatus.message}",
        )
    }
    if (!connectionStatus.networkActionsEnabled && connectionStatus.networkActionMessage != null) {
        TvDetailStatusSurface(
            title = "Playback and watch updates paused",
            body = connectionStatus.networkActionMessage,
            tone = FerrexStageSurfaceTone.StaleOffline,
            testTag = FerrexQaTags.namespaced("tv", "theater-plate", "status", focusPageKey, "network-actions-paused"),
            contentDescription = "Playback and watch updates paused. ${connectionStatus.networkActionMessage}",
        )
    }
    playbackNotice?.let {
        Text(
            text = it,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun TvDetailLoadingState(onRetryCacheSync: () -> Unit) {
    TvDetailStatusSurface(
        title = "Details loading",
        body = "Library cache is resolving the selected media.",
        loading = true,
        tone = FerrexStageSurfaceTone.Cache,
        testTag = FerrexQaTags.namespaced("tv", "theater-plate", "status", "loading"),
        contentDescription = "Details loading. Library cache is resolving the selected media.",
    )
    TvDetailActionShelf(
        buttons = listOf(
            TvDetailButton(
                key = "retry-cache",
                label = "Retry cache sync",
                role = DetailActionRole.Retry,
                enabled = true,
                testTag = FerrexQaTags.Tv.action("detail-loading", "retry-cache"),
                contentDescription = "Retry cache sync",
                onSelect = onRetryCacheSync,
            ),
        ),
        surfaceKey = "detail-loading",
        focusRestorer = null,
        autoFocus = false,
    )
}

@Composable
private fun TvDetailStatusSurface(
    title: String,
    body: String,
    tone: FerrexStageSurfaceTone,
    testTag: String,
    contentDescription: String,
    modifier: Modifier = Modifier,
    loading: Boolean = false,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.StatusSlab,
        density = DetailSurfaceInteractionMode.TvDpad.density,
        tone = tone,
        modifier = modifier.fillMaxWidth(),
        contentDescription = contentDescription,
        testTag = testTag,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
            ) {
                if (loading) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(FerrexDesignTokens.Space.Xl),
                        color = MaterialTheme.colorScheme.primary,
                        strokeWidth = FerrexDesignTokens.Focus.TvRestingBorder,
                    )
                }
                TheaterPlateText(
                    text = title,
                    role = TheaterPlateTypographyRole.StatusTitle,
                    densityRole = TheaterPlateDensityRole.Tv1080p,
                )
            }
            TheaterPlateText(
                text = body,
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = TheaterPlateDensityRole.Tv1080p,
            )
        }
    }
}

@Composable
private fun TvDetailPageContent(
    page: DetailPageModel,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    scope: ServerCacheScope,
    focusRestorer: TvFocusRestorer,
    onAction: (DetailPageAction) -> Unit,
    onRailItemActivated: (DetailRail, DetailRailItem) -> Unit,
) {
    val interactionMode = DetailSurfaceInteractionMode.TvDpad
    val presentation = remember(page, interactionMode) { DetailPrimitivePresenter.stage(page, interactionMode) }
    val lastTarget = focusRestorer.state.lastTarget(focusRestorer.screen)

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .testTag(presentation.testTag)
            .semantics { contentDescription = presentation.contentDescription },
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
    ) {
        FerrexDetailHero(
            page = page,
            presentation = presentation,
            imageResolutions = imageResolutions,
            imageLoader = imageLoader,
            serverUrl = scope.canonicalServerUrl,
            interactionMode = interactionMode,
        )
        if (presentation.metadataBand.chips.isNotEmpty()) {
            FerrexDetailMetadataBand(
                presentation = presentation.metadataBand,
                interactionMode = interactionMode,
            )
        }
        if (page.actions.isNotEmpty()) {
            TvDetailPageActionShelf(
                actions = page.actions,
                presentations = presentation.actionShelf.actions,
                surfaceKey = TvDetailFocusPolicy.SURFACE_ACTIONS,
                focusRestorer = focusRestorer,
                autoFocus = lastTarget?.surface == TvDetailFocusPolicy.SURFACE_ACTIONS,
                onAction = onAction,
                title = "Playback and watch actions",
                testTag = presentation.actionShelf.testTag,
                contentDescription = presentation.actionShelf.contentDescription,
            )
        }
        presentation.slabs.forEach { slab ->
            val sourceActions = if (slab.actions.isEmpty()) emptyList() else page.recovery.actions
            TvDetailStatusSlab(
                slab = slab,
                sourceActions = sourceActions,
                focusRestorer = focusRestorer,
                autoFocus = lastTarget?.surface == TvDetailFocusPolicy.recoverySurface(slab.testTag),
                onAction = onAction,
            )
        }
        page.rails.zip(presentation.rails).forEach { (rail, railPresentation) ->
            val surfaceKey = TvDetailFocusPolicy.railSurface(rail.stableKey)
            TvDetailRail(
                rail = rail,
                presentation = railPresentation,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                serverUrl = scope.canonicalServerUrl,
                focusRestorer = focusRestorer,
                surfaceKey = surfaceKey,
                autoFocus = lastTarget?.surface == surfaceKey,
                onRailItemActivated = onRailItemActivated,
            )
        }
    }
}

@Composable
private fun TvDetailPageActionShelf(
    actions: List<DetailPageAction>,
    presentations: List<DetailActionPresentation>,
    surfaceKey: String,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onAction: (DetailPageAction) -> Unit,
    title: String,
    testTag: String,
    contentDescription: String,
) {
    TvDetailActionShelf(
        buttons = actions.zip(presentations).map { (action, presentation) ->
            TvDetailButton(
                key = presentation.key,
                label = presentation.label,
                role = action.role,
                enabled = presentation.enabled,
                testTag = presentation.testTag,
                contentDescription = presentation.contentDescription,
                disabledReason = presentation.disabledReason,
                onSelect = { onAction(action) },
            )
        },
        surfaceKey = surfaceKey,
        focusRestorer = focusRestorer,
        autoFocus = autoFocus,
        title = title,
        testTag = testTag,
        contentDescription = contentDescription,
    )
}

@Composable
private fun TvDetailStatusSlab(
    slab: DetailSlabPresentation,
    sourceActions: List<DetailPageAction>,
    focusRestorer: TvFocusRestorer,
    autoFocus: Boolean,
    onAction: (DetailPageAction) -> Unit,
) {
    val recoveryPairs = sourceActions.zip(slab.actions)
    val recoverySurfaceKey = TvDetailFocusPolicy.recoverySurface(slab.testTag)
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.StatusSlab,
        density = DetailSurfaceInteractionMode.TvDpad.density,
        tone = slab.tone,
        modifier = Modifier.fillMaxWidth(),
        contentDescription = slab.contentDescription,
        testTag = slab.testTag,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            TheaterPlateText(
                text = slab.title,
                role = TheaterPlateTypographyRole.StatusTitle,
                densityRole = TheaterPlateDensityRole.Tv1080p,
            )
            TheaterPlateText(
                text = slab.message,
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = TheaterPlateDensityRole.Tv1080p,
            )
            if (recoveryPairs.isNotEmpty()) {
                TvDetailActionShelf(
                    buttons = recoveryPairs.map { (action, presentation) ->
                        TvDetailButton(
                            key = presentation.key,
                            label = presentation.label,
                            role = action.role,
                            enabled = presentation.enabled,
                            testTag = presentation.testTag,
                            contentDescription = presentation.contentDescription,
                            disabledReason = presentation.disabledReason,
                            onSelect = { onAction(action) },
                        )
                    },
                    surfaceKey = recoverySurfaceKey,
                    focusRestorer = focusRestorer,
                    autoFocus = autoFocus,
                    title = "Recovery and diagnostics",
                    testTag = "${slab.testTag}.actions",
                    contentDescription = "Recovery actions for ${slab.title}",
                )
            }
        }
    }
}

@Composable
private fun TvDetailRail(
    rail: DetailRail,
    presentation: DetailRailPresentation,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    serverUrl: String,
    focusRestorer: TvFocusRestorer,
    surfaceKey: String,
    autoFocus: Boolean,
    onRailItemActivated: (DetailRail, DetailRailItem) -> Unit,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.RailBand,
        density = DetailSurfaceInteractionMode.TvDpad.density,
        tone = if (rail.state == DetailRailState.Unavailable) FerrexStageSurfaceTone.Warning else FerrexStageSurfaceTone.Neutral,
        modifier = Modifier.fillMaxWidth(),
        contentDescription = presentation.contentDescription,
        testTag = presentation.testTag,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                TheaterPlateText(
                    text = presentation.title,
                    role = TheaterPlateTypographyRole.RailTitle,
                    densityRole = TheaterPlateDensityRole.Tv1080p,
                )
                TvDetailTextBadge(presentation.stateLabel)
                TvDetailTextBadge(presentation.activationPolicyLabel)
            }
            presentation.emptyOrUnavailableMessage?.let { message ->
                TheaterPlateText(
                    text = message,
                    role = TheaterPlateTypographyRole.RailSubtitle,
                    densityRole = TheaterPlateDensityRole.Tv1080p,
                )
            }
            if (rail.items.isNotEmpty()) {
                val pairs = remember(rail.items, presentation.items) { rail.items.zip(presentation.items) }
                val keys = pairs.map { (_, itemPresentation) -> itemPresentation.renderKey }
                val requesters = remember(keys) { keys.associateWith { FocusRequester() } }
                val restoredKey = keys.firstOrNull()?.let { fallback ->
                    focusRestorer.restoreItem(surfaceKey, keys, fallback)
                }
                LaunchedEffect(autoFocus, restoredKey, keys) {
                    if (autoFocus && restoredKey != null) {
                        runCatching { requesters[restoredKey]?.requestFocus() }
                    }
                }
                LazyRow(
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusGroup(),
                    horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
                    contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Xs),
                ) {
                    items(
                        items = pairs,
                        key = { (_, itemPresentation) -> itemPresentation.renderKey },
                    ) { (item, itemPresentation) ->
                        TvDetailRailItemCard(
                            rail = rail,
                            item = item,
                            presentation = itemPresentation,
                            imageResolutions = imageResolutions,
                            imageLoader = imageLoader,
                            serverUrl = serverUrl,
                            focusRequester = requesters[itemPresentation.renderKey],
                            onFocused = { focusRestorer.record(surfaceKey, itemPresentation.renderKey) },
                            onActivate = { onRailItemActivated(rail, item) },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun TvDetailRailItemCard(
    rail: DetailRail,
    item: DetailRailItem,
    presentation: DetailRailItemPresentation,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    serverUrl: String,
    focusRequester: FocusRequester?,
    onFocused: () -> Unit,
    onActivate: () -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val focusTreatment = FerrexDesignTokens.Focus.tvTreatment(TvFocusTreatmentRole.MediaArt)
    val scale by animateFloatAsState(
        targetValue = if (focused) focusTreatment.focusedScale else focusTreatment.restingScale,
        animationSpec = tween(FerrexDesignTokens.Motion.FocusMillis),
        label = "tvDetailRailFocusScale",
    )
    val borderColor = when {
        focused -> MaterialTheme.colorScheme.primary
        presentation.activatable -> MaterialTheme.colorScheme.outline.copy(alpha = 0.42f)
        else -> MaterialTheme.colorScheme.outline.copy(alpha = 0.22f)
    }
    Surface(
        modifier = Modifier
            .width(presentation.media.sizing.width)
            .scale(scale)
            .testTag(presentation.testTag)
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .onFocusChanged {
                focused = it.isFocused
                if (it.isFocused) onFocused()
            }
            .tvDetailRemoteActivation(presentation.activatable, onActivate)
            .detailRailItemSemantics(presentation, onActivate)
            .focusable(),
        shape = FerrexDesignTokens.Shapes.FocusSurface,
        color = Color.Transparent,
        contentColor = MaterialTheme.colorScheme.onSurface,
        border = BorderStroke(
            width = if (focused) focusTreatment.focusedBorder else focusTreatment.restingBorder,
            color = borderColor,
        ),
        tonalElevation = if (focused) focusTreatment.focusedElevation else FerrexDesignTokens.Space.None,
        shadowElevation = FerrexDesignTokens.Space.None,
    ) {
        FerrexStageSurface(
            variant = FerrexStageSurfaceVariant.ProjectionShelf,
            density = DetailSurfaceInteractionMode.TvDpad.density,
            tone = if (presentation.activatable) FerrexStageSurfaceTone.Neutral else FerrexStageSurfaceTone.StaleOffline,
            modifier = Modifier.fillMaxWidth(),
            contentDescription = null,
            testTag = null,
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
                FerrexDetailMediaObject(
                    art = item.art,
                    presentation = presentation.media,
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    serverUrl = serverUrl,
                    extraBadges = presentation.badges,
                )
                TheaterPlateText(
                    text = item.title,
                    role = TheaterPlateTypographyRole.RailTitle,
                    densityRole = TheaterPlateDensityRole.Tv1080p,
                    maxLines = 2,
                )
                item.subtitle?.let {
                    TheaterPlateText(
                        text = it,
                        role = TheaterPlateTypographyRole.RailSubtitle,
                        densityRole = TheaterPlateDensityRole.Tv1080p,
                        maxLines = 2,
                    )
                }
                item.progress?.let { progress ->
                    TvDetailProgressBar(progress = progress, label = "${item.title} progress")
                }
                if (!presentation.activatable) {
                    Text(
                        text = rail.activationPolicy.disabledHint(),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

@Composable
private fun TvDetailProgressBar(progress: Float, label: String) {
    val coerced = progress.coerceIn(0f, 1f)
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .height(FerrexDesignTokens.Space.Xs)
            .semantics { contentDescription = label },
        shape = FerrexDesignTokens.Shapes.Pill,
        color = FerrexDesignTokens.Palette.SlateLine,
        contentColor = FerrexDesignTokens.Palette.SignalCyan,
    ) {
        Row(modifier = Modifier.fillMaxWidth()) {
            Surface(
                modifier = Modifier
                    .fillMaxWidth(coerced)
                    .height(FerrexDesignTokens.Space.Xs),
                color = FerrexDesignTokens.Palette.SignalCyan,
                contentColor = FerrexDesignTokens.Palette.SignalCyan,
            ) {}
        }
    }
}

@Composable
private fun TvDetailActionShelf(
    buttons: List<TvDetailButton>,
    surfaceKey: String,
    focusRestorer: TvFocusRestorer?,
    autoFocus: Boolean,
    modifier: Modifier = Modifier,
    title: String? = null,
    testTag: String = FerrexQaTags.Tv.surface(surfaceKey),
    contentDescription: String = "TV detail action shelf $surfaceKey",
) {
    if (buttons.isEmpty()) return
    val keys = buttons.map { it.key }
    val disabledReasons = buttons.mapNotNull { it.disabledReason }.distinct()
    val requesters = remember(keys) { keys.associateWith { FocusRequester() } }
    val enabledKeys = buttons.filter { it.enabled }.map { it.key }
    val restoredKey = enabledKeys.firstOrNull()?.let { fallback ->
        focusRestorer?.restoreItem(surfaceKey, enabledKeys, fallback) ?: fallback
    }
    LaunchedEffect(autoFocus, restoredKey, keys) {
        if (autoFocus && restoredKey != null) {
            runCatching { requesters[restoredKey]?.requestFocus() }
        }
    }
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ControlShelf,
        density = DetailSurfaceInteractionMode.TvDpad.density,
        tone = if (buttons.any { it.role == DetailActionRole.Primary }) FerrexStageSurfaceTone.Primary else FerrexStageSurfaceTone.Neutral,
        modifier = modifier.fillMaxWidth(),
        contentDescription = contentDescription,
        testTag = testTag,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            title?.let {
                TheaterPlateText(
                    text = it,
                    role = TheaterPlateTypographyRole.SectionTitle,
                    densityRole = TheaterPlateDensityRole.Tv1080p,
                )
            }
            LazyRow(
                modifier = Modifier
                    .fillMaxWidth()
                    .focusGroup(),
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
                contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Xs),
            ) {
                items(buttons, key = { it.key }) { button ->
                    TvFocusableButton(
                        label = button.label,
                        onClick = button.onSelect,
                        enabled = button.enabled,
                        style = button.role.focusableStyle(),
                        tone = button.role.statusTone(),
                        focusRequester = requesters[button.key],
                        contentDescription = button.contentDescription,
                        testTag = button.testTag,
                        onFocused = { focusRestorer?.record(surfaceKey, button.key) },
                        modifier = Modifier.widthIn(
                            min = FerrexDesignTokens.Tv.ActionMinWidth,
                            max = FerrexDesignTokens.Tv.ActionMaxWidth,
                        ),
                    ) {
                        Text(button.label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    }
                }
            }
            disabledReasons.forEach { reason ->
                TheaterPlateText(
                    text = reason,
                    role = TheaterPlateTypographyRole.ActionSubtitle,
                    densityRole = TheaterPlateDensityRole.Tv1080p,
                )
            }
        }
    }
}

@Composable
private fun TvDetailTextBadge(label: String) {
    Surface(
        shape = FerrexDesignTokens.Shapes.Pill,
        color = FerrexDesignTokens.Palette.SlateBlack.copy(alpha = 0.78f),
        contentColor = FerrexDesignTokens.Palette.TextPrimary,
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, FerrexDesignTokens.Palette.TextMuted.copy(alpha = 0.52f)),
        tonalElevation = FerrexDesignTokens.Space.None,
        shadowElevation = FerrexDesignTokens.Space.None,
        modifier = Modifier.semantics { contentDescription = label },
    ) {
        Text(
            modifier = Modifier.padding(horizontal = FerrexDesignTokens.Space.Sm, vertical = FerrexDesignTokens.Space.Xs),
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = FerrexDesignTokens.Palette.TextPrimary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

private fun Modifier.detailRailItemSemantics(
    presentation: DetailRailItemPresentation,
    onActivate: () -> Unit,
): Modifier = semantics(mergeDescendants = true) {
    contentDescription = presentation.contentDescription
    role = Role.Button
    if (presentation.activatable) {
        onClick(label = presentation.activationLabel) {
            onActivate()
            true
        }
    } else {
        disabled()
    }
}

private fun Modifier.tvDetailRemoteActivation(
    enabled: Boolean,
    onActivate: () -> Unit,
): Modifier = if (!enabled) {
    this
} else {
    onPreviewKeyEvent { event ->
        if (!event.key.isTvDetailActivationKey()) return@onPreviewKeyEvent false
        when (event.type) {
            KeyEventType.KeyDown -> true
            KeyEventType.KeyUp -> {
                onActivate()
                true
            }
            else -> false
        }
    }
}

private fun Key.isTvDetailActivationKey(): Boolean = this == Key.DirectionCenter || this == Key.Enter || this == Key.NumPadEnter

private fun DetailPageAction.dispatchDetailAction(
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
        DetailPageActionKind.MarkUnwatched -> dispatchWatchToggle(
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

private fun DetailPageAction.dispatchWatchToggle(
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

private fun DetailRailActivationPolicy.disabledHint(): String = when (this) {
    DetailRailActivationPolicy.Navigate -> "Open details unavailable"
    DetailRailActivationPolicy.Play -> "Playback unavailable"
    DetailRailActivationPolicy.Disabled -> "Reference only"
}

private fun DetailActionRole.focusableStyle(): TvFocusableStyle = when (this) {
    DetailActionRole.Primary,
    DetailActionRole.Retry -> TvFocusableStyle.Primary
    DetailActionRole.DestructiveReset -> TvFocusableStyle.Destructive
    DetailActionRole.Secondary,
    DetailActionRole.Cache,
    DetailActionRole.Diagnostics,
    DetailActionRole.Back -> TvFocusableStyle.Secondary
}

private fun DetailActionRole.statusTone(): FerrexStatusTone = when (this) {
    DetailActionRole.Primary -> FerrexStatusTone.Primary
    DetailActionRole.Retry -> FerrexStatusTone.Retry
    DetailActionRole.Cache -> FerrexStatusTone.Cache
    DetailActionRole.DestructiveReset -> FerrexStatusTone.DestructiveReset
    DetailActionRole.Secondary,
    DetailActionRole.Diagnostics,
    DetailActionRole.Back -> FerrexStatusTone.Secondary
}

private data class TvDetailButton(
    val key: String,
    val label: String,
    val role: DetailActionRole,
    val enabled: Boolean,
    val testTag: String,
    val contentDescription: String,
    val disabledReason: String? = null,
    val onSelect: () -> Unit,
)
