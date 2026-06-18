package com.ferrex.android.core.theaterplate

import androidx.compose.runtime.Composable
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember

/**
 * Compose bridge that immediately exposes a deterministic fallback, then swaps in decoded-bitmap
 * analysis after [TheaterPlateAnalyzer] finishes on its background dispatcher.
 */
@Composable
fun rememberTheaterPlateAnalysisState(
    cacheKey: TheaterPlateCacheKey?,
    bitmap: TheaterPlateDecodedBitmap?,
    context: TheaterPlateSourceContext,
    cache: TheaterPlateAnalysisCache? = null,
): TheaterPlateAnalysisState {
    val analyzer = remember { TheaterPlateAnalyzer() }
    val fallback = remember(context) { analyzer.analyzeMissingBackdrop(context) }

    return produceState<TheaterPlateAnalysisState>(
        TheaterPlateAnalysisState.Pending(fallback),
        cacheKey,
        bitmap,
        context,
        cache,
    ) {
        value = TheaterPlateAnalysisState.Pending(fallback)
        if (bitmap == null) return@produceState

        if (cacheKey != null) {
            cache?.get(cacheKey)?.let { cached ->
                value = TheaterPlateAnalysisState.Ready(cached)
                return@produceState
            }
        }

        when (val result = analyzer.analyzeDecodedBitmap(bitmap, context)) {
            is TheaterPlateAnalysisResult.Success -> {
                if (cacheKey != null) cache?.put(cacheKey, result.analysis)
                value = TheaterPlateAnalysisState.Ready(result.analysis)
            }
            is TheaterPlateAnalysisResult.Failure -> {
                value = TheaterPlateAnalysisState.Failed(result.error, fallback)
            }
        }
    }.value
}
