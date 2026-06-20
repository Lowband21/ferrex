package com.ferrex.android.core.image

import com.ferrex.android.core.library.ServerCacheScope
import org.junit.Assert.assertEquals
import org.junit.Test
import java.util.UUID

class ImageCacheResetTest {
    private val scope = ServerCacheScope.from("http://ferrex.local", "user-1")

    @Test
    fun scopedClearerInvalidatesManifestMetadataAndImageLoaderState() {
        val manifest = FakeImageCacheClearer()
        val invalidations = ScopedImageLoaderInvalidations()
        val loader = FakeScopedImageLoaderClearer(invalidations)
        val clearer = ScopedImageCacheClearer(manifest, loader)
        val key = ImageRequestKey(UUID(0L, 1L).toString(), BrowseImageCategory.Poster)

        clearer.clearSelectedImages(scope, listOf(key))
        clearer.clearAllImages(scope)

        assertEquals(listOf(scope to listOf(key)), manifest.selectedClears)
        assertEquals(listOf(scope), manifest.allClears)
        assertEquals(listOf(scope, scope), loader.loaderClears)
        assertEquals(2L, invalidations.generationFor(scope))
        assertEquals(mapOf(scope.directoryName to 2L), invalidations.generations.value)
    }

    private class FakeImageCacheClearer : ImageCacheClearer {
        val selectedClears = mutableListOf<Pair<ServerCacheScope, List<ImageRequestKey>>>()
        val allClears = mutableListOf<ServerCacheScope>()

        override fun clearSelectedImages(scope: ServerCacheScope, keys: Collection<ImageRequestKey>) {
            selectedClears += scope to keys.toList()
        }

        override fun clearAllImages(scope: ServerCacheScope) {
            allClears += scope
        }
    }

    private class FakeScopedImageLoaderClearer(
        private val invalidations: ScopedImageLoaderInvalidations,
    ) : ScopedImageLoaderClearer {
        val loaderClears = mutableListOf<ServerCacheScope>()

        override fun clearImageLoaderState(scope: ServerCacheScope) {
            loaderClears += scope
            invalidations.invalidate(scope)
        }
    }
}
