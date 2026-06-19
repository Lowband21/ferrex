package com.ferrex.android.ui.detail

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil.ImageLoader
import com.ferrex.android.core.detail.DetailActionRole
import com.ferrex.android.core.detail.DetailArtRole
import com.ferrex.android.core.detail.DetailEmptyState
import com.ferrex.android.core.detail.DetailFactItem
import com.ferrex.android.core.detail.DetailFreshnessKind
import com.ferrex.android.core.detail.DetailFreshnessNotice
import com.ferrex.android.core.detail.DetailImageState
import com.ferrex.android.core.detail.DetailMetadataItem
import com.ferrex.android.core.detail.DetailPageAction
import com.ferrex.android.core.detail.DetailPageActionKind
import com.ferrex.android.core.detail.DetailPageArt
import com.ferrex.android.core.detail.DetailPageKind
import com.ferrex.android.core.detail.DetailPageModel
import com.ferrex.android.core.detail.DetailRail
import com.ferrex.android.core.detail.DetailRailActivationPolicy
import com.ferrex.android.core.detail.DetailRailCardKind
import com.ferrex.android.core.detail.DetailRailItem
import com.ferrex.android.core.detail.DetailRailState
import com.ferrex.android.core.detail.DetailTone
import com.ferrex.android.core.detail.DetailWatchState
import com.ferrex.android.core.detail.DetailWatchStateKind
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.mediaart.MediaArtFallbackPolicy
import com.ferrex.android.core.mediaart.MediaArtFitPolicy
import com.ferrex.android.core.mediaart.MediaArtGrounding
import com.ferrex.android.core.mediaart.runtimeFallback
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.theaterplate.TheaterPlateAnalysis
import com.ferrex.android.core.theaterplate.TheaterPlateAnalyzer
import com.ferrex.android.core.theaterplate.TheaterPlateColor
import com.ferrex.android.core.theaterplate.TheaterPlateImageSource
import com.ferrex.android.core.theaterplate.TheaterPlateImageSourceKind
import com.ferrex.android.core.theaterplate.TheaterPlateSourceContext
import com.ferrex.android.core.theaterplate.TheaterPlateViewport
import com.ferrex.android.ui.components.FerrexActionButton
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexMediaArt
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theaterplate.FerrexStageDensityFamily
import com.ferrex.android.ui.theaterplate.FerrexStageSurface
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceTone
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceVariant
import com.ferrex.android.ui.theaterplate.TheaterPlateBackdropAdaptation
import com.ferrex.android.ui.theme.FerrexDesignTokens
import kotlin.math.roundToInt

/** Input mode for the shared detail primitives. The same [DetailPageModel] drives both variants. */
enum class DetailSurfaceInteractionMode(
    val targetKey: String,
    val density: FerrexStageDensityFamily,
    val activationVerb: String,
    val actionMinWidth: Dp,
) {
    PhoneTouch(
        targetKey = "phone",
        density = FerrexStageDensityFamily.Standard,
        activationVerb = "Tap",
        actionMinWidth = 120.dp,
    ),
    PhoneLandscapeTouch(
        targetKey = "phone",
        density = FerrexStageDensityFamily.Standard,
        activationVerb = "Tap",
        actionMinWidth = 132.dp,
    ),
    TvDpad(
        targetKey = "tv",
        density = FerrexStageDensityFamily.TenFoot,
        activationVerb = "Press Select",
        actionMinWidth = FerrexDesignTokens.Tv.ActionMinWidth,
    ),
}

@Immutable
data class DetailMediaSizing(
    val width: Dp,
    val minHeight: Dp,
    val maxHeight: Dp,
    val aspectRatio: Float,
) {
    init {
        require(width.value > 0f) { "Detail media width must be positive" }
        require(minHeight.value >= 0f) { "Detail media min height must be non-negative" }
        require(maxHeight.value >= minHeight.value) { "Detail media max height must be >= min height" }
        require(aspectRatio > 0f) { "Detail media aspect ratio must be positive" }
    }
}

@Immutable
data class DetailMediaPresentation(
    val stableKey: String,
    val role: DetailArtRole,
    val testTag: String,
    val contentDescription: String,
    val fallbackLabel: String,
    val sizing: DetailMediaSizing,
    val badges: List<String>,
    val fitPolicy: MediaArtFitPolicy?,
    val grounding: MediaArtGrounding?,
    val stateLabel: String,
)

@Immutable
data class DetailActionPresentation(
    val key: String,
    val label: String,
    val enabled: Boolean,
    val role: FerrexActionRole,
    val testTag: String,
    val contentDescription: String,
    val disabledReason: String?,
)

@Immutable
data class DetailActionShelfPresentation(
    val testTag: String,
    val contentDescription: String,
    val actions: List<DetailActionPresentation>,
)

@Immutable
data class DetailMetadataChipPresentation(
    val label: String,
    val tone: FerrexStageSurfaceTone,
    val contentDescription: String,
)

@Immutable
data class DetailMetadataBandPresentation(
    val testTag: String,
    val contentDescription: String,
    val chips: List<DetailMetadataChipPresentation>,
)

@Immutable
data class DetailSlabPresentation(
    val testTag: String,
    val title: String,
    val message: String,
    val tone: FerrexStageSurfaceTone,
    val contentDescription: String,
    val actions: List<DetailActionPresentation>,
)

@Immutable
data class DetailRailItemPresentation(
    val renderKey: String,
    val stableId: String,
    val testTag: String,
    val contentDescription: String,
    val activationLabel: String,
    val activatable: Boolean,
    val badges: List<String>,
    val progressLabel: String?,
    val media: DetailMediaPresentation,
)

@Immutable
data class DetailRailPresentation(
    val stableKey: String,
    val testTag: String,
    val title: String,
    val stateLabel: String,
    val activationPolicyLabel: String,
    val containmentLabel: String,
    val contentDescription: String,
    val items: List<DetailRailItemPresentation>,
    val emptyOrUnavailableMessage: String?,
    val virtualized: Boolean,
)

@Immutable
data class DetailStagePresentation(
    val stableKey: String,
    val testTag: String,
    val contentDescription: String,
    val density: FerrexStageDensityFamily,
    val heroMedia: List<DetailMediaPresentation>,
    val metadataBand: DetailMetadataBandPresentation,
    val actionShelf: DetailActionShelfPresentation,
    val slabs: List<DetailSlabPresentation>,
    val rails: List<DetailRailPresentation>,
)

@Immutable
data class DetailTheaterPlateStagePresentation(
    val stableKey: String,
    val contentDescription: String,
    val context: TheaterPlateSourceContext,
    val adaptation: TheaterPlateBackdropAdaptation,
    val sourceArt: DetailPageArt?,
)

/** Presentation seam for the route-level Theater Plate background used by phone detail. */
object DetailTheaterPlateStagePresenter {
    fun stage(
        page: DetailPageModel,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
        viewport: TheaterPlateViewport,
    ): DetailTheaterPlateStagePresentation {
        val sourceArt = stageSourceArt(page)
        val sourceKey = sourceArt?.requestKey
        val resolution = sourceKey?.let(imageResolutions::get)
        val context = TheaterPlateSourceContext(
            source = if (sourceKey != null) {
                TheaterPlateImageSource.backdrop(
                    request = sourceKey,
                    token = resolution.stageToken(sourceKey),
                )
            } else {
                TheaterPlateImageSource.fallback(TheaterPlateImageSourceKind.GeneratedFallback)
            },
            viewport = viewport,
            defaultColor = page.kind.detailStageDefaultColor(),
        )
        return DetailTheaterPlateStagePresentation(
            stableKey = page.stableKey,
            contentDescription = buildString {
                append(page.kind.label)
                append(" detail Theater Plate stage for ")
                append(page.title)
                sourceArt?.label?.let { append(". Stage source: ").append(it) }
            },
            context = context,
            adaptation = sourceArt.stageAdaptation(resolution),
            sourceArt = sourceArt,
        )
    }

