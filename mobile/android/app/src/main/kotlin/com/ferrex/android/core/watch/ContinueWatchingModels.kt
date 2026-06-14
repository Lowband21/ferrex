package com.ferrex.android.core.watch

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.library.toJavaUuidOrNull
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class ContinueWatchingApiItem(
    @SerialName("media_id") val mediaId: String,
    @SerialName("media_type") val mediaType: String,
    @SerialName("card_media_id") val cardMediaId: String,
    @SerialName("action_target") val actionTarget: ContinueWatchingActionTarget,
    @SerialName("action_hint") val actionHint: String,
    val position: Float = 0f,
    val duration: Float = 0f,
    @SerialName("last_watched") val lastWatched: Long = 0L,
    val title: String,
    val subtitle: String? = null,
    @SerialName("poster_iid") val posterIid: String? = null,
)

@Serializable
data class ContinueWatchingActionTarget(
    @SerialName("media_id") val mediaId: String,
    @SerialName("media_type") val mediaType: String,
)

data class ContinueWatchingCard(
    val stableKey: String,
    val title: String,
    val subtitle: String,
    val progressLabel: String,
    val imageKey: ImageRequestKey?,
    val route: MediaRouteArgs,
)

sealed interface ContinueWatchingStatus {
    data object Idle : ContinueWatchingStatus
    data object Loading : ContinueWatchingStatus
    data object Empty : ContinueWatchingStatus
    data class Fresh(val itemCount: Int) : ContinueWatchingStatus
    data class StaleOffline(val message: String, val itemCount: Int) : ContinueWatchingStatus
    data class ErrorRetryable(val message: String) : ContinueWatchingStatus

    val label: String
        get() = when (this) {
            Idle -> "idle"
            Loading -> "loading"
            Empty -> "empty"
            is Fresh -> "fresh"
            is StaleOffline -> "stale-offline"
            is ErrorRetryable -> "error"
        }
}

data class ContinueWatchingState(
    val status: ContinueWatchingStatus = ContinueWatchingStatus.Idle,
    val cards: List<ContinueWatchingCard> = emptyList(),
)

object ContinueWatchingMapper {
    fun toCard(item: ContinueWatchingApiItem): ContinueWatchingCard {
        val actionType = BrowseMediaType.fromApi(item.actionTarget.mediaType)
        val imageKey = item.posterIid?.takeIf { it.toJavaUuidOrNull() != null }?.let {
            ImageRequestKey(it, BrowseImageCategory.Poster)
        }
        return ContinueWatchingCard(
            stableKey = "continue:${item.cardMediaId}:${item.actionTarget.mediaId}",
            title = item.title,
            subtitle = item.subtitle ?: actionCopy(item.actionHint, item.mediaType),
            progressLabel = progressCopy(item.position, item.duration, item.actionHint),
            imageKey = imageKey,
            route = MediaRouteArgs(
                mediaType = actionType,
                mediaId = item.actionTarget.mediaId,
                libraryId = null,
                sourceSurface = BrowseSourceSurface.HomeContinueWatching,
            ),
        )
    }

    private fun actionCopy(actionHint: String, mediaType: String): String = when (actionHint) {
        "next_episode" -> "Next episode"
        "resume" -> "Resume ${mediaType.lowercase()}"
        else -> "Continue ${mediaType.lowercase()}"
    }

    private fun progressCopy(position: Float, duration: Float, actionHint: String): String {
        if (actionHint == "next_episode" && duration <= 0f) return "Next episode"
        if (duration <= 0f) return "Ready to play"
        val percent = ((position / duration).coerceIn(0f, 1f) * 100f).toInt()
        val remaining = ((duration - position).coerceAtLeast(0f) / 60f).toInt()
        return "$percent% watched • ${remaining} min left"
    }
}
