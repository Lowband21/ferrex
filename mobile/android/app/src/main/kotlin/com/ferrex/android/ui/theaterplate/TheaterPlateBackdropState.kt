package com.ferrex.android.ui.theaterplate

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.Drawable
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import coil.ImageLoader
import coil.compose.AsyncImagePainter
import coil.compose.rememberAsyncImagePainter
import coil.request.ImageRequest as CoilImageRequest
import com.ferrex.android.core.browse.HomeBackdropStageState
import com.ferrex.android.core.browse.HomeBackdropStageStatus
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.theaterplate.TheaterPlateAnalysis
import com.ferrex.android.core.theaterplate.TheaterPlateAnalysisCache
import com.ferrex.android.core.theaterplate.TheaterPlateAnalysisState
import com.ferrex.android.core.theaterplate.TheaterPlateCacheKey
import com.ferrex.android.core.theaterplate.TheaterPlateDecodedBitmap
import com.ferrex.android.core.theaterplate.TheaterPlateSourceContext
import com.ferrex.android.core.theaterplate.asTheaterPlateDecodedBitmap
import com.ferrex.android.core.theaterplate.rememberTheaterPlateAnalysisState

class TheaterPlateResolvedBackdropState internal constructor(
    val analysisState: TheaterPlateAnalysisState,
    val painter: AsyncImagePainter?,
    val contentDescription: String?,
) {
    val analysis: TheaterPlateAnalysis
        get() = when (val state = analysisState) {
            is TheaterPlateAnalysisState.Pending -> state.fallback
            is TheaterPlateAnalysisState.Ready -> state.analysis
            is TheaterPlateAnalysisState.Failed -> state.fallback
        }

    val canRenderBackdrop: Boolean get() = painter != null
}

fun HomeBackdropStageState.toTheaterPlateBackdropAdaptation(
    imageLoaderAvailable: Boolean = true,
): TheaterPlateBackdropAdaptation = when (status) {
    HomeBackdropStageStatus.Ready -> if (isRenderable && imageLoaderAvailable) {
        TheaterPlateBackdropAdaptation.Ready
    } else {
        TheaterPlateBackdropAdaptation.Pending
    }
    HomeBackdropStageStatus.StaleOffline -> if (isRenderable && imageLoaderAvailable) {
        TheaterPlateBackdropAdaptation.StaleOffline
    } else {
        TheaterPlateBackdropAdaptation.Pending
    }
    HomeBackdropStageStatus.Pending -> TheaterPlateBackdropAdaptation.Pending
    HomeBackdropStageStatus.Failed,
    HomeBackdropStageStatus.NoBackdrop -> TheaterPlateBackdropAdaptation.MissingBackdrop
}

@Composable
fun rememberTheaterPlateResolvedBackdropState(
    scope: ServerCacheScope,
    stageState: HomeBackdropStageState,
    imageLoader: ImageLoader?,
    fallbackContext: TheaterPlateSourceContext,
    cache: TheaterPlateAnalysisCache? = null,
): TheaterPlateResolvedBackdropState {
    val analysisCache = cache ?: remember(scope.directoryName) { TheaterPlateAnalysisCache() }
    val resolution = stageState.readyResolution
    if (resolution == null || imageLoader == null) {
        val fallbackAnalysisState = rememberTheaterPlateAnalysisState(
            cacheKey = null,
            bitmap = null,
            context = fallbackContext,
            cache = analysisCache,
        )
        return TheaterPlateResolvedBackdropState(
            analysisState = fallbackAnalysisState,
            painter = null,
            contentDescription = null,
        )
    }

    val context = LocalContext.current
    val imageRequest = remember(resolution.url, resolution.token) {
        CoilImageRequest.Builder(context)
            .data(resolution.url)
            .allowHardware(false)
            .crossfade(false)
            .build()
    }
    val painter = rememberAsyncImagePainter(
        model = imageRequest,
        imageLoader = imageLoader,
    )
    val viewport = fallbackContext.viewport
    val sourceContext = remember(
        resolution.key.cacheKey,
        resolution.token,
        viewport,
        fallbackContext.posterColor,
        fallbackContext.themeColor,
        fallbackContext.defaultColor,
    ) {
        TheaterPlateSourceContext.backdrop(
            request = resolution.key,
            token = resolution.token,
            viewport = viewport,
        ).copy(
            posterColor = fallbackContext.posterColor,
            themeColor = fallbackContext.themeColor,
            defaultColor = fallbackContext.defaultColor,
        )
    }
    val cacheKey = remember(scope.directoryName, resolution.key.cacheKey, resolution.token, viewport) {
        TheaterPlateCacheKey.fromReadyResolution(scope, resolution, viewport)
    }
    val decodedBitmap = remember(painter.state) {
        (painter.state as? AsyncImagePainter.State.Success)
            ?.result
            ?.drawable
            ?.toTheaterPlateDecodedBitmapOrNull()
    }
    val analysisState = rememberTheaterPlateAnalysisState(
        cacheKey = cacheKey,
        bitmap = decodedBitmap,
        context = sourceContext,
        cache = analysisCache,
    )

    return TheaterPlateResolvedBackdropState(
        analysisState = analysisState,
        painter = painter,
        contentDescription = stageState.candidate?.title?.let { "Backdrop for $it" },
    )
}

@Composable
fun TheaterPlateResolvedBackdrop(
    state: TheaterPlateResolvedBackdropState,
    modifier: Modifier = Modifier,
) {
    val painter = state.painter ?: return
    Image(
        painter = painter,
        contentDescription = state.contentDescription,
        contentScale = ContentScale.Crop,
        modifier = modifier.fillMaxSize(),
    )
}

private fun Drawable.toTheaterPlateDecodedBitmapOrNull(): TheaterPlateDecodedBitmap? =
    toSoftwareBitmapOrNull()?.asTheaterPlateDecodedBitmap()

private fun Drawable.toSoftwareBitmapOrNull(): Bitmap? {
    if (this is BitmapDrawable) {
        val source = bitmap ?: return null
        if (source.isRecycled) return null
        return if (source.config == Bitmap.Config.HARDWARE) {
            runCatching { source.copy(Bitmap.Config.ARGB_8888, false) }.getOrNull()
        } else {
            source
        }
    }

    val width = intrinsicWidth.takeIf { it > 0 } ?: return null
    val height = intrinsicHeight.takeIf { it > 0 } ?: return null
    val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
    val canvas = Canvas(bitmap)
    val previousLeft = bounds.left
    val previousTop = bounds.top
    val previousRight = bounds.right
    val previousBottom = bounds.bottom
    return runCatching {
        setBounds(0, 0, width, height)
        draw(canvas)
        bitmap
    }.also {
        setBounds(previousLeft, previousTop, previousRight, previousBottom)
    }.getOrNull()
}
