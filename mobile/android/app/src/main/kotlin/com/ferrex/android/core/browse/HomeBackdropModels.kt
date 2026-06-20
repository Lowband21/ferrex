package com.ferrex.android.core.browse

import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution

private const val DEFAULT_HOME_BACKDROP_CANDIDATE_LIMIT = 8

data class HomeBackdropCandidate(
    val stableKey: String,
    val title: String,
    val backdropKey: ImageRequestKey,
    val fallbackPath: String?,
    val sourceSurface: BrowseSourceSurface,
)

enum class HomeBackdropStageStatus {
    Ready,
    Pending,
    Failed,
    NoBackdrop,
    StaleOffline,
}

data class HomeBackdropStageState(
    val status: HomeBackdropStageStatus,
    val candidate: HomeBackdropCandidate?,
    val readyResolution: ImageResolution.Ready?,
    val retryAfterMillis: Long? = null,
    val failedReasons: List<String> = emptyList(),
) {
    val isRenderable: Boolean get() = readyResolution != null
}

object HomeBackdropModels {
    fun candidatesFromShelves(
        shelves: List<HomeShelf>,
        limit: Int = DEFAULT_HOME_BACKDROP_CANDIDATE_LIMIT,
    ): List<HomeBackdropCandidate> = candidatesFromCards(
        cards = shelves.flatMap { it.items },
        limit = limit,
    )

    fun candidatesFromCards(
        cards: List<LibraryMediaCard>,
        limit: Int = DEFAULT_HOME_BACKDROP_CANDIDATE_LIMIT,
    ): List<HomeBackdropCandidate> {
        if (limit <= 0) return emptyList()
        val seen = LinkedHashSet<String>()
        val candidates = ArrayList<HomeBackdropCandidate>(limit)
        for (card in cards) {
            val key = card.backdropKey ?: continue
            if (!seen.add(key.cacheKey)) continue
            candidates += HomeBackdropCandidate(
                stableKey = card.stableKey,
                title = card.title,
                backdropKey = key,
                fallbackPath = card.backdropFallbackPath,
                sourceSurface = card.route.sourceSurface,
            )
            if (candidates.size >= limit) break
        }
        return candidates
    }

    fun keys(candidates: List<HomeBackdropCandidate>): List<ImageRequestKey> = candidates.map { it.backdropKey }

    fun resolveStage(
        candidates: List<HomeBackdropCandidate>,
        resolutions: Map<ImageRequestKey, ImageResolution>,
        forceStaleOffline: Boolean = false,
    ): HomeBackdropStageState {
        if (candidates.isEmpty()) {
            return HomeBackdropStageState(
                status = HomeBackdropStageStatus.NoBackdrop,
                candidate = null,
                readyResolution = null,
            )
        }

        var firstStaleReady: Pair<HomeBackdropCandidate, ImageResolution.Ready>? = null
        var firstPendingCandidate: HomeBackdropCandidate? = null
        var firstPendingRetryAfterMillis: Long? = null
        val failedReasons = mutableListOf<String>()

        for (candidate in candidates) {
            when (val resolution = resolutions.resolutionFor(candidate.backdropKey)) {
                is ImageResolution.Ready -> {
                    if (resolution.stale || forceStaleOffline) {
                        if (firstStaleReady == null) firstStaleReady = candidate to resolution
                    } else {
                        return HomeBackdropStageState(
                            status = HomeBackdropStageStatus.Ready,
                            candidate = candidate,
                            readyResolution = resolution,
                        )
                    }
                }
                is ImageResolution.Pending -> {
                    if (firstPendingCandidate == null) {
                        firstPendingCandidate = candidate
                        firstPendingRetryAfterMillis = resolution.retryAfterMillis
                    }
                }
                is ImageResolution.Failed -> {
                    failedReasons += resolution.reason
                    if (resolution.retryable && firstPendingCandidate == null) {
                        firstPendingCandidate = candidate
                    }
                }
                is ImageResolution.Placeholder,
                null -> {
                    if (firstPendingCandidate == null) {
                        firstPendingCandidate = candidate
                    }
                }
            }
        }

        firstStaleReady?.let { (candidate, resolution) ->
            return HomeBackdropStageState(
                status = HomeBackdropStageStatus.StaleOffline,
                candidate = candidate,
                readyResolution = resolution,
            )
        }

        firstPendingCandidate?.let { candidate ->
            return HomeBackdropStageState(
                status = HomeBackdropStageStatus.Pending,
                candidate = candidate,
                readyResolution = null,
                retryAfterMillis = firstPendingRetryAfterMillis,
                failedReasons = failedReasons,
            )
        }

        return HomeBackdropStageState(
            status = HomeBackdropStageStatus.Failed,
            candidate = candidates.firstOrNull(),
            readyResolution = null,
            failedReasons = failedReasons,
        )
    }
}

private fun Map<ImageRequestKey, ImageResolution>.resolutionFor(key: ImageRequestKey): ImageResolution? =
    this[key] ?: entries.firstOrNull { it.key.cacheKey == key.cacheKey }?.value
