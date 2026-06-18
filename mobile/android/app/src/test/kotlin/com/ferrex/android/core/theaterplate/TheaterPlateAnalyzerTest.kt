package com.ferrex.android.core.theaterplate

import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.ServerCacheScope
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.UUID
import java.util.concurrent.Executors

class TheaterPlateAnalyzerTest {
    @Test
    fun brightDecodedBackdropGradesAsBrightAndUsesTinySamplingContract() {
        val analysis = analyze(solidBitmap(64, 36, TheaterPlateColor.rgb(240, 242, 246)))

        assertEquals(TheaterPlateGradeClass.Bright, analysis.grade.gradeClass)
        assertTrue(analysis.grade.isBright)
        assertTrue(analysis.averageLuminance > 0.9f)
        assertTrue(analysis.grade.highlightCompression > 0.7f)
        assertEquals(32, analysis.downsample.width)
        assertEquals(18, analysis.downsample.height)
        assertEquals(4, analysis.localLuma.columns)
        assertEquals(4, analysis.localLuma.rows)
        assertEquals(16, analysis.localLuma.cells.size)
    }

    @Test
    fun darkDecodedBackdropGradesAsDark() {
        val analysis = analyze(solidBitmap(64, 36, TheaterPlateColor.rgb(8, 10, 14)))

        assertEquals(TheaterPlateGradeClass.Dark, analysis.grade.gradeClass)
        assertTrue(analysis.grade.isDark)
        assertTrue(analysis.averageLuminance < 0.08f)
        assertTrue(analysis.grade.plateOpacity > 0.6f)
    }

    @Test
    fun busyDecodedBackdropGradesAsBusy() {
        val width = 64
        val height = 36
        val pixels = IntArray(width * height) { index ->
            val x = index % width
            val y = index / width
            val bright = ((x / 2) + (y / 2)) % 2 == 0
            val v = if (bright) 245 else 12
            argb(TheaterPlateColor.rgb(v, v, v))
        }

        val analysis = analyze(FakeDecodedBitmap(width, height, pixels))

        assertEquals(TheaterPlateGradeClass.Busy, analysis.grade.gradeClass)
        assertTrue(analysis.grade.isBusy)
        assertTrue(analysis.edgeDensity > 0.35f)
        assertTrue(analysis.grade.scrimOpacity > 0.7f)
    }

    @Test
    fun saturatedDecodedBackdropGradesAsSaturated() {
        val analysis = analyze(solidBitmap(64, 36, TheaterPlateColor.rgb(230, 32, 20)))

        assertEquals(TheaterPlateGradeClass.Saturated, analysis.grade.gradeClass)
        assertTrue(analysis.grade.isSaturated)
        assertTrue(analysis.averageSaturation > 0.8f)
        assertTrue(analysis.grade.desaturation > 0.2f)
    }

    @Test
    fun lowDetailDecodedBackdropGradesAsLowDetail() {
        val analysis = analyze(solidBitmap(64, 36, TheaterPlateColor.rgb(96, 96, 96)))

        assertEquals(TheaterPlateGradeClass.LowDetail, analysis.grade.gradeClass)
        assertTrue(analysis.grade.isLowDetail)
        assertTrue(analysis.edgeDensity < 0.02f)
        assertTrue(analysis.localLuma.contrast() < 0.02f)
    }

    @Test
    fun paletteExtractsDominantAccentMutedAndStageColor() {
        val width = 32
        val height = 16
        val dominant = TheaterPlateColor.rgb(24, 48, 96)
        val accent = TheaterPlateColor.rgb(230, 80, 24)
        val pixels = IntArray(width * height) { index ->
            if (index < width * height * 3 / 4) argb(dominant) else argb(accent)
        }
        val analysis = analyze(FakeDecodedBitmap(width, height, pixels))

        assertEquals(dominant, analysis.palette.dominant)
        assertEquals(accent, analysis.palette.accent)
        assertNotEquals(TheaterPlateColor.DefaultStage, analysis.palette.stage)
        assertTrue(analysis.palette.muted.luminance() in 0f..1f)
    }