    fun analysis(
        page: DetailPageModel,
        imageResolutions: Map<ImageRequestKey, ImageResolution>,
        viewport: TheaterPlateViewport,
        analyzer: TheaterPlateAnalyzer = TheaterPlateAnalyzer(),
    ): TheaterPlateAnalysis {
        val context = stage(page, imageResolutions, viewport).context
        return analyzer.analyzeMissingBackdrop(context).copy(context = context)
    }

    private fun stageSourceArt(page: DetailPageModel): DetailPageArt? = listOfNotNull(
        page.hero.background.takeIf { it.role == DetailArtRole.Backdrop && it.requestKey != null },
        page.hero.background.takeIf { it.role == DetailArtRole.Still && it.requestKey != null },
    ).firstOrNull()

    private fun ImageResolution?.stageToken(sourceKey: ImageRequestKey): String = when (this) {
        is ImageResolution.Ready -> token
        else -> sourceKey.cacheKey
    }

    private fun DetailPageArt?.stageAdaptation(resolution: ImageResolution?): TheaterPlateBackdropAdaptation {
        if (this == null || imageState is DetailImageState.NoArt || resolution is ImageResolution.Placeholder) {
            return TheaterPlateBackdropAdaptation.MissingBackdrop
        }
        if (imageState.staleOffline || resolution?.stale == true) {
            return TheaterPlateBackdropAdaptation.StaleOffline
        }
        return when (resolution) {
            is ImageResolution.Failed,
            is ImageResolution.Pending -> TheaterPlateBackdropAdaptation.LowQuality
            is ImageResolution.Ready -> TheaterPlateBackdropAdaptation.Ready
            is ImageResolution.Placeholder -> TheaterPlateBackdropAdaptation.MissingBackdrop
            null -> when (imageState) {
                is DetailImageState.Ready -> TheaterPlateBackdropAdaptation.Ready
                is DetailImageState.Pending,
                is DetailImageState.Failed -> TheaterPlateBackdropAdaptation.LowQuality
                is DetailImageState.NoArt -> TheaterPlateBackdropAdaptation.MissingBackdrop
            }
        }
    }
}

/** Presentation seam used by unit tests and by the Compose primitives below. */
object DetailPrimitivePresenter {
    fun stage(page: DetailPageModel, mode: DetailSurfaceInteractionMode): DetailStagePresentation {
        val heroMedia = listOfNotNull(
            media(
                pageKey = page.stableKey,
                art = page.hero.background,
                mode = mode,
                mediaKey = "hero-background",
                preferredCardKind = cardKindFor(page.hero.background.role),
            ),
            page.hero.foreground?.let {
                media(
                    pageKey = page.stableKey,
                    art = it,
                    mode = mode,
                    mediaKey = "hero-foreground",
                    preferredCardKind = cardKindFor(it.role),
                )
            },
        )
        val metadataBand = metadataBand(page, mode)
        val actionShelf = actionShelf(page.stableKey, page.actions, mode)
        val slabs = slabs(page, mode)
        val rails = page.rails.map { rail(page.stableKey, it, mode) }
        return DetailStagePresentation(
            stableKey = page.stableKey,
            testTag = FerrexQaTags.TheaterPlate.root(mode.targetKey, page.stableKey),
            contentDescription = stageDescription(
                page = page,
                heroMedia = heroMedia,
                metadataBand = metadataBand,
                actionShelf = actionShelf,
                slabs = slabs,
            ),
            density = mode.density,
            heroMedia = heroMedia,
            metadataBand = metadataBand,
            actionShelf = actionShelf,
            slabs = slabs,
            rails = rails,
        )
    }

    fun actionShelf(
        pageKey: String,
        actions: List<DetailPageAction>,
        mode: DetailSurfaceInteractionMode,
    ): DetailActionShelfPresentation {
        val actionPresentations = actions.map { action(pageKey, it, mode) }
        return DetailActionShelfPresentation(
            testTag = FerrexQaTags.TheaterPlate.action(mode.targetKey, pageKey, "shelf"),
            contentDescription = actionShelfDescription(actionPresentations),
            actions = actionPresentations,
        )
    }

    fun action(
        pageKey: String,
        action: DetailPageAction,
        mode: DetailSurfaceInteractionMode,
    ): DetailActionPresentation {
        val key = action.stableActionKey()
        val role = action.role.toSharedRole()
        val disabledCopy = action.disabledReason?.takeIf { !action.enabled }
        return DetailActionPresentation(
            key = key,
            label = action.label,
            enabled = action.enabled,
            role = role,
            testTag = FerrexQaTags.TheaterPlate.action(mode.targetKey, pageKey, key),
            contentDescription = buildString {
                append(action.label)
                append(". ")
                append(action.role.accessibilityLabel)
                disabledCopy?.let { append(". Disabled: ").append(it) }
            },
            disabledReason = disabledCopy,
        )
    }

    fun metadataBand(page: DetailPageModel, mode: DetailSurfaceInteractionMode): DetailMetadataBandPresentation {
        val chips = buildList {
            page.metadata.forEach { item ->
                add(
                    DetailMetadataChipPresentation(
                        label = item.label,
                        tone = item.tone.toStageTone(),
                        contentDescription = metadataDescription(item),
                    ),
                )
            }
            page.facts.forEach { item ->
                add(
                    DetailMetadataChipPresentation(
                        label = if (item.value.isBlank()) item.label else "${item.label}: ${item.value}",
                        tone = item.tone.toStageTone(),
                        contentDescription = factDescription(item),
                    ),
                )
            }
            page.watchState?.let { watch ->
                add(
                    DetailMetadataChipPresentation(
                        label = watch.label,
                        tone = when (watch.state) {
                            DetailWatchStateKind.Watched -> FerrexStageSurfaceTone.Primary
                            DetailWatchStateKind.InProgress -> FerrexStageSurfaceTone.Cache
                            DetailWatchStateKind.Unavailable,
                            DetailWatchStateKind.Unknown -> FerrexStageSurfaceTone.Warning
                            DetailWatchStateKind.Unwatched -> FerrexStageSurfaceTone.Neutral
                        },
                        contentDescription = watch.message,
                    ),
                )
            }
        }
        return DetailMetadataBandPresentation(
            testTag = FerrexQaTags.namespaced(mode.targetKey, "detail", page.stableKey, "metadata"),
            contentDescription = metadataBandDescription(page.title, chips),
            chips = chips,
        )
    }

    fun slabs(page: DetailPageModel, mode: DetailSurfaceInteractionMode): List<DetailSlabPresentation> = buildList {
        page.watchState?.let { watch ->
            add(
                watchSlab(
                    pageKey = page.stableKey,
                    watch = watch,
                    recoveryActions = page.recovery.actions.takeIf { watch.needsRecoveryActions() }.orEmpty(),
                    mode = mode,
                ),
            )
        }
        page.emptyState?.let { add(emptySlab(page.stableKey, it, page.recovery.actions, mode)) }
        page.recovery.freshness?.let { add(freshnessSlab(page.stableKey, it, page.recovery.actions, mode)) }
        addAll(imageSlabs(page, mode))
    }

