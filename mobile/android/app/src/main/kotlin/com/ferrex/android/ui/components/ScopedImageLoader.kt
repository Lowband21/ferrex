package com.ferrex.android.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.remember
import coil.ImageLoader
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.library.ServerCacheScope

/**
 * Remembers the scoped ImageLoader while observing cache-reset generations so active screens
 * stop using a loader immediately after the scope's previous loader is shut down.
 */
@Composable
fun rememberScopedImageLoader(
    imagePipeline: FerrexImagePipeline?,
    scope: ServerCacheScope,
): ImageLoader? {
    val generationState = imagePipeline?.imageLoaderGenerations?.collectAsState()
    val generation = generationState?.value?.get(scope.directoryName) ?: 0L
    return remember(imagePipeline, scope, generation) {
        imagePipeline?.imageLoader(scope)
    }
}
