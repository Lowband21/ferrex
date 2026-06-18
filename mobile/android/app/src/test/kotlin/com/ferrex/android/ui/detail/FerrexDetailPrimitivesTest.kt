package com.ferrex.android.ui.detail

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.detail.DetailActionRole
import com.ferrex.android.core.detail.DetailArtRole
import com.ferrex.android.core.detail.DetailEmptyState
import com.ferrex.android.core.detail.DetailFactItem
import com.ferrex.android.core.detail.DetailFreshnessKind
import com.ferrex.android.core.detail.DetailFreshnessNotice
import com.ferrex.android.core.detail.DetailHero
import com.ferrex.android.core.detail.DetailImagePrefetchPlan
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
import com.ferrex.android.core.detail.DetailRailKind
import com.ferrex.android.core.detail.DetailRailState
import com.ferrex.android.core.detail.DetailRecoveryState
import com.ferrex.android.core.detail.DetailTone
import com.ferrex.android.core.detail.DetailWatchState
import com.ferrex.android.core.detail.DetailWatchStateKind
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.mediaart.MediaArtFitPolicy
import com.ferrex.android.core.mediaart.MediaArtGrounding
import com.ferrex.android.core.mediaart.MediaArtObject
import com.ferrex.android.core.mediaart.MediaArtRequest
import com.ferrex.android.core.mediaart.MediaArtTargetIdentity
import com.ferrex.android.core.playback.PlaybackRouteContract
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FerrexDetailPrimitivesTest {
    @Test
    fun phoneAndTvPresentationsShareTheDetailModelWithModeSpecificSizingAndTags() {
        val page = page(
            actions = listOf(
                DetailPageAction(
                    kind = DetailPageActionKind.Play,
                    label = "Play",
                    role = DetailActionRole.Primary,
                    playbackContract = playbackContract("movie"),
                ),
            ),
            rails = listOf(playRail()),
        )

        val phone = DetailPrimitivePresenter.stage(page, DetailSurfaceInteractionMode.PhoneTouch)
        val tv = DetailPrimitivePresenter.stage(page, DetailSurfaceInteractionMode.TvDpad)

        assertEquals(page.stableKey, phone.stableKey)
        assertEquals(page.stableKey, tv.stableKey)
        assertTrue(phone.testTag.startsWith("phone.theater-plate"))
        assertTrue(tv.testTag.startsWith("tv.theater-plate"))
        assertTrue(tv.heroMedia.first().sizing.width > phone.heroMedia.first().sizing.width)
        assertEquals("Tap to play", phone.rails.single().items.single().activationLabel)
        assertEquals("Press Select to play", tv.rails.single().items.single().activationLabel)
    }

    @Test
    fun mediaPresentationPreservesTheaterPlateArtTreatmentAndFallbackLabels() {
        val poster = art(
            role = DetailArtRole.Poster,
            category = BrowseImageCategory.Poster,
            label = "Cache Movie poster",
            state = DetailImageState.Failed("failed", staleOffline = false, reason = "decode failed", retryable = true),
            grounding = MediaArtGrounding.TheaterPlateContactShadow,
        )
        val backdrop = art(
            role = DetailArtRole.Backdrop,
            category = BrowseImageCategory.Backdrop,
            label = "Cache Movie backdrop",
            state = DetailImageState.Ready("ready", staleOffline = true, offlineMessage = "server unreachable"),
            grounding = MediaArtGrounding.Flat,
        )
        val profile = art(
            role = DetailArtRole.Profile,
            category = BrowseImageCategory.Profile,
            label = "Actor profile",
            state = DetailImageState.Ready("ready", staleOffline = false),
            grounding = MediaArtGrounding.CardObject,
        )

        val posterPresentation = DetailPrimitivePresenter.media("movie:1", poster, DetailSurfaceInteractionMode.PhoneTouch, "poster")
        val backdropPresentation = DetailPrimitivePresenter.media("movie:1", backdrop, DetailSurfaceInteractionMode.TvDpad, "backdrop")
        val profilePresentation = DetailPrimitivePresenter.media("movie:1", profile, DetailSurfaceInteractionMode.TvDpad, "profile")

        assertEquals(MediaArtFitPolicy.Contain, posterPresentation.fitPolicy)
        assertEquals(MediaArtGrounding.TheaterPlateContactShadow, posterPresentation.grounding)
        assertEquals("decode failed", posterPresentation.fallbackLabel)
        assertTrue(posterPresentation.badges.contains("Failed"))
        assertTrue(posterPresentation.badges.contains("Retryable"))

        assertEquals(MediaArtFitPolicy.ArtDirectedCrop, backdropPresentation.fitPolicy)
        assertEquals(MediaArtGrounding.Flat, backdropPresentation.grounding)
        assertTrue(backdropPresentation.badges.contains("Stale/offline"))
        assertTrue(backdropPresentation.contentDescription.contains("Offline: server unreachable"))

        assertEquals(MediaArtFitPolicy.Contain, profilePresentation.fitPolicy)
        assertEquals(MediaArtGrounding.TheaterPlateContactShadow, profilePresentation.grounding)
    }

    @Test
    fun railsExposeStableTagsActivationPolicyAndImageStateBadges() {
        val rail = DetailRail(
            stableKey = "episodes:season-1",
            kind = DetailRailKind.Episodes,
            title = "Episodes",
            state = DetailRailState.Available,
            cardKind = DetailRailCardKind.Still,
            activationPolicy = DetailRailActivationPolicy.Play,
            items = listOf(
                railItem("episode-1", DetailImageState.Pending("pending", staleOffline = true, retryAfterMillis = 5_000)),
                railItem("episode-1", DetailImageState.Failed("failed", staleOffline = false, reason = "manifest failed", retryable = true)),
                railItem("episode-3", DetailImageState.NoArt("missing", "No still cached"), playback = null),
                railItem("episode-4", DetailImageState.Ready("ready", staleOffline = true, offlineMessage = null)),
            ),
        )

        val presentation = DetailPrimitivePresenter.rail("series:1", rail, DetailSurfaceInteractionMode.TvDpad)
        val allBadges = presentation.items.flatMap { it.badges }

        assertEquals("Plays media", presentation.activationPolicyLabel)
        assertEquals("episode-1", presentation.items[0].renderKey)
        assertEquals("episode-1-2", presentation.items[1].renderKey)
        assertTrue(presentation.items[0].testTag.contains("episodes-season-1.episode-1"))
        assertTrue(presentation.items[1].testTag.contains("episodes-season-1.episode-1-2"))
        assertTrue(allBadges.contains("Pending"))
        assertTrue(allBadges.contains("Failed"))
        assertTrue(allBadges.contains("Missing artwork"))
        assertTrue(allBadges.contains("Stale/offline"))
        assertTrue(presentation.items[0].activatable)
        assertFalse(presentation.items[2].activatable)
        assertTrue(presentation.items[2].contentDescription.contains("Activation unavailable"))
        assertTrue(presentation.virtualized)
    }

    @Test
    fun actionShelfLabelsDisabledActionsWithoutDroppingAvailability() {
        val actions = listOf(
            DetailPageAction(
                kind = DetailPageActionKind.Resume,
                label = "Resume",
                role = DetailActionRole.Primary,
                enabled = false,
                disabledReason = "Reconnect before playback.",
                playbackContract = playbackContract("movie"),
            ),
            DetailPageAction(
                kind = DetailPageActionKind.ResetConnection,
                label = "Reset connection",
                role = DetailActionRole.DestructiveReset,
            ),
        )

        val shelf = DetailPrimitivePresenter.actionShelf("movie:1", actions, DetailSurfaceInteractionMode.PhoneTouch)

        assertEquals(2, shelf.actions.size)
        assertFalse(shelf.actions[0].enabled)
        assertTrue(shelf.actions[0].contentDescription.contains("Disabled: Reconnect before playback."))
        assertTrue(shelf.actions[0].testTag.contains("resume"))
        assertTrue(shelf.actions[1].contentDescription.contains("Destructive reset action"))
    }

    @Test
    fun statusAndRecoverySlabsPreserveFallbackStatesAndRecoveryActions() {
        val page = page(
            emptyState = DetailEmptyState("No detail cached", "Retry cache sync to recover."),
            recovery = DetailRecoveryState(
                freshness = DetailFreshnessNotice(
                    kind = DetailFreshnessKind.StaleOffline,
                    title = "Offline cache",
                    message = "Showing stale library data.",
                ),
                actions = listOf(
                    DetailPageAction(DetailPageActionKind.RetryCache, "Retry cache sync", DetailActionRole.Retry),
                    DetailPageAction(DetailPageActionKind.Diagnostics, "Diagnostics / Export diagnostics", DetailActionRole.Diagnostics),
                ),
            ),
        )

        val slabs = DetailPrimitivePresenter.stage(page, DetailSurfaceInteractionMode.TvDpad).slabs

        assertEquals(2, slabs.size)
        assertTrue(slabs[0].contentDescription.contains("Retry cache sync to recover."))
        assertTrue(slabs[1].contentDescription.contains("Showing stale library data."))
        assertEquals(2, slabs[1].actions.size)
        assertTrue(slabs[1].actions.any { it.label == "Diagnostics / Export diagnostics" })
    }

    @Test
    fun tvStageSummarizesDetailSurfacesAndImageRecoveryStates() {
        val page = page(
            actions = listOf(
                DetailPageAction(
                    kind = DetailPageActionKind.Play,
                    label = "Play",
                    role = DetailActionRole.Primary,
                    enabled = false,
                    disabledReason = "Reconnect before playback.",
                    playbackContract = playbackContract("movie"),
                ),
            ),
            rails = listOf(playRail()),
            recovery = DetailRecoveryState(
                freshness = DetailFreshnessNotice(
                    kind = DetailFreshnessKind.StaleOffline,
                    title = "Offline cache",
                    message = "Showing stale library data.",
                ),
                actions = fullRecoveryActions(),
            ),
            watchState = DetailWatchState(
                scopeKey = "movie",
                label = "Movie watch state",
                state = DetailWatchStateKind.Unknown,
                progress = 0f,
                pendingMutation = false,
                message = "Watch state has not loaded yet.",
            ),
            heroBackgroundState = DetailImageState.Failed("failed", staleOffline = true, reason = "decode failed", retryable = true),
            heroForegroundState = DetailImageState.NoArt("missing", "No poster cached"),
        )

        val stage = DetailPrimitivePresenter.stage(page, DetailSurfaceInteractionMode.TvDpad)

        assertEquals(DetailSurfaceInteractionMode.TvDpad.density, stage.density)
        assertTrue(stage.contentDescription.contains("Hero media:"))
        assertTrue(stage.contentDescription.contains("Media objects:"))
        assertTrue(stage.contentDescription.contains("Metadata:"))
        assertTrue(stage.contentDescription.contains("Actions:"))
        assertTrue(stage.contentDescription.contains("Status slabs:"))
        assertTrue(stage.heroMedia.any { it.grounding == MediaArtGrounding.TheaterPlateContactShadow })
        assertTrue(stage.actionShelf.contentDescription.contains("Play disabled: Reconnect before playback."))

        val slabTitles = stage.slabs.map { it.title }
        assertTrue(slabTitles.contains("Movie watch state"))
        assertTrue(slabTitles.contains("Missing artwork"))
        assertTrue(slabTitles.contains("Image load failed"))
        assertTrue(slabTitles.contains("Stale/offline artwork"))

        val imageSlabActions = stage.slabs.first { it.title == "Image load failed" }.actions.map { it.label }
        assertTrue(imageSlabActions.contains("Back"))
        assertTrue(imageSlabActions.contains("Retry cache sync"))
        assertTrue(imageSlabActions.contains("Clear selected cache"))
        assertTrue(imageSlabActions.contains("Change server"))
        assertTrue(imageSlabActions.contains("Reset connection"))
        assertTrue(imageSlabActions.contains("Diagnostics / Export diagnostics"))
        assertTrue(stage.slabs.first { it.title == "Movie watch state" }.actions.any { it.label == "Back" })
    }

    private fun page(
        actions: List<DetailPageAction> = emptyList(),
        rails: List<DetailRail> = emptyList(),
        emptyState: DetailEmptyState? = null,
        recovery: DetailRecoveryState = DetailRecoveryState(freshness = null, actions = emptyList()),
        watchState: DetailWatchState? = null,
        heroBackgroundState: DetailImageState = DetailImageState.Ready("ready", staleOffline = false),
        heroForegroundState: DetailImageState? = DetailImageState.Pending("queued", staleOffline = false),
    ): DetailPageModel = DetailPageModel(
        stableKey = "movie:1",
        kind = DetailPageKind.Movie,
        route = null,
        title = "Cache Movie",
        subtitle = "A cached story",
        overview = "A detail model shared by phone and TV primitives.",
        hero = DetailHero(
            background = art(
                role = DetailArtRole.Backdrop,
                category = BrowseImageCategory.Backdrop,
                label = "Cache Movie backdrop",
                state = heroBackgroundState,
                grounding = MediaArtGrounding.Flat,
            ),
            foreground = heroForegroundState?.let { state ->
                art(
                    role = DetailArtRole.Poster,
                    category = BrowseImageCategory.Poster,
                    label = "Cache Movie poster",
                    state = state,
                    grounding = MediaArtGrounding.TheaterPlateContactShadow,
                )
            },
        ),
        metadata = listOf(DetailMetadataItem("PG-13", tone = DetailTone.Neutral)),
        facts = listOf(DetailFactItem("Runtime", "95 min", tone = DetailTone.Accent)),
        watchState = watchState,
        actions = actions,
        recovery = recovery,
        rails = rails,
        emptyState = emptyState,
        imagePrefetch = DetailImagePrefetchPlan(keys = emptySet(), visibleRailItemWindow = 0, maxImageKeys = 0),
    )

    private fun playRail(
        itemState: DetailImageState = DetailImageState.Ready("ready", staleOffline = false),
    ): DetailRail = DetailRail(
        stableKey = "related",
        kind = DetailRailKind.Recommendations,
        title = "Related",
        state = DetailRailState.Available,
        cardKind = DetailRailCardKind.Poster,
        activationPolicy = DetailRailActivationPolicy.Play,
        items = listOf(railItem("related-1", itemState)),
    )

    private fun railItem(
        stableId: String,
        state: DetailImageState,
        playback: PlaybackRouteContract? = playbackContract(stableId),
    ): DetailRailItem = DetailRailItem(
        stableId = stableId,
        title = "Episode $stableId",
        subtitle = "42 min",
        badge = null,
        progress = null,
        art = art(
            role = DetailArtRole.Still,
            category = BrowseImageCategory.Episode,
            label = "Episode $stableId still",
            state = state,
            grounding = MediaArtGrounding.CardObject,
        ),
        playbackContract = playback,
    )

    private fun art(
        role: DetailArtRole,
        category: BrowseImageCategory,
        label: String,
        state: DetailImageState,
        grounding: MediaArtGrounding,
    ): DetailPageArt {
        val key = ImageRequestKey("${category.name.lowercase()}-${label.hashCode()}", category)
        return DetailPageArt(
            role = role,
            label = label,
            mediaArt = MediaArtObject.forCategory(
                category = category,
                request = MediaArtRequest(key = key, publicFallbackPath = "/${key.iid}.jpg"),
                fallbackLabel = "$label unavailable",
                targetIdentity = MediaArtTargetIdentity(
                    surfaceKey = "detail-test",
                    itemKey = label,
                    semanticLabel = label,
                ),
                grounding = grounding,
            ),
            imageState = state,
        )
    }

    private fun fullRecoveryActions(): List<DetailPageAction> = listOf(
        DetailPageAction(DetailPageActionKind.Back, "Back", DetailActionRole.Back),
        DetailPageAction(DetailPageActionKind.RetryCache, "Retry cache sync", DetailActionRole.Retry),
        DetailPageAction(
            kind = DetailPageActionKind.ClearSelectedCache,
            label = "Clear selected cache",
            role = DetailActionRole.Cache,
            targetId = "library",
        ),
        DetailPageAction(DetailPageActionKind.ChangeServer, "Change server", DetailActionRole.Secondary),
        DetailPageAction(DetailPageActionKind.ResetConnection, "Reset connection", DetailActionRole.DestructiveReset),
        DetailPageAction(DetailPageActionKind.Diagnostics, "Diagnostics / Export diagnostics", DetailActionRole.Diagnostics),
    )

    private fun playbackContract(id: String): PlaybackRouteContract = PlaybackRouteContract(
        targetMediaId = "file-$id",
        logicalMediaId = id,
        mediaType = BrowseMediaType.Movie,
        startPositionSeconds = null,
        startOver = true,
        sourceDetailRoute = "detail/$id",
    )
}