    fun rail(pageKey: String, rail: DetailRail, mode: DetailSurfaceInteractionMode): DetailRailPresentation {
        val stateLabel = rail.state.label
        val activationPolicyLabel = rail.activationPolicy.label
        val duplicateCounts = mutableMapOf<String, Int>()
        val itemPresentations = rail.items.map { item ->
            val occurrence = duplicateCounts.getOrDefault(item.stableId, 0)
            duplicateCounts[item.stableId] = occurrence + 1
            railItem(pageKey, rail, item, occurrence, mode)
        }
        val stateMessage = when (rail.state) {
            DetailRailState.Available -> null
            DetailRailState.Empty -> rail.emptyMessage ?: "No ${rail.title.lowercase()} are cached yet."
            DetailRailState.Unavailable -> rail.unavailableMessage ?: "${rail.title} is unavailable."
        }
        val containmentLabel = when {
            itemPresentations.isEmpty() && mode == DetailSurfaceInteractionMode.TvDpad -> "No D-pad targets"
            itemPresentations.isEmpty() -> "No rail targets"
            mode == DetailSurfaceInteractionMode.TvDpad -> "D-pad contained"
            else -> "Bounded rail"
        }
        val containmentDescription = if (mode == DetailSurfaceInteractionMode.TvDpad) {
            "Left/right rail edges are contained before focus moves to neighboring rows."
        } else {
            "Rail edges are bounded by the visible scroll container."
        }
        return DetailRailPresentation(
            stableKey = rail.stableKey,
            testTag = FerrexQaTags.TheaterPlate.rail(mode.targetKey, pageKey, rail.stableKey),
            title = rail.title,
            stateLabel = stateLabel,
            activationPolicyLabel = activationPolicyLabel,
            containmentLabel = containmentLabel,
            contentDescription = buildString {
                append(rail.title)
                append(" rail. ")
                append(stateLabel)
                append(". ")
                append(activationPolicyLabel)
                append(". ")
                append(itemPresentations.size)
                append(" item")
                if (itemPresentations.size != 1) append("s")
                append(". ")
                append(containmentLabel)
                append(". ")
                append(containmentDescription)
                stateMessage?.let { append(" ").append(it) }
            },
            items = itemPresentations,
            emptyOrUnavailableMessage = stateMessage,
            virtualized = true,
        )
    }

    fun railItem(
        pageKey: String,
        rail: DetailRail,
        item: DetailRailItem,
        occurrence: Int,
        mode: DetailSurfaceInteractionMode,
    ): DetailRailItemPresentation {
        val renderKey = item.renderKey(occurrence)
        val badges = item.badges()
        val progressLabel = item.progressLabel()
        val activatable = rail.state == DetailRailState.Available && rail.activationPolicy.isSatisfiedBy(item)
        val activationLabel = rail.activationPolicy.activationLabel(mode, activatable)
        val media = media(
            pageKey = pageKey,
            art = item.art,
            mode = mode,
            mediaKey = "${rail.stableKey}-$renderKey",
            preferredCardKind = rail.cardKind,
        )
        return DetailRailItemPresentation(
            renderKey = renderKey,
            stableId = item.stableId,
            testTag = FerrexQaTags.namespaced(mode.targetKey, "detail", pageKey, "rail-item", rail.stableKey, renderKey),
            contentDescription = buildString {
                append(item.title)
                item.subtitle?.let { append(". ").append(it) }
                if (badges.isNotEmpty()) append(". ").append(badges.joinToString(". "))
                progressLabel?.let { append(". ").append(it) }
                append(". ").append(activationLabel)
            },
            activationLabel = activationLabel,
            activatable = activatable,
            badges = badges,
            progressLabel = progressLabel,
            media = media,
        )
    }

    fun media(
        pageKey: String,
        art: DetailPageArt,
        mode: DetailSurfaceInteractionMode,
        mediaKey: String,
        preferredCardKind: DetailRailCardKind = cardKindFor(art.role),
    ): DetailMediaPresentation {
        val badges = art.imageState.badges()
        val sizing = preferredCardKind.sizing(mode)
        val safeMediaKey = art.mediaArt?.targetIdentity?.focusKey ?: mediaKey
        val detailGrounding = art.role.detailGrounding(art.mediaArt?.treatment?.grounding)
        return DetailMediaPresentation(
            stableKey = safeMediaKey,
            role = art.role,
            testTag = FerrexQaTags.TheaterPlate.media(mode.targetKey, pageKey, safeMediaKey),
            contentDescription = mediaDescription(art, detailGrounding, badges),
            fallbackLabel = fallbackLabel(art),
            sizing = sizing,
            badges = badges,
            fitPolicy = art.mediaArt?.treatment?.fitPolicy,
            grounding = detailGrounding,
            stateLabel = art.imageState.label,
        )
    }

    private fun watchSlab(
        pageKey: String,
        watch: DetailWatchState,
        recoveryActions: List<DetailPageAction>,
        mode: DetailSurfaceInteractionMode,
    ): DetailSlabPresentation {
        val actions = recoveryActions.map { action(pageKey, it, mode) }
        return DetailSlabPresentation(
            testTag = FerrexQaTags.namespaced(mode.targetKey, "theater-plate", "status", pageKey, "watch"),
            title = watch.label,
            message = watch.message,
            tone = when (watch.state) {
                DetailWatchStateKind.Watched -> FerrexStageSurfaceTone.Primary
                DetailWatchStateKind.InProgress -> FerrexStageSurfaceTone.Cache
                DetailWatchStateKind.Unavailable,
                DetailWatchStateKind.Unknown -> FerrexStageSurfaceTone.Warning
                DetailWatchStateKind.Unwatched -> FerrexStageSurfaceTone.Neutral
            },
            contentDescription = slabDescription(watch.label, watch.message, actions),
            actions = actions,
        )
    }

    private fun emptySlab(
        pageKey: String,
        empty: DetailEmptyState,
        recoveryActions: List<DetailPageAction>,
        mode: DetailSurfaceInteractionMode,
    ): DetailSlabPresentation {
        val actions = recoveryActions.map { action(pageKey, it, mode) }
        return DetailSlabPresentation(
            testTag = FerrexQaTags.namespaced(mode.targetKey, "theater-plate", "status", pageKey, "empty"),
            title = empty.title,
            message = empty.message,
            tone = FerrexStageSurfaceTone.StaleOffline,
            contentDescription = slabDescription(empty.title, empty.message, actions),
            actions = actions,
        )
    }

    private fun freshnessSlab(
        pageKey: String,
        freshness: DetailFreshnessNotice,
        recoveryActions: List<DetailPageAction>,
        mode: DetailSurfaceInteractionMode,
    ): DetailSlabPresentation {
        val actions = recoveryActions.map { action(pageKey, it, mode) }
        return DetailSlabPresentation(
            testTag = FerrexQaTags.namespaced(mode.targetKey, "theater-plate", "status", pageKey, freshness.kind.name),
            title = freshness.title,
            message = freshness.message,
            tone = freshness.kind.toStageTone(),
            contentDescription = slabDescription(freshness.title, freshness.message, actions),
            actions = actions,
        )
    }

    private fun imageSlabs(page: DetailPageModel, mode: DetailSurfaceInteractionMode): List<DetailSlabPresentation> =
        page.imageStatusSummaries().map { summary ->
            val actions = page.recovery.actions.map { action(page.stableKey, it, mode) }
            DetailSlabPresentation(
                testTag = FerrexQaTags.namespaced(mode.targetKey, "theater-plate", "status", page.stableKey, "image", summary.key),
                title = summary.title,
                message = summary.message,
                tone = summary.tone,
                contentDescription = slabDescription(summary.title, summary.message, actions),
                actions = actions,
            )
        }
}

private data class DetailImageStatusSummary(
    val key: String,
    val title: String,
    val message: String,
    val tone: FerrexStageSurfaceTone,
)

