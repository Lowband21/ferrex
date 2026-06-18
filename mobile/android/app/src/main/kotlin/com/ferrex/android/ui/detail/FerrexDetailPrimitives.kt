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
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.mediaart.MediaArtFallbackPolicy
import com.ferrex.android.core.mediaart.MediaArtFitPolicy
import com.ferrex.android.core.mediaart.MediaArtGrounding
import com.ferrex.android.core.mediaart.runtimeFallback
import com.ferrex.android.core.playback.PlaybackRouteContract
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
    val media: DetailMediaPresentation,
)

@Immutable
data class DetailRailPresentation(
    val stableKey: String,
    val testTag: String,
    val title: String,
    val stateLabel: String,
    val activationPolicyLabel: String,
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
        return DetailStagePresentation(
            stableKey = page.stableKey,
            testTag = FerrexQaTags.TheaterPlate.root(mode.targetKey, page.stableKey),
            contentDescription = buildString {
                append(page.kind.label)
                append(" detail for ")
                append(page.title)
                page.subtitle?.let { append(". ").append(it) }
            },
            density = mode.density,
            heroMedia = heroMedia,
            metadataBand = metadataBand(page, mode),
            actionShelf = actionShelf(page.stableKey, page.actions, mode),
            slabs = slabs(page, mode),
            rails = page.rails.map { rail(page.stableKey, it, mode) },
        )
    }

    fun actionShelf(
        pageKey: String,
        actions: List<DetailPageAction>,
        mode: DetailSurfaceInteractionMode,
    ): DetailActionShelfPresentation = DetailActionShelfPresentation(
        testTag = FerrexQaTags.TheaterPlate.action(mode.targetKey, pageKey, "shelf"),
        contentDescription = "Detail actions. ${actions.size} action${if (actions.size == 1) "" else "s"} available.",
        actions = actions.map { action(pageKey, it, mode) },
    )

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
                        tone = when (watch.state.name) {
                            "Watched" -> FerrexStageSurfaceTone.Primary
                            "InProgress" -> FerrexStageSurfaceTone.Cache
                            "Unavailable" -> FerrexStageSurfaceTone.Warning
                            else -> FerrexStageSurfaceTone.Neutral
                        },
                        contentDescription = watch.message,
                    ),
                )
            }
        }
        return DetailMetadataBandPresentation(
            testTag = FerrexQaTags.namespaced(mode.targetKey, "detail", page.stableKey, "metadata"),
            contentDescription = "Metadata band for ${page.title}. ${chips.size} item${if (chips.size == 1) "" else "s"}.",
            chips = chips,
        )
    }

    fun slabs(page: DetailPageModel, mode: DetailSurfaceInteractionMode): List<DetailSlabPresentation> = buildList {
        page.emptyState?.let { add(emptySlab(page.stableKey, it, mode)) }
        page.recovery.freshness?.let { add(freshnessSlab(page.stableKey, it, page.recovery.actions, mode)) }
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
        return DetailRailPresentation(
            stableKey = rail.stableKey,
            testTag = FerrexQaTags.TheaterPlate.rail(mode.targetKey, pageKey, rail.stableKey),
            title = rail.title,
            stateLabel = stateLabel,
            activationPolicyLabel = activationPolicyLabel,
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
                append(".")
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
                append(". ").append(activationLabel)
            },
            activationLabel = activationLabel,
            activatable = activatable,
            badges = badges,
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
            contentDescription = buildString {
                append(art.label)
                if (badges.isNotEmpty()) append(". ").append(badges.joinToString(". "))
            },
            fallbackLabel = fallbackLabel(art),
            sizing = sizing,
            badges = badges,
            fitPolicy = art.mediaArt?.treatment?.fitPolicy,
            grounding = detailGrounding,
            stateLabel = art.imageState.label,
        )
    }

    private fun emptySlab(pageKey: String, empty: DetailEmptyState, mode: DetailSurfaceInteractionMode): DetailSlabPresentation =
        DetailSlabPresentation(
            testTag = FerrexQaTags.namespaced(mode.targetKey, "theater-plate", "status", pageKey, "empty"),
            title = empty.title,
            message = empty.message,
            tone = FerrexStageSurfaceTone.StaleOffline,
            contentDescription = "${empty.title}. ${empty.message}",
            actions = emptyList(),
        )

    private fun freshnessSlab(
        pageKey: String,
        freshness: DetailFreshnessNotice,
        recoveryActions: List<DetailPageAction>,
        mode: DetailSurfaceInteractionMode,
    ): DetailSlabPresentation = DetailSlabPresentation(
        testTag = FerrexQaTags.namespaced(mode.targetKey, "theater-plate", "status", pageKey, freshness.kind.name),
        title = freshness.title,
        message = freshness.message,
        tone = freshness.kind.toStageTone(),
        contentDescription = "${freshness.title}. ${freshness.message}",
        actions = recoveryActions.map { action(pageKey, it, mode) },
    )
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
) {
    val presentation = remember(page, interactionMode) { DetailPrimitivePresenter.stage(page, interactionMode) }
    LazyColumn(
        modifier = modifier
            .fillMaxSize()
            .background(FerrexDesignTokens.Palette.SlateCanvas)
            .testTag(presentation.testTag)
            .semantics { contentDescription = presentation.contentDescription },
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg),
        contentPadding = PaddingValues(
            horizontal = if (interactionMode == DetailSurfaceInteractionMode.TvDpad) {
                FerrexDesignTokens.Space.ScreenTvHorizontal
            } else {
                FerrexDesignTokens.Space.ScreenPhoneHorizontal
            },
            vertical = if (interactionMode == DetailSurfaceInteractionMode.TvDpad) {
                FerrexDesignTokens.Space.ScreenTvVertical
            } else {
                FerrexDesignTokens.Space.ScreenPhoneVertical
            },
        ),
    ) {
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
        val foreground = page.hero.foreground
        val heroArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Lg)
        if (interactionMode == DetailSurfaceInteractionMode.TvDpad && foreground != null) {
            Row(horizontalArrangement = heroArrangement, verticalAlignment = Alignment.Top) {
                FerrexDetailMediaObject(
                    art = foreground,
                    presentation = presentation.heroMedia.first { it.role == foreground.role },
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    serverUrl = serverUrl,
                    fallbackPolicy = fallbackPolicy,
                    modifier = Modifier.width(foreground.role.heroWidth(interactionMode)),
                )
                DetailHeroCopy(page = page, interactionMode = interactionMode, modifier = Modifier.weight(1f))
            }
        } else {
            Column(verticalArrangement = heroArrangement) {
                FerrexDetailMediaObject(
                    art = foreground ?: page.hero.background,
                    presentation = presentation.heroMedia.last(),
                    imageResolutions = imageResolutions,
                    imageLoader = imageLoader,
                    serverUrl = serverUrl,
                    fallbackPolicy = fallbackPolicy,
                    modifier = Modifier.widthIn(max = (foreground ?: page.hero.background).role.heroWidth(interactionMode)),
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
                DetailProgressBar(progress = item.progress, label = "${item.title} progress")
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
            maxLines = if (interactionMode == DetailSurfaceInteractionMode.TvDpad) 2 else 3,
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
    DetailArtRole.Profile -> if (mode == DetailSurfaceInteractionMode.TvDpad) 240.dp else 180.dp
    DetailArtRole.Backdrop,
    DetailArtRole.Still -> if (mode == DetailSurfaceInteractionMode.TvDpad) 640.dp else 360.dp
    DetailArtRole.None -> if (mode == DetailSurfaceInteractionMode.TvDpad) 320.dp else 200.dp
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
    DetailSurfaceInteractionMode.TvDpad -> com.ferrex.android.ui.components.TheaterPlateDensityRole.Tv1080p
}
