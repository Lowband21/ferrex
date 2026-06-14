package com.ferrex.android.core.tvfocus

/** Stable identity for a focusable TV target. */
data class TvFocusKey(
    val screen: String,
    val surface: String,
    val item: String,
) {
    init {
        require(screen.isNotBlank()) { "screen must not be blank" }
        require(surface.isNotBlank()) { "surface must not be blank" }
        require(item.isNotBlank()) { "item must not be blank" }
    }

    val surfaceKey: String = "$screen:$surface"
    val stableKey: String = "$screen:$surface:$item"
}

data class TvFocusRestoreRequest(
    val target: TvFocusKey,
    val usedFallback: Boolean,
)

/**
 * Pure, deterministic focus memory for Android TV surfaces.
 *
 * The model stores the last focused item per screen/surface pair and resolves it
 * only when the item is still available. Otherwise it returns the caller-provided
 * fallback target so composables can safely request focus after data changes.
 */
data class TvFocusRestoreState(
    private val focusedBySurface: Map<String, TvFocusKey> = emptyMap(),
    private val lastFocused: TvFocusKey? = null,
) {
    fun record(target: TvFocusKey): TvFocusRestoreState = copy(
        focusedBySurface = focusedBySurface + (target.surfaceKey to target),
        lastFocused = target,
    )

    fun restore(
        screen: String,
        surface: String,
        availableItems: Collection<String>,
        fallbackItem: String,
    ): TvFocusRestoreRequest {
        require(screen.isNotBlank()) { "screen must not be blank" }
        require(surface.isNotBlank()) { "surface must not be blank" }
        require(fallbackItem.isNotBlank()) { "fallbackItem must not be blank" }

        val available = availableItems.filter { it.isNotBlank() }.toSet()
        val fallback = TvFocusKey(screen = screen, surface = surface, item = fallbackItem)
        val remembered = focusedBySurface[fallback.surfaceKey]
            ?.takeIf { it.item in available }

        return if (remembered != null) {
            TvFocusRestoreRequest(target = remembered, usedFallback = false)
        } else {
            TvFocusRestoreRequest(target = fallback, usedFallback = true)
        }
    }

    fun forgetScreen(screen: String): TvFocusRestoreState {
        require(screen.isNotBlank()) { "screen must not be blank" }
        return copy(
            focusedBySurface = focusedBySurface.filterKeys { !it.startsWith("$screen:") },
            lastFocused = lastFocused?.takeUnless { it.screen == screen },
        )
    }

    fun lastTarget(screen: String): TvFocusKey? {
        require(screen.isNotBlank()) { "screen must not be blank" }
        return lastFocused?.takeIf { it.screen == screen }
    }

    fun rememberedTarget(screen: String, surface: String): TvFocusKey? {
        require(screen.isNotBlank()) { "screen must not be blank" }
        require(surface.isNotBlank()) { "surface must not be blank" }
        return focusedBySurface["$screen:$surface"]
    }
}