private fun stageDescription(
    page: DetailPageModel,
    heroMedia: List<DetailMediaPresentation>,
    metadataBand: DetailMetadataBandPresentation,
    actionShelf: DetailActionShelfPresentation,
    slabs: List<DetailSlabPresentation>,
): String = buildString {
    append(page.kind.label)
    append(" detail for ")
    append(page.title)
    page.subtitle?.let { append(". ").append(it) }
    append(". Hero media: ")
    append(heroMedia.summaryLabels { "${it.role.accessibilityLabel} ${it.stateLabel}" })
    append(". Media objects: ")
    append(heroMedia.summaryLabels { it.contentDescription })
    append(". Metadata: ")
    append(metadataBand.chips.summaryLabels { it.label })
    append(". Actions: ")
    append(actionShelf.actions.summaryLabels { action ->
        if (action.enabled) {
            "${action.label} enabled"
        } else {
            "${action.label} disabled: ${action.disabledReason}"
        }
    })
    append(". Status slabs: ")
    append(slabs.summaryLabels { it.title })
}

private fun actionShelfDescription(actions: List<DetailActionPresentation>): String = buildString {
    append("Detail actions.")
    if (actions.isEmpty()) {
        append(" No actions.")
    } else {
        append(" ")
        append(actions.size)
        append(" action")
        if (actions.size != 1) append("s")
        append(": ")
        append(actions.joinToString("; ") { action ->
            if (action.enabled) {
                "${action.label} enabled"
            } else {
                "${action.label} disabled: ${action.disabledReason}"
            }
        })
        append(".")
    }
}

private fun metadataBandDescription(title: String, chips: List<DetailMetadataChipPresentation>): String = buildString {
    append("Metadata band for ")
    append(title)
    append(". ")
    append(chips.size)
    append(" item")
    if (chips.size != 1) append("s")
    if (chips.isNotEmpty()) {
        append(": ")
        append(chips.joinToString(", ") { it.label })
    }
    append(".")
}

private fun mediaDescription(
    art: DetailPageArt,
    grounding: MediaArtGrounding?,
    badges: List<String>,
): String = buildString {
    append(art.role.accessibilityLabel)
    append(" media object: ")
    append(art.label)
    append(". Image state: ")
    append(art.imageState.accessibilityLabel)
    grounding?.let {
        append(". Grounding: ")
        append(it.accessibilityLabel)
    }
    if (badges.isNotEmpty()) append(". ").append(badges.joinToString(". "))
}

private fun slabDescription(
    title: String,
    message: String,
    actions: List<DetailActionPresentation>,
): String = buildString {
    append(title)
    append(". ")
    append(message)
    if (actions.isNotEmpty()) {
        append(". Recovery actions: ")
        append(actions.joinToString(", ") { action ->
            if (action.enabled) action.label else "${action.label} disabled: ${action.disabledReason}"
        })
    }
}

private fun DetailPageModel.imageStatusSummaries(): List<DetailImageStatusSummary> {
    val media = imageStatusMedia()
    val missing = media.filter { it.imageState is DetailImageState.NoArt }
    val failed = media.filter { it.imageState is DetailImageState.Failed }
    val stale = media.filter { it.imageState.staleOffline }
    return buildList {
        if (missing.isNotEmpty()) {
            add(
                DetailImageStatusSummary(
                    key = "missing-artwork",
                    title = "Missing artwork",
                    message = "Missing artwork for ${missing.mediaLabels()}. Labeled placeholders stay mounted; retry cache sync or clear the selected cache to refresh image metadata.",
                    tone = FerrexStageSurfaceTone.StaleOffline,
                ),
            )
        }
        if (failed.isNotEmpty()) {
            val reasons = failed.mapNotNull { (it.imageState as? DetailImageState.Failed)?.reason }.distinct().summaryLabels { it }
            add(
                DetailImageStatusSummary(
                    key = "failed-images",
                    title = "Image load failed",
                    message = "Failed image load for ${failed.mediaLabels()}. Reason: $reasons. Retry cache sync or clear the selected cache to request fresh image metadata.",
                    tone = FerrexStageSurfaceTone.Warning,
                ),
            )
        }
        if (stale.isNotEmpty()) {
            add(
                DetailImageStatusSummary(
                    key = "stale-offline-artwork",
                    title = "Stale/offline artwork",
                    message = "Showing stale or offline artwork for ${stale.mediaLabels()}. Details stay readable while recovery actions reconnect or refresh the cache.",
                    tone = FerrexStageSurfaceTone.StaleOffline,
                ),
            )
        }
    }
}

private fun DetailPageModel.imageStatusMedia(): List<DetailPageArt> = buildList {
    add(hero.background)
    hero.foreground?.let(::add)
}.distinctBy { art ->
    listOf(art.label, art.requestKey?.iid, art.imageState.label).joinToString("|")
}

private fun List<DetailPageArt>.mediaLabels(): String = summaryLabels { it.label }

private fun DetailWatchState.needsRecoveryActions(): Boolean = when (state) {
    DetailWatchStateKind.Unknown,
    DetailWatchStateKind.Unavailable -> true
    DetailWatchStateKind.Unwatched,
    DetailWatchStateKind.InProgress,
    DetailWatchStateKind.Watched -> false
}

private val DetailImageState.accessibilityLabel: String get() = when (this) {
    is DetailImageState.Ready -> if (staleOffline) "ready but stale/offline" else "ready"
    is DetailImageState.Pending -> if (staleOffline) "pending and stale/offline" else "pending"
    is DetailImageState.Failed -> "failed: $reason"
    is DetailImageState.NoArt -> "missing artwork: $reason"
}

private val DetailArtRole.accessibilityLabel: String get() = when (this) {
    DetailArtRole.Poster -> "Poster"
    DetailArtRole.Backdrop -> "Backdrop"
    DetailArtRole.Still -> "Still"
    DetailArtRole.Profile -> "Profile"
    DetailArtRole.None -> "Fallback"
}

private val MediaArtGrounding.accessibilityLabel: String get() = when (this) {
    MediaArtGrounding.Flat -> "flat backdrop"
    MediaArtGrounding.CardObject -> "card object"
    MediaArtGrounding.TheaterPlateContactShadow -> "Theater Plate contact shadow"
}

private fun <T> List<T>.summaryLabels(
    maxItems: Int = 4,
    label: (T) -> String,
): String {
    if (isEmpty()) return "none"
    val visible = take(maxItems).map(label).filter { it.isNotBlank() }
    val remaining = size - visible.size
    return buildString {
        append(visible.joinToString(", "))
        if (remaining > 0) append(", plus ").append(remaining).append(" more")
    }
}

class DetailPrimitiveCallbacks(
    val onAction: (DetailPageAction) -> Unit = {},
    val onPlaybackContract: (PlaybackRouteContract) -> Unit = {},
    val onRailItemActivated: (DetailRail, DetailRailItem) -> Unit = { _, _ -> },
)