    @Test
    fun missingBackdropPrefersPosterThenThemeThenDefaultStage() {
        val analyzer = TheaterPlateAnalyzer()
        val poster = TheaterPlateColor.rgb(180, 92, 20)
        val theme = TheaterPlateColor.rgb(20, 92, 180)
        val default = TheaterPlateColor.rgb(9, 12, 15)

        val posterAnalysis = analyzer.analyzeMissingBackdrop(
            TheaterPlateSourceContext.missingBackdrop(TheaterPlateViewport.of(800, 600))
                .copy(posterColor = poster, themeColor = theme, defaultColor = default),
        )
        val themeAnalysis = analyzer.analyzeMissingBackdrop(
            TheaterPlateSourceContext.missingBackdrop(TheaterPlateViewport.of(800, 600))
                .copy(themeColor = theme, defaultColor = default),
        )
        val defaultAnalysis = analyzer.analyzeMissingBackdrop(
            TheaterPlateSourceContext.missingBackdrop(TheaterPlateViewport.of(800, 600))
                .copy(defaultColor = default),
        )

        assertEquals(TheaterPlateImageSourceKind.PosterFallback, posterAnalysis.context.source.kind)
        assertEquals(poster, posterAnalysis.palette.dominant)
        assertEquals(theme, posterAnalysis.palette.accent)
        assertEquals(TheaterPlateImageSourceKind.ThemeColorFallback, themeAnalysis.context.source.kind)
        assertEquals(theme, themeAnalysis.palette.dominant)
        assertEquals(TheaterPlateImageSourceKind.GeneratedFallback, defaultAnalysis.context.source.kind)
        assertEquals(default, defaultAnalysis.palette.dominant)
        assertEquals(TheaterPlateGradeClass.MissingBackdrop, posterAnalysis.grade.gradeClass)
        assertTrue(posterAnalysis.grade.isMissingBackdrop)
        assertEquals(0.0f, posterAnalysis.grade.plateOpacity, 0.0001f)
    }

    @Test
    fun invalidDecodedBitmapReturnsFailureWithoutThrowing() {
        val analyzer = TheaterPlateAnalyzer()
        val invalidDimensions = analyzer.analyzeDecodedBitmapNow(
            FakeDecodedBitmap(0, 36, IntArray(0)),
            backdropContext(),
        )
        val shortBuffer = analyzer.analyzeDecodedBitmapNow(
            FakeDecodedBitmap(4, 4, IntArray(3)),
            backdropContext(),
        )

        assertTrue((invalidDimensions as TheaterPlateAnalysisResult.Failure).error is TheaterPlateAnalysisError.InvalidDimensions)
        val shortError = (shortBuffer as TheaterPlateAnalysisResult.Failure).error as TheaterPlateAnalysisError.BufferTooSmall
        assertEquals(16, shortError.expected)
        assertEquals(3, shortError.actual)
    }

