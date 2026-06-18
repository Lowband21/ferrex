package com.ferrex.android.core.tvfocus

/**
 * Deterministic focus surfaces for Android TV detail pages.
 *
 * Each detail page gets an isolated focus memory scope so opening a new movie/series/episode starts
 * on the safe Back target, while watch actions and media rails can restore their last focused item
 * during state refreshes on the same page.
 */
object TvDetailFocusPolicy {
    private const val SCREEN_PREFIX = "detail"

    const val SURFACE_BACK = "detail-back"
    const val SURFACE_ACTIONS = "detail-actions"
    const val SURFACE_CONNECTION = "detail-connection"
    const val SURFACE_RECOVERY_PREFIX = "detail-recovery"
    const val SURFACE_RAIL_PREFIX = "detail-rail"
    const val ITEM_BACK = "back"

    fun screen(pageStableKey: String): String = "$SCREEN_PREFIX:${pageStableKey.ifBlank { "loading" }}"

    fun initialDetailTarget(pageStableKey: String): TvFocusKey = TvFocusKey(
        screen = screen(pageStableKey),
        surface = SURFACE_BACK,
        item = ITEM_BACK,
    )

    fun recoverySurface(stateKey: String): String = "$SURFACE_RECOVERY_PREFIX:$stateKey"

    fun railSurface(railStableKey: String): String = "$SURFACE_RAIL_PREFIX:$railStableKey"
}