@Composable
fun FerrexDetailStage(
    page: DetailPageModel,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    serverUrl: String,
    modifier: Modifier = Modifier,
    interactionMode: DetailSurfaceInteractionMode = DetailSurfaceInteractionMode.PhoneTouch,
    callbacks: DetailPrimitiveCallbacks = DetailPrimitiveCallbacks(),
    fallbackPolicy: MediaArtFallbackPolicy = MediaArtFallbackPolicy(),
    contentPadding: PaddingValues = detailStageContentPadding(interactionMode),
    containerColor: Color = FerrexDesignTokens.Palette.SlateCanvas,
    header: (@Composable () -> Unit)? = null,
) {
    val presentation = remember(page, interactionMode) { DetailPrimitivePresenter.stage(page, interactionMode) }
    LazyColumn(
        modifier = modifier
            .fillMaxSize()
            .background(containerColor)
            .testTag(presentation.testTag)
            .semantics { contentDescription = presentation.contentDescription },
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
        contentPadding = contentPadding,
    ) {
        if (header != null) {
            item(key = "detail-header") {
                header()
            }
        }
        item(key = "hero") {
            FerrexDetailHero(
                page = page,
                presentation = presentation,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                serverUrl = serverUrl,
                interactionMode = interactionMode,
                fallbackPolicy = fallbackPolicy,
            )
        }
        if (presentation.metadataBand.chips.isNotEmpty()) {
            item(key = "metadata") {
                FerrexDetailMetadataBand(
                    presentation = presentation.metadataBand,
                    interactionMode = interactionMode,
                )
            }
        }
        if (page.actions.isNotEmpty()) {
            item(key = "actions") {
                FerrexDetailActionShelf(
                    actions = page.actions,
                    presentation = presentation.actionShelf,
                    interactionMode = interactionMode,
                    callbacks = callbacks,
                )
            }
        }
        items(presentation.slabs, key = { it.testTag }) { slab ->
            val actions = if (slab.actions.isEmpty()) emptyList() else page.recovery.actions
            FerrexDetailStatusSlab(
                slab = slab,
                sourceActions = actions,
                interactionMode = interactionMode,
                callbacks = callbacks,
            )
        }
        items(page.rails, key = { it.stableKey }) { rail ->
            val railPresentation = presentation.rails.first { it.stableKey == rail.stableKey }
            FerrexDetailRail(
                rail = rail,
                presentation = railPresentation,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                serverUrl = serverUrl,
                interactionMode = interactionMode,
                callbacks = callbacks,
                fallbackPolicy = fallbackPolicy,
            )
        }
    }
}

@Composable
fun FerrexDetailHero(
    page: DetailPageModel,
    presentation: DetailStagePresentation,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    serverUrl: String,
    interactionMode: DetailSurfaceInteractionMode,
    fallbackPolicy: MediaArtFallbackPolicy = MediaArtFallbackPolicy(),
    modifier: Modifier = Modifier,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ProjectionShelf,
        density = presentation.density,
        tone = FerrexStageSurfaceTone.Neutral,
        modifier = modifier.fillMaxWidth(),
        contentDescription = "Hero detail for ${page.title}",
        testTag = FerrexQaTags.namespaced(interactionMode.targetKey, "detail", page.stableKey, "hero"),
    ) {
        val background = page.hero.background
        val foreground = page.hero.foreground
        val backgroundPresentation = presentation.heroMedia.first()
        val foregroundPresentation = presentation.heroMedia.getOrNull(1) ?: backgroundPresentation
        val heroArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg)
        val showBackdropBand = interactionMode == DetailSurfaceInteractionMode.TvDpad && foreground != null
        if (showBackdropBand) {
            Column(verticalArrangement = heroArrangement) {
                FerrexDetailMediaObject(
                    art = background,
                    presentation = backgroundPresentation,
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    serverUrl = serverUrl,
                    fallbackPolicy = fallbackPolicy,
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(horizontalArrangement = heroArrangement, verticalAlignment = Alignment.Top) {
                    FerrexDetailMediaObject(
                        art = foreground,
                        presentation = foregroundPresentation,
                        imageResolutions = imageResolutions,
                        imageLoader = imageLoader,
                        serverUrl = serverUrl,
                        fallbackPolicy = fallbackPolicy,
                        modifier = Modifier.width(foreground.role.heroWidth(interactionMode)),
                    )
                    DetailHeroCopy(page = page, interactionMode = interactionMode, modifier = Modifier.weight(1f))
                }
            }
        } else if (interactionMode.prefersWideHeroLayout() && foreground != null) {
            Row(horizontalArrangement = heroArrangement, verticalAlignment = Alignment.Top) {
                FerrexDetailMediaObject(
                    art = foreground,
                    presentation = foregroundPresentation,
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    serverUrl = serverUrl,
                    fallbackPolicy = fallbackPolicy,
                    modifier = Modifier.width(foreground.role.heroWidth(interactionMode)),
                )
                DetailHeroCopy(page = page, interactionMode = interactionMode, modifier = Modifier.weight(1f))
            }
        } else {
            val primaryArt = foreground ?: background
            val primaryPresentation = if (foreground == null) backgroundPresentation else foregroundPresentation
            Column(verticalArrangement = heroArrangement) {
                FerrexDetailMediaObject(
                    art = primaryArt,
                    presentation = primaryPresentation,
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    serverUrl = serverUrl,
                    fallbackPolicy = fallbackPolicy,
                    modifier = Modifier.widthIn(max = primaryArt.role.heroWidth(interactionMode)),
                )
                DetailHeroCopy(page = page, interactionMode = interactionMode)
            }
        }
    }
}

@Composable
fun FerrexDetailMediaObject(
    art: DetailPageArt,
    presentation: DetailMediaPresentation,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    serverUrl: String,
    modifier: Modifier = Modifier,
    fallbackPolicy: MediaArtFallbackPolicy = MediaArtFallbackPolicy(),
    extraBadges: List<String> = emptyList(),
) {
    val mediaArt = art.mediaArt
    val renderArt = remember(mediaArt, presentation.grounding) {
        if (mediaArt != null && presentation.grounding != null && mediaArt.treatment.grounding != presentation.grounding) {
            mediaArt.copy(treatment = mediaArt.treatment.copy(grounding = presentation.grounding))
        } else {
            mediaArt
        }
    }
    val combinedBadges = (extraBadges + presentation.badges).filter { it.isNotBlank() }.distinct().take(4)
    Box(
        modifier = modifier
            .defaultMinSize(minHeight = presentation.sizing.minHeight)
            .heightIn(max = presentation.sizing.maxHeight)
            .testTag(presentation.testTag)
            .semantics(mergeDescendants = true) { contentDescription = presentation.contentDescription },
    ) {
        if (renderArt != null && imageLoader != null) {
            FerrexMediaArt(
                art = renderArt,
                resolution = art.requestKey?.let { imageResolutions[it] },
                imageLoader = imageLoader,
                contentDescription = presentation.contentDescription,
                fallback = renderArt.runtimeFallback(serverUrl, fallbackPolicy),
                modifier = Modifier.fillMaxWidth(),
            )
        } else {
            DetailImageFallbackSurface(
                label = presentation.fallbackLabel,
                badges = combinedBadges,
                modifier = Modifier
                    .fillMaxWidth()
                    .aspectRatio(presentation.sizing.aspectRatio),
            )
        }
        if (combinedBadges.isNotEmpty()) {
            DetailBadgeColumn(
                badges = combinedBadges,
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(FerrexDesignTokens.Space.Xs),
            )
        }
    }
}

@Composable
fun FerrexDetailMetadataBand(
    presentation: DetailMetadataBandPresentation,
    interactionMode: DetailSurfaceInteractionMode,
    modifier: Modifier = Modifier,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.FactRibbon,
        density = interactionMode.density,
        tone = FerrexStageSurfaceTone.Cache,
        modifier = modifier.fillMaxWidth(),
        contentDescription = presentation.contentDescription,
        testTag = presentation.testTag,
    ) {
        Row(
            modifier = Modifier.horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs),
        ) {
            presentation.chips.forEach { chip ->
                DetailChip(chip = chip)
            }
        }
    }
}

