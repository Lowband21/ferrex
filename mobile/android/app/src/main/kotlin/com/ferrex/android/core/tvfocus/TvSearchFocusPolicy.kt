package com.ferrex.android.core.tvfocus

/** Stable focus and row identity helpers for Android TV search. */
object TvSearchFocusPolicy {
    const val SCREEN_SEARCH = "search"

    const val SURFACE_FIELD = "search-field"
    const val SURFACE_ACTIONS = "search-actions"
    const val SURFACE_RESULTS = "search-results"
    const val SURFACE_RESULTS_RECOVERY = "search-results-recovery"
    const val SURFACE_CACHE_MISS_PREFIX = "search-cache-miss"

    const val ITEM_QUERY = "query"

    fun resolvedRowKey(typeSegment: String, id: String, libraryId: String?): String = listOf(
        "resolved",
        typeSegment.ifBlank { "media" },
        id.ifBlank { "unknown" },
        libraryId?.takeIf { it.isNotBlank() } ?: "library-unknown",
    ).joinToString(":")

    fun cacheMissRowKey(typeSegment: String, id: String): String = listOf(
        "miss",
        typeSegment.ifBlank { "media" },
        id.ifBlank { "unknown" },
    ).joinToString(":")

    fun cacheMissSurface(rowKey: String): String = "$SURFACE_CACHE_MISS_PREFIX:${rowKey.ifBlank { "unknown" }}"

    fun cacheMissRetryAction(rowKey: String): String = "retry:${rowKey.ifBlank { "unknown" }}"

    fun cacheMissDiagnosticsAction(rowKey: String): String = "diagnostics:${rowKey.ifBlank { "unknown" }}"

    fun shouldAutoFocusRecovery(preferredSurface: String): Boolean =
        preferredSurface == SURFACE_RESULTS_RECOVERY ||
            preferredSurface == SURFACE_RESULTS ||
            preferredSurface.startsWith("$SURFACE_CACHE_MISS_PREFIX:")
}
