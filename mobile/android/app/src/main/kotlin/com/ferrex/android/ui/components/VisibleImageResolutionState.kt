package com.ferrex.android.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.ImageResolutionController
import com.ferrex.android.core.image.ImageResolutionControllerState
import com.ferrex.android.core.library.ServerCacheScope

class VisibleImageResolutionState(
    val controllerState: ImageResolutionControllerState,
    val retryVisibleNow: () -> Unit,
) {
    val resolutions: Map<ImageRequestKey, ImageResolution> get() = controllerState.resolutions
    val resolving: Boolean get() = controllerState.resolving
    val scheduledRetryAtMillis: Long? get() = controllerState.scheduledRetryAtMillis
}

@Composable
fun rememberVisibleImageResolutionState(
    scope: ServerCacheScope,
    imageRepository: ImageRepository?,
    visibleKeys: Collection<ImageRequestKey>,
): VisibleImageResolutionState {
    val controllerScope = rememberCoroutineScope()
    val controller = remember(imageRepository) {
        imageRepository?.let { ImageResolutionController(it, controllerScope) }
    }
    val distinctKeys = visibleKeys.distinctBy { it.cacheKey }

    DisposableEffect(controller) {
        onDispose { controller?.close() }
    }

    LaunchedEffect(controller, scope.directoryName, distinctKeys) {
        controller?.setVisibleImages(scope, distinctKeys)
    }

    val controllerState = if (controller != null) {
        val collected by controller.state.collectAsState()
        collected
    } else {
        ImageResolutionControllerState(
            scope = scope,
            visibleKeys = distinctKeys.toCollection(LinkedHashSet()),
        )
    }

    return VisibleImageResolutionState(
        controllerState = controllerState,
        retryVisibleNow = { controller?.retryVisibleNow() },
    )
}