@Composable
fun FerrexDetailActionShelf(
    actions: List<DetailPageAction>,
    presentation: DetailActionShelfPresentation,
    interactionMode: DetailSurfaceInteractionMode,
    callbacks: DetailPrimitiveCallbacks,
    modifier: Modifier = Modifier,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ControlShelf,
        density = interactionMode.density,
        tone = FerrexStageSurfaceTone.Primary,
        modifier = modifier.fillMaxWidth(),
        contentDescription = presentation.contentDescription,
        testTag = presentation.testTag,
    ) {
        val disabledReasons = presentation.actions.mapNotNull { it.disabledReason }.distinct()
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            Row(
                modifier = Modifier
                    .horizontalScroll(rememberScrollState())
                    .then(if (interactionMode == DetailSurfaceInteractionMode.TvDpad) Modifier.focusGroup() else Modifier),
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                actions.zip(presentation.actions).forEach { (action, actionPresentation) ->
                    FerrexActionButton(
                        label = actionPresentation.label,
                        role = actionPresentation.role,
                        enabled = actionPresentation.enabled,
                        onClick = { action.dispatch(callbacks) },
                        testTag = actionPresentation.testTag,
                        contentDescription = actionPresentation.contentDescription,
                        modifier = Modifier.widthIn(min = interactionMode.actionMinWidth),
                    )
                }
            }
            disabledReasons.forEach { reason ->
                TheaterPlateText(
                    text = reason,
                    role = TheaterPlateTypographyRole.ActionSubtitle,
                    densityRole = interactionMode.toTypographyDensity(),
                )
            }
        }
    }
}

@Composable
fun FerrexDetailStatusSlab(
    slab: DetailSlabPresentation,
    sourceActions: List<DetailPageAction>,
    interactionMode: DetailSurfaceInteractionMode,
    callbacks: DetailPrimitiveCallbacks,
    modifier: Modifier = Modifier,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.StatusSlab,
        density = interactionMode.density,
        tone = slab.tone,
        modifier = modifier.fillMaxWidth(),
        contentDescription = slab.contentDescription,
        testTag = slab.testTag,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            TheaterPlateText(
                text = slab.title,
                role = TheaterPlateTypographyRole.StatusTitle,
                densityRole = interactionMode.toTypographyDensity(),
            )
            TheaterPlateText(
                text = slab.message,
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = interactionMode.toTypographyDensity(),
            )
            if (slab.actions.isNotEmpty() && sourceActions.isNotEmpty()) {
                FerrexDetailActionShelf(
                    actions = sourceActions,
                    presentation = DetailActionShelfPresentation(
                        testTag = "${slab.testTag}.actions",
                        contentDescription = "Recovery actions for ${slab.title}",
                        actions = slab.actions,
                    ),
                    interactionMode = interactionMode,
                    callbacks = callbacks,
                )
            }
        }
    }
}

@Composable
fun FerrexDetailRail(
    rail: DetailRail,
    presentation: DetailRailPresentation,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    serverUrl: String,
    interactionMode: DetailSurfaceInteractionMode,
    callbacks: DetailPrimitiveCallbacks,
    modifier: Modifier = Modifier,
    fallbackPolicy: MediaArtFallbackPolicy = MediaArtFallbackPolicy(),
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.RailBand,
        density = interactionMode.density,
        tone = if (rail.state == DetailRailState.Unavailable) FerrexStageSurfaceTone.Warning else FerrexStageSurfaceTone.Neutral,
        modifier = modifier.fillMaxWidth(),
        contentDescription = presentation.contentDescription,
        testTag = presentation.testTag,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
                TheaterPlateText(
                    text = presentation.title,
                    role = TheaterPlateTypographyRole.RailTitle,
                    densityRole = interactionMode.toTypographyDensity(),
                )
                DetailTextBadge(label = presentation.stateLabel)
                DetailTextBadge(label = presentation.activationPolicyLabel)
                DetailTextBadge(label = presentation.containmentLabel)
            }
            presentation.emptyOrUnavailableMessage?.let { message ->
                TheaterPlateText(
                    text = message,
                    role = TheaterPlateTypographyRole.RailSubtitle,
                    densityRole = interactionMode.toTypographyDensity(),
                )
            }
            if (rail.items.isNotEmpty()) {
                LazyRow(
                    modifier = Modifier
                        .fillMaxWidth()
                        .then(if (interactionMode == DetailSurfaceInteractionMode.TvDpad) Modifier.focusGroup() else Modifier),
                    horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
                    contentPadding = PaddingValues(vertical = FerrexDesignTokens.Space.Xs),
                ) {
                    items(
                        items = rail.items.zip(presentation.items),
                        key = { (_, itemPresentation) -> itemPresentation.renderKey },
                    ) { (item, itemPresentation) ->
                        FerrexDetailRailItem(
                            rail = rail,
                            item = item,
                            presentation = itemPresentation,
                            imageResolutions = imageResolutions,
                            imageLoader = imageLoader,
                            serverUrl = serverUrl,
                            interactionMode = interactionMode,
                            callbacks = callbacks,
                            fallbackPolicy = fallbackPolicy,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun FerrexDetailRailItem(
    rail: DetailRail,
    item: DetailRailItem,
    presentation: DetailRailItemPresentation,
    imageResolutions: Map<ImageRequestKey, ImageResolution>,
    imageLoader: ImageLoader?,
    serverUrl: String,
    interactionMode: DetailSurfaceInteractionMode,
    callbacks: DetailPrimitiveCallbacks,
    fallbackPolicy: MediaArtFallbackPolicy,
) {
    val focusRequester = remember(presentation.renderKey) { FocusRequester() }
    val baseModifier = Modifier
        .width(presentation.media.sizing.width)
        .testTag(presentation.testTag)
        .then(if (interactionMode == DetailSurfaceInteractionMode.TvDpad) Modifier.focusRequester(focusRequester) else Modifier)
        .detailRailItemSemantics(presentation)
        .detailRailItemActivation(presentation.activatable) { callbacks.onRailItemActivated(rail, item) }

    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.ProjectionShelf,
        density = interactionMode.density,
        tone = if (presentation.activatable) FerrexStageSurfaceTone.Neutral else FerrexStageSurfaceTone.StaleOffline,
        modifier = baseModifier,
        contentDescription = null,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
            FerrexDetailMediaObject(
                art = item.art,
                presentation = presentation.media,
                imageResolutions = imageResolutions,
                imageLoader = imageLoader,
                serverUrl = serverUrl,
                fallbackPolicy = fallbackPolicy,
                extraBadges = presentation.badges,
            )
            TheaterPlateText(
                text = item.title,
                role = TheaterPlateTypographyRole.RailTitle,
                densityRole = interactionMode.toTypographyDensity(),
                maxLines = 2,
            )
            item.subtitle?.let {
                TheaterPlateText(
                    text = it,
                    role = TheaterPlateTypographyRole.RailSubtitle,
                    densityRole = interactionMode.toTypographyDensity(),
                    maxLines = 2,
                )
            }
            if (item.progress != null) {
                DetailProgressBar(progress = item.progress, label = presentation.progressLabel ?: "${item.title} progress")
            }
        }
    }
}

@Composable
private fun DetailHeroCopy(
    page: DetailPageModel,
    interactionMode: DetailSurfaceInteractionMode,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm)) {
        TheaterPlateText(
            text = page.kind.label,
            role = TheaterPlateTypographyRole.HeroEyebrow,
            densityRole = interactionMode.toTypographyDensity(),
        )
        TheaterPlateText(
            text = page.title,
            role = TheaterPlateTypographyRole.HeroTitle,
            densityRole = interactionMode.toTypographyDensity(),
            maxLines = if (interactionMode == DetailSurfaceInteractionMode.PhoneTouch) 3 else 2,
        )
        page.subtitle?.let {
            TheaterPlateText(
                text = it,
                role = TheaterPlateTypographyRole.HeroSubtitle,
                densityRole = interactionMode.toTypographyDensity(),
            )
        }
        page.overview?.let {
            TheaterPlateText(
                text = it,
                role = TheaterPlateTypographyRole.HeroBody,
                densityRole = interactionMode.toTypographyDensity(),
            )
        }
    }
}