    @Test
    fun decodedBitmapAnalysisUsesConfiguredBackgroundDispatcher() = runTest {
        val dispatcher = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "theater-plate-test-worker")
        }.asCoroutineDispatcher()
        try {
            val bitmap = solidBitmap(16, 9, TheaterPlateColor.rgb(64, 72, 80))
            val result = TheaterPlateAnalyzer(bitmapDispatcher = dispatcher)
                .analyzeDecodedBitmap(bitmap, backdropContext())

            result.expectAnalysis()
            assertTrue(bitmap.readThreadName?.contains("theater-plate-test-worker") == true)
        } finally {
            dispatcher.close()
        }
    }

    @Test
    fun pendingStateCarriesFallbackForComposeWhileAnalysisRuns() {
        val theme = TheaterPlateColor.fromHex("#336699")
        val context = TheaterPlateSourceContext.missingBackdrop()
            .copy(themeColor = theme)

        val pending = TheaterPlateAnalyzer().pendingState(context)

        assertEquals(TheaterPlateGradeClass.MissingBackdrop, pending.fallback.grade.gradeClass)
        assertEquals(TheaterPlateImageSourceKind.ThemeColorFallback, pending.fallback.context.source.kind)
        assertEquals(theme, pending.fallback.palette.dominant)
    }

    @Test
    fun colorHelpersExposeLumaSaturationHexAndStageWash() {
        val color = TheaterPlateColor.fromHex("#336699") ?: error("hex should parse")

        assertEquals("#336699", color.toHex())
        assertEquals(0.363f, color.luminance(), 0.01f)
        assertEquals(0.667f, color.saturation(), 0.01f)
        assertEquals(TheaterPlateColor.rgb(14, 23, 32), color.stageWash())
        assertNull(TheaterPlateColor.fromHex("#1234"))
    }

    @Test
    fun cacheKeyReusesScopeImageTokenAndViewportClass() {
        val scope = ServerCacheScope.from("HTTP://ferrex.local/", "user-1")
        val image = ImageRequestKey("00000000-0000-0000-0000-00000000abcd", BrowseImageCategory.Backdrop)
        val sameImageDifferentCase = ImageRequestKey(image.iid.uppercase(), BrowseImageCategory.Backdrop)
        val ready = ImageResolution.Ready(image, "http://ferrex.local/api/v1/images/blob/token-a", "token-a")
        val sameReady = ImageResolution.Ready(sameImageDifferentCase, ready.url, "token-a")
        val detail = TheaterPlateViewport.of(1280, 720)
        val sameClass = TheaterPlateViewport.of(1366, 768)
        val compact = TheaterPlateViewport.of(800, 600)

        val keyA = TheaterPlateCacheKey.fromReadyResolution(scope, ready, detail)
        val keyB = TheaterPlateCacheKey.fromReadyResolution(scope, sameReady, sameClass)
        val tokenChanged = TheaterPlateCacheKey.fromReadyResolution(scope, ready.copy(token = "token-b"), detail)
        val compactKey = TheaterPlateCacheKey.fromReadyResolution(scope, ready, compact)
        val cache = TheaterPlateAnalysisCache(capacity = 4)
        var runs = 0

        val first = cache.getOrPut(keyA) {
            runs += 1
            TheaterPlateAnalyzer().analyzeMissingBackdrop(TheaterPlateSourceContext.missingBackdrop())
        }
        val second = cache.getOrPut(keyB) {
            runs += 1
            TheaterPlateAnalyzer().analyzeMissingBackdrop(TheaterPlateSourceContext.missingBackdrop())
        }

        assertEquals(keyA, keyB)
        assertNotEquals(keyA, tokenChanged)
        assertNotEquals(keyA, compactKey)
        assertEquals("w780", theaterPlateBackdropSizeForViewport(compact))
        assertEquals("w1280", theaterPlateBackdropSizeForViewport(detail))
        assertEquals(1, runs)
        assertSame(first, second)
    }

    @Test
    fun analysisCacheEvictsLeastRecentlyUsedEntry() {
        val analyzer = TheaterPlateAnalyzer()
        val scope = ServerCacheScope.from("http://ferrex.local", "user-1")
        val cache = TheaterPlateAnalysisCache(capacity = 2)
        val a = cacheKey(scope, 101)
        val b = cacheKey(scope, 102)
        val c = cacheKey(scope, 103)

        cache.put(a, analyzer.analyzeMissingBackdrop(TheaterPlateSourceContext.missingBackdrop()))
        cache.put(b, analyzer.analyzeMissingBackdrop(TheaterPlateSourceContext.missingBackdrop()))
        assertNotNull(cache[a])
        cache.put(c, analyzer.analyzeMissingBackdrop(TheaterPlateSourceContext.missingBackdrop()))

        assertNotNull(cache.peek(a))
        assertNull(cache.peek(b))
        assertNotNull(cache.peek(c))
    }

    private fun analyze(bitmap: FakeDecodedBitmap): TheaterPlateAnalysis = TheaterPlateAnalyzer()
        .analyzeDecodedBitmapNow(bitmap, backdropContext())
        .expectAnalysis()

    private fun TheaterPlateAnalysisResult.expectAnalysis(): TheaterPlateAnalysis = when (this) {
        is TheaterPlateAnalysisResult.Success -> analysis
        is TheaterPlateAnalysisResult.Failure -> throw AssertionError(error.message)
    }

    private fun backdropContext(): TheaterPlateSourceContext = TheaterPlateSourceContext.backdrop(
        request = key(1, BrowseImageCategory.Backdrop),
        token = "backdrop-token",
        viewport = TheaterPlateViewport.of(1280, 720),
    )

    private fun cacheKey(scope: ServerCacheScope, seed: Int): TheaterPlateCacheKey {
        val ready = ImageResolution.Ready(
            key = key(seed, BrowseImageCategory.Backdrop),
            url = "http://ferrex.local/api/v1/images/blob/token-$seed",
            token = "token-$seed",
        )
        return TheaterPlateCacheKey.fromReadyResolution(scope, ready, TheaterPlateViewport.of(1280, 720))
    }

    private fun key(seed: Int, category: BrowseImageCategory): ImageRequestKey =
        ImageRequestKey(UUID(0L, seed.toLong()).toString(), category)

    private fun solidBitmap(width: Int, height: Int, color: TheaterPlateColor): FakeDecodedBitmap =
        FakeDecodedBitmap(width, height, IntArray(width * height) { argb(color) })

    private fun argb(color: TheaterPlateColor, alpha: Int = 255): Int =
        ((alpha.coerceIn(0, 255) and 0xff) shl 24) or
            ((color.r and 0xff) shl 16) or
            ((color.g and 0xff) shl 8) or
            (color.b and 0xff)

    private class FakeDecodedBitmap(
        override val width: Int,
        override val height: Int,
        private val pixels: IntArray,
    ) : TheaterPlateDecodedBitmap {
        var readThreadName: String? = null
            private set

        override fun copyArgbPixels(): IntArray {
            readThreadName = Thread.currentThread().name
            return pixels.copyOf()
        }
    }
}