@Composable
private fun DetailChip(chip: DetailMetadataChipPresentation) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.FactRibbon,
        density = FerrexStageDensityFamily.Compact,
        tone = chip.tone,
        contentDescription = chip.contentDescription,
    ) {
        Text(
            text = chip.label,
            style = MaterialTheme.typography.labelMedium,
            color = FerrexDesignTokens.Palette.TextPrimary,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun DetailImageFallbackSurface(
    label: String,
    badges: List<String>,
    modifier: Modifier = Modifier,
) {
    FerrexStageSurface(
        variant = FerrexStageSurfaceVariant.EmptyState,
        density = FerrexStageDensityFamily.Compact,
        tone = FerrexStageSurfaceTone.StaleOffline,
        modifier = modifier,
        contentDescription = label,
    ) {
        Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text(
                text = label,
                style = MaterialTheme.typography.bodyMedium,
                color = FerrexDesignTokens.Palette.TextSecondary,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
            if (badges.isNotEmpty()) {
                DetailBadgeColumn(
                    badges = badges,
                    modifier = Modifier.align(Alignment.TopStart),
                )
            }
        }
    }
}

@Composable
private fun DetailBadgeColumn(
    badges: List<String>,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        badges.filter { it.isNotBlank() }.distinct().take(4).forEach { badge ->
            DetailTextBadge(label = badge)
        }
    }
}

@Composable
private fun DetailTextBadge(label: String) {
    Surface(
        modifier = Modifier.semantics { contentDescription = label },
        shape = FerrexDesignTokens.Shapes.Pill,
        color = FerrexDesignTokens.Palette.SlateBlack.copy(alpha = 0.78f),
        contentColor = FerrexDesignTokens.Palette.TextPrimary,
        border = BorderStroke(1.dp, FerrexDesignTokens.Palette.TextMuted.copy(alpha = 0.52f)),
        tonalElevation = 0.dp,
        shadowElevation = 0.dp,
    ) {
        Text(
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = FerrexDesignTokens.Palette.TextPrimary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun DetailProgressBar(progress: Float, label: String) {
    val coerced = progress.coerceIn(0f, 1f)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(6.dp)
            .background(FerrexDesignTokens.Palette.SlateLine, FerrexDesignTokens.Shapes.Pill)
            .semantics { contentDescription = label },
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth(coerced)
                .height(6.dp)
                .background(FerrexDesignTokens.Palette.SignalCyan, FerrexDesignTokens.Shapes.Pill),
        )
    }
}

private fun Modifier.detailRailItemActivation(
    activatable: Boolean,
    onClick: () -> Unit,
): Modifier = if (activatable) {
    clickable(role = Role.Button, onClick = onClick)
} else {
    this
}

private fun Modifier.detailRailItemSemantics(presentation: DetailRailItemPresentation): Modifier = semantics(mergeDescendants = true) {
    contentDescription = presentation.contentDescription
    role = Role.Button
    if (!presentation.activatable) disabled()
}

private fun DetailPageAction.dispatch(callbacks: DetailPrimitiveCallbacks) {
    playbackContract?.let {
        callbacks.onPlaybackContract(it)
        return
    }
    callbacks.onAction(this)
}

private fun DetailPageAction.stableActionKey(): String = listOfNotNull(
    kind.key,
    targetId?.takeIf { it.isNotBlank() }?.let(FerrexQaTags::segment),
).joinToString("-")

private val DetailPageActionKind.key: String get() = name.replace(Regex("([a-z])([A-Z])"), "${'$'}1-${'$'}2").lowercase()

private val DetailActionRole.accessibilityLabel: String get() = when (this) {
    DetailActionRole.Primary -> "Primary action"
    DetailActionRole.Secondary -> "Secondary action"
    DetailActionRole.Retry -> "Retry action"
    DetailActionRole.Cache -> "Cache recovery action"
    DetailActionRole.DestructiveReset -> "Destructive reset action"
    DetailActionRole.Diagnostics -> "Diagnostics action"
    DetailActionRole.Back -> "Back action"
}

private fun DetailActionRole.toSharedRole(): FerrexActionRole = when (this) {
    DetailActionRole.Primary -> FerrexActionRole.Primary
    DetailActionRole.Secondary -> FerrexActionRole.Secondary
    DetailActionRole.Retry -> FerrexActionRole.Retry
    DetailActionRole.Cache -> FerrexActionRole.Cache
    DetailActionRole.DestructiveReset -> FerrexActionRole.DestructiveReset
    DetailActionRole.Diagnostics -> FerrexActionRole.Secondary
    DetailActionRole.Back -> FerrexActionRole.Secondary
}

private fun detailStageContentPadding(interactionMode: DetailSurfaceInteractionMode): PaddingValues = PaddingValues(
    horizontal = when (interactionMode) {
        DetailSurfaceInteractionMode.TvDpad -> FerrexDesignTokens.Space.ScreenTvHorizontal
        DetailSurfaceInteractionMode.PhoneLandscapeTouch -> 32.dp
        DetailSurfaceInteractionMode.PhoneTouch -> FerrexDesignTokens.Space.ScreenPhoneHorizontal
    },
    vertical = when (interactionMode) {
        DetailSurfaceInteractionMode.TvDpad -> FerrexDesignTokens.Space.ScreenTvVertical
        DetailSurfaceInteractionMode.PhoneLandscapeTouch -> FerrexDesignTokens.Space.Xxl
        DetailSurfaceInteractionMode.PhoneTouch -> FerrexDesignTokens.Space.ScreenPhoneVertical
    },
)

private fun DetailPageKind.detailStageDefaultColor(): TheaterPlateColor = when (this) {
    DetailPageKind.Movie -> TheaterPlateColor.rgb(15, 23, 42)
    DetailPageKind.Series -> TheaterPlateColor.rgb(23, 37, 84)
    DetailPageKind.Season -> TheaterPlateColor.rgb(30, 27, 75)
    DetailPageKind.Episode -> TheaterPlateColor.rgb(22, 78, 99)
    DetailPageKind.MissingDetail -> TheaterPlateColor.rgb(31, 41, 55)
}

private val DetailPageKind.label: String get() = when (this) {
    DetailPageKind.Movie -> "Movie"
    DetailPageKind.Series -> "Series"
    DetailPageKind.Season -> "Season"
    DetailPageKind.Episode -> "Episode"
    DetailPageKind.MissingDetail -> "Missing detail"
}

private fun DetailRailActivationPolicy.isSatisfiedBy(item: DetailRailItem): Boolean = when (this) {
    DetailRailActivationPolicy.Disabled -> false
    DetailRailActivationPolicy.Navigate -> item.route != null
    DetailRailActivationPolicy.Play -> item.playbackContract != null
}

private fun DetailRailActivationPolicy.activationLabel(
    mode: DetailSurfaceInteractionMode,
    activatable: Boolean,
): String = if (!activatable) {
    "Activation unavailable"
} else {
    when (this) {
        DetailRailActivationPolicy.Disabled -> "Activation unavailable"
        DetailRailActivationPolicy.Navigate -> "${mode.activationVerb} to open details"
        DetailRailActivationPolicy.Play -> "${mode.activationVerb} to play"
    }
}

private val DetailRailActivationPolicy.label: String get() = when (this) {
    DetailRailActivationPolicy.Disabled -> "Activation disabled"
    DetailRailActivationPolicy.Navigate -> "Opens details"
    DetailRailActivationPolicy.Play -> "Plays media"
}

private val DetailRailState.label: String get() = when (this) {
    DetailRailState.Available -> "Available"
    DetailRailState.Empty -> "Empty"
    DetailRailState.Unavailable -> "Unavailable"
}

private fun DetailRailItem.renderKey(occurrence: Int): String = if (occurrence == 0) {
    FerrexQaTags.segment(stableId)
} else {
    "${FerrexQaTags.segment(stableId)}-${occurrence + 1}"
}

private fun DetailRailItem.badges(): List<String> = buildList {
    badge?.takeIf { it.isNotBlank() }?.let(::add)
    progress?.let { add("${(it.coerceIn(0f, 1f) * 100f).roundToInt()}%") }
    addAll(art.imageState.badges())
}.distinct()

private fun DetailRailItem.progressLabel(): String? = progress?.let {
    "$title progress ${(it.coerceIn(0f, 1f) * 100f).roundToInt()}%"
}

private fun DetailImageState.badges(): List<String> = when (this) {
    is DetailImageState.Ready -> buildList {
        if (staleOffline) add("Stale/offline")
        offlineMessage?.let { add("Offline: $it") }
    }
    is DetailImageState.Pending -> buildList {
        add("Pending")
        if (staleOffline) add("Stale/offline")
    }
    is DetailImageState.Failed -> buildList {
        add("Failed")
        if (staleOffline) add("Stale/offline")
        if (retryable) add("Retryable")
    }
    is DetailImageState.NoArt -> listOf("Missing artwork")
}.filter { it.isNotBlank() }.distinct()

private fun fallbackLabel(art: DetailPageArt): String = when (val state = art.imageState) {
    is DetailImageState.Ready -> art.label
    is DetailImageState.Pending -> state.message
    is DetailImageState.Failed -> state.reason
    is DetailImageState.NoArt -> state.reason
}

private fun cardKindFor(role: DetailArtRole): DetailRailCardKind = when (role) {
    DetailArtRole.Poster -> DetailRailCardKind.Poster
    DetailArtRole.Profile -> DetailRailCardKind.Profile
    DetailArtRole.Backdrop,
    DetailArtRole.Still -> DetailRailCardKind.Still
    DetailArtRole.None -> DetailRailCardKind.Text
}

private fun DetailArtRole.detailGrounding(current: MediaArtGrounding?): MediaArtGrounding? = when (this) {
    DetailArtRole.Poster,
    DetailArtRole.Profile -> MediaArtGrounding.TheaterPlateContactShadow
    DetailArtRole.Backdrop,
    DetailArtRole.Still,
    DetailArtRole.None -> current
}

fun DetailRailCardKind.sizing(mode: DetailSurfaceInteractionMode): DetailMediaSizing {
    val tv = mode == DetailSurfaceInteractionMode.TvDpad
    return when (this) {
        DetailRailCardKind.Poster -> DetailMediaSizing(
            width = if (tv) FerrexDesignTokens.Poster.TvWidth else 132.dp,
            minHeight = if (tv) FerrexDesignTokens.Poster.TvCardMinHeight else 188.dp,
            maxHeight = if (tv) 360.dp else 240.dp,
            aspectRatio = FerrexDesignTokens.Poster.AspectRatio,
        )
        DetailRailCardKind.Still -> DetailMediaSizing(
            width = if (tv) 320.dp else 184.dp,
            minHeight = if (tv) 180.dp else 104.dp,
            maxHeight = if (tv) 220.dp else 132.dp,
            aspectRatio = 16f / 9f,
        )
        DetailRailCardKind.Profile -> DetailMediaSizing(
            width = if (tv) 168.dp else 112.dp,
            minHeight = if (tv) 168.dp else 112.dp,
            maxHeight = if (tv) 220.dp else 148.dp,
            aspectRatio = FerrexDesignTokens.Poster.AspectRatio,
        )
        DetailRailCardKind.Text -> DetailMediaSizing(
            width = if (tv) 280.dp else 188.dp,
            minHeight = if (tv) 110.dp else 84.dp,
            maxHeight = if (tv) 168.dp else 128.dp,
            aspectRatio = 16f / 10f,
        )
    }
}

private fun DetailArtRole.heroWidth(mode: DetailSurfaceInteractionMode): Dp = when (this) {
    DetailArtRole.Poster,
    DetailArtRole.Profile -> when (mode) {
        DetailSurfaceInteractionMode.TvDpad -> 240.dp
        DetailSurfaceInteractionMode.PhoneLandscapeTouch -> 196.dp
        DetailSurfaceInteractionMode.PhoneTouch -> 180.dp
    }
    DetailArtRole.Backdrop,
    DetailArtRole.Still -> when (mode) {
        DetailSurfaceInteractionMode.TvDpad -> 640.dp
        DetailSurfaceInteractionMode.PhoneLandscapeTouch -> 520.dp
        DetailSurfaceInteractionMode.PhoneTouch -> 360.dp
    }
    DetailArtRole.None -> when (mode) {
        DetailSurfaceInteractionMode.TvDpad -> 320.dp
        DetailSurfaceInteractionMode.PhoneLandscapeTouch -> 260.dp
        DetailSurfaceInteractionMode.PhoneTouch -> 200.dp
    }
}

private fun DetailTone.toStageTone(): FerrexStageSurfaceTone = when (this) {
    DetailTone.Neutral -> FerrexStageSurfaceTone.Neutral
    DetailTone.Accent,
    DetailTone.Success -> FerrexStageSurfaceTone.Primary
    DetailTone.Warning -> FerrexStageSurfaceTone.Warning
    DetailTone.Danger -> FerrexStageSurfaceTone.Error
    DetailTone.Muted -> FerrexStageSurfaceTone.StaleOffline
}

private fun DetailFreshnessKind.toStageTone(): FerrexStageSurfaceTone = when (this) {
    DetailFreshnessKind.Fresh -> FerrexStageSurfaceTone.Primary
    DetailFreshnessKind.Empty -> FerrexStageSurfaceTone.StaleOffline
    DetailFreshnessKind.Syncing -> FerrexStageSurfaceTone.Cache
    DetailFreshnessKind.StaleOffline -> FerrexStageSurfaceTone.StaleOffline
    DetailFreshnessKind.RecoverableError -> FerrexStageSurfaceTone.Warning
}

private fun metadataDescription(item: DetailMetadataItem): String = when (item.kind.name) {
    "WatchState" -> "Watch state ${item.label}"
    "AudienceRating" -> "Audience rating ${item.label}"
    "Recovery" -> "Recovery status ${item.label}"
    else -> item.label
}

private fun factDescription(item: DetailFactItem): String = if (item.value.isBlank()) item.label else "${item.label}: ${item.value}"

private fun DetailSurfaceInteractionMode.toTypographyDensity(): com.ferrex.android.ui.components.TheaterPlateDensityRole = when (this) {
    DetailSurfaceInteractionMode.PhoneTouch -> com.ferrex.android.ui.components.TheaterPlateDensityRole.PhonePortrait
    DetailSurfaceInteractionMode.PhoneLandscapeTouch -> com.ferrex.android.ui.components.TheaterPlateDensityRole.PhoneLandscape
    DetailSurfaceInteractionMode.TvDpad -> com.ferrex.android.ui.components.TheaterPlateDensityRole.Tv1080p
}

private fun DetailSurfaceInteractionMode.prefersWideHeroLayout(): Boolean = when (this) {
    DetailSurfaceInteractionMode.PhoneTouch -> false
    DetailSurfaceInteractionMode.PhoneLandscapeTouch,
    DetailSurfaceInteractionMode.TvDpad -> true
}
