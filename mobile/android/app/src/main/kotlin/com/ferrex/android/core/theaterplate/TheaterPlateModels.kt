package com.ferrex.android.core.theaterplate

import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.library.ServerCacheScope
import java.util.LinkedHashMap
import kotlin.math.roundToInt

private const val DEFAULT_CACHE_CAPACITY = 128

/** Viewport dimensions used to scope Android Theater Plate analysis and cache entries. */
data class TheaterPlateViewport(
    val width: Int,
    val height: Int,
) {
    init {
        require(width > 0) { "Theater Plate viewport width must be positive" }
        require(height > 0) { "Theater Plate viewport height must be positive" }
    }

    val longEdge: Int get() = maxOf(width, height)
    val shortEdge: Int get() = minOf(width, height)
    val viewportClass: TheaterPlateViewportClass get() = TheaterPlateViewportClass.forViewport(this)

    companion object {
        val DefaultDetail: TheaterPlateViewport = TheaterPlateViewport(1280, 720)

        fun of(width: Int, height: Int): TheaterPlateViewport = TheaterPlateViewport(
            width = width.coerceAtLeast(1),
            height = height.coerceAtLeast(1),
        )

        fun fromLogicalSize(width: Float, height: Float): TheaterPlateViewport = of(
            width = width.normalizedDimension(),
            height = height.normalizedDimension(),
        )
    }
}

/** Coarse viewport bucket used in cache keys so rotation/resize noise does not thrash analysis. */
enum class TheaterPlateViewportClass(
    val cacheValue: String,
    val boundedBackdropSize: String,
) {
    Compact(cacheValue = "compact", boundedBackdropSize = "w780"),
    Detail(cacheValue = "detail", boundedBackdropSize = "w1280"),
    TenFoot(cacheValue = "ten-foot", boundedBackdropSize = "w1280"),
    ;

    companion object {
        fun forViewport(viewport: TheaterPlateViewport): TheaterPlateViewportClass = when {
            viewport.longEdge >= 1920 || viewport.shortEdge >= 1000 -> TenFoot
            viewport.longEdge >= 1100 || viewport.shortEdge >= 700 -> Detail
            else -> Compact
        }
    }
}

fun theaterPlateBackdropSizeForViewport(viewport: TheaterPlateViewport): String =
    viewport.viewportClass.boundedBackdropSize

/** RGB color used by Theater Plate analysis and fallback decisions. */
data class TheaterPlateColor(
    val r: Int,
    val g: Int,
    val b: Int,
) {
    init {
        require(r in 0..255) { "red channel must be in 0..255" }
        require(g in 0..255) { "green channel must be in 0..255" }
        require(b in 0..255) { "blue channel must be in 0..255" }
    }

    fun toHex(): String = "#%02x%02x%02x".format(r, g, b)

    fun luminance(): Float {
        val rf = r / 255f
        val gf = g / 255f
        val bf = b / 255f
        return (0.2126f * rf + 0.7152f * gf + 0.0722f * bf).coerceIn(0f, 1f)
    }

    fun saturation(): Float {
        val rf = r / 255f
        val gf = g / 255f
        val bf = b / 255f
        val max = maxOf(rf, gf, bf)
        val min = minOf(rf, gf, bf)
        return if (max <= Float.MIN_VALUE) {
            0f
        } else {
            ((max - min) / max).coerceIn(0f, 1f)
        }
    }

    fun scale(factor: Float): TheaterPlateColor = rgb(
        r = (r * factor.safeFinite()).roundToInt().coerceIn(0, 255),
        g = (g * factor.safeFinite()).roundToInt().coerceIn(0, 255),
        b = (b * factor.safeFinite()).roundToInt().coerceIn(0, 255),
    )

    fun mix(other: TheaterPlateColor, otherWeight: Float): TheaterPlateColor {
        val t = otherWeight.safeFinite().coerceIn(0f, 1f)
        val inv = 1f - t
        return rgb(
            r = (r * inv + other.r * t).roundToInt().coerceIn(0, 255),
            g = (g * inv + other.g * t).roundToInt().coerceIn(0, 255),
            b = (b * inv + other.b * t).roundToInt().coerceIn(0, 255),
        )
    }

    fun stageWash(): TheaterPlateColor = scale(0.24f).mix(DefaultStage, 0.35f)

    companion object {
        val DefaultStage: TheaterPlateColor = TheaterPlateColor(18, 20, 24)

        fun rgb(r: Int, g: Int, b: Int): TheaterPlateColor = TheaterPlateColor(
            r = r.coerceIn(0, 255),
            g = g.coerceIn(0, 255),
            b = b.coerceIn(0, 255),
        )

        fun fromHex(input: String?): TheaterPlateColor? {
            val hex = input?.trim()?.removePrefix("#") ?: return null
            if (hex.length != 6) return null
            return runCatching {
                rgb(
                    r = hex.substring(0, 2).toInt(16),
                    g = hex.substring(2, 4).toInt(16),
                    b = hex.substring(4, 6).toInt(16),
                )
            }.getOrNull()
        }
    }
}

/** The image or fallback source used for a Theater Plate analysis. */
enum class TheaterPlateImageSourceKind {
    Backdrop,
    PosterFallback,
    ThemeColorFallback,
    GeneratedFallback,
    ;

    val isFallback: Boolean get() = this != Backdrop
}

data class TheaterPlateImageSource(
    val kind: TheaterPlateImageSourceKind,
    val request: ImageRequestKey? = null,
    val token: String? = null,
) {
    companion object {
        fun backdrop(request: ImageRequestKey, token: String? = null): TheaterPlateImageSource = TheaterPlateImageSource(
            kind = TheaterPlateImageSourceKind.Backdrop,
            request = request,
            token = token,
        )

        fun fallback(kind: TheaterPlateImageSourceKind): TheaterPlateImageSource = TheaterPlateImageSource(
            kind = kind,
        )
    }
}

/** Inputs outside decoded pixels that influence fallback source and stage color. */
data class TheaterPlateSourceContext(
    val source: TheaterPlateImageSource,
    val viewport: TheaterPlateViewport = TheaterPlateViewport.DefaultDetail,
    val posterColor: TheaterPlateColor? = null,
    val themeColor: TheaterPlateColor? = null,
    val defaultColor: TheaterPlateColor = TheaterPlateColor.DefaultStage,
) {
    companion object {
        fun backdrop(
            request: ImageRequestKey,
            token: String? = null,
            viewport: TheaterPlateViewport = TheaterPlateViewport.DefaultDetail,
        ): TheaterPlateSourceContext = TheaterPlateSourceContext(
            source = TheaterPlateImageSource.backdrop(request, token),
            viewport = viewport,
        )

        fun missingBackdrop(
            viewport: TheaterPlateViewport = TheaterPlateViewport.DefaultDetail,
        ): TheaterPlateSourceContext = TheaterPlateSourceContext(
            source = TheaterPlateImageSource.fallback(TheaterPlateImageSourceKind.GeneratedFallback),
            viewport = viewport,
        )
    }

    internal fun fallbackSeed(): Pair<TheaterPlateImageSourceKind, TheaterPlateColor> = when {
        posterColor != null -> TheaterPlateImageSourceKind.PosterFallback to posterColor
        themeColor != null -> TheaterPlateImageSourceKind.ThemeColorFallback to themeColor
        else -> TheaterPlateImageSourceKind.GeneratedFallback to defaultColor
    }
}

/** Supported decoded pixel layouts for JVM unit fixtures and non-Bitmap callers. */
enum class TheaterPlatePixelFormat(val bytesPerPixel: Int) {
    Rgb8(3),
    Rgba8(4),
}

data class TheaterPlatePixelImage(
    val width: Int,
    val height: Int,
    val pixels: ByteArray,
    val pixelFormat: TheaterPlatePixelFormat,
) {
    companion object {
        fun rgb8(width: Int, height: Int, pixels: ByteArray): TheaterPlatePixelImage = TheaterPlatePixelImage(
            width = width,
            height = height,
            pixels = pixels,
            pixelFormat = TheaterPlatePixelFormat.Rgb8,
        )

        fun rgba8(width: Int, height: Int, pixels: ByteArray): TheaterPlatePixelImage = TheaterPlatePixelImage(
            width = width,
            height = height,
            pixels = pixels,
            pixelFormat = TheaterPlatePixelFormat.Rgba8,
        )
    }
}

sealed interface TheaterPlateAnalysisError {
    val message: String

    data object InvalidDimensions : TheaterPlateAnalysisError {
        override val message: String = "Theater Plate image dimensions must be non-zero and bounded"
    }

    data class BufferTooSmall(
        val expected: Int,
        val actual: Int,
    ) : TheaterPlateAnalysisError {
        override val message: String = "Theater Plate image buffer is too small: expected at least $expected values, got $actual"
    }

    data class BitmapReadFailed(
        val reason: String,
    ) : TheaterPlateAnalysisError {
        override val message: String = "Theater Plate bitmap read failed: $reason"
    }
}

sealed interface TheaterPlateAnalysisResult {
    data class Success(val analysis: TheaterPlateAnalysis) : TheaterPlateAnalysisResult
    data class Failure(val error: TheaterPlateAnalysisError) : TheaterPlateAnalysisResult
}

/** Tiny ambient image produced by CPU downsampling. */
data class TheaterPlateDownsample(
    val width: Int,
    val height: Int,
    val pixels: List<TheaterPlateColor>,
) {
    companion object {
        fun solid(color: TheaterPlateColor, width: Int, height: Int): TheaterPlateDownsample {
            val safeWidth = width.coerceAtLeast(1)
            val safeHeight = height.coerceAtLeast(1)
            return TheaterPlateDownsample(
                width = safeWidth,
                height = safeHeight,
                pixels = List(safeWidth * safeHeight) { color },
            )
        }
    }
}

/** Local luminance grid for readability mask placement. */
data class TheaterPlateLocalLuma(
    val columns: Int,
    val rows: Int,
    val cells: List<Float>,
    val min: Float,
    val max: Float,
) {
    fun contrast(): Float = (max - min).coerceIn(0f, 1f)
}

data class TheaterPlatePalette(
    val dominant: TheaterPlateColor,
    val accent: TheaterPlateColor,
    val muted: TheaterPlateColor,
    val stage: TheaterPlateColor,
)

/** Primary art-direction bucket for a Theater Plate image. */
enum class TheaterPlateGradeClass {
    Balanced,
    Bright,
    Dark,
    Busy,
    Saturated,
    LowDetail,
    MissingBackdrop,
}

/** Stable CPU decisions later shader or Compose work can map to uniforms. */
data class TheaterPlateGradeControls(
    val highlightCompression: Float,
    val scrimOpacity: Float,
    val ambientOpacity: Float,
    val plateOpacity: Float,
    val desaturation: Float,
    val grainOpacity: Float,
)

data class TheaterPlateGrade(
    val gradeClass: TheaterPlateGradeClass,
    val isMissingBackdrop: Boolean,
    val isBright: Boolean,
    val isDark: Boolean,
    val isBusy: Boolean,
    val isSaturated: Boolean,
    val isLowDetail: Boolean,
    val controls: TheaterPlateGradeControls,
    val stageColor: TheaterPlateColor,
) {
    val highlightCompression: Float get() = controls.highlightCompression
    val scrimOpacity: Float get() = controls.scrimOpacity
    val ambientOpacity: Float get() = controls.ambientOpacity
    val plateOpacity: Float get() = controls.plateOpacity
    val desaturation: Float get() = controls.desaturation
    val grainOpacity: Float get() = controls.grainOpacity
}

/** Full CPU analysis sidecar for a Theater Plate source. */
data class TheaterPlateAnalysis(
    val context: TheaterPlateSourceContext,
    val sourceDimensions: Pair<Int, Int>?,
    val downsample: TheaterPlateDownsample,
    val palette: TheaterPlatePalette,
    val averageLuminance: Float,
    val medianLuminance: Float,
    val p95Luminance: Float,
    val averageSaturation: Float,
    val edgeDensity: Float,
    val edgeEnergy: Float,
    val localLuma: TheaterPlateLocalLuma,
    val grade: TheaterPlateGrade,
)

/** Compose-friendly state: callers can render [fallback] while decoded bitmap analysis is pending. */
sealed interface TheaterPlateAnalysisState {
    val fallback: TheaterPlateAnalysis?

    data class Pending(
        override val fallback: TheaterPlateAnalysis,
    ) : TheaterPlateAnalysisState

    data class Ready(
        val analysis: TheaterPlateAnalysis,
    ) : TheaterPlateAnalysisState {
        override val fallback: TheaterPlateAnalysis? = null
    }

    data class Failed(
        val error: TheaterPlateAnalysisError,
        override val fallback: TheaterPlateAnalysis,
    ) : TheaterPlateAnalysisState
}

/** Scope/image/token/viewport-class identity for bounded Android Theater Plate analysis caches. */
data class TheaterPlateCacheKey(
    val scopeKey: String,
    val imageCacheKey: String,
    val imageToken: String,
    val viewportClass: TheaterPlateViewportClass,
) {
    companion object {
        fun fromReadyResolution(
            scope: ServerCacheScope,
            resolution: ImageResolution.Ready,
            viewport: TheaterPlateViewport,
        ): TheaterPlateCacheKey = TheaterPlateCacheKey(
            scopeKey = scope.directoryName,
            imageCacheKey = resolution.key.cacheKey,
            imageToken = resolution.token,
            viewportClass = viewport.viewportClass,
        )

        fun fallback(
            scope: ServerCacheScope,
            stableSourceId: String,
            viewport: TheaterPlateViewport,
        ): TheaterPlateCacheKey = TheaterPlateCacheKey(
            scopeKey = scope.directoryName,
            imageCacheKey = "fallback:${stableSourceId.trim().lowercase()}",
            imageToken = "missing-backdrop",
            viewportClass = viewport.viewportClass,
        )
    }
}

class TheaterPlateAnalysisCache(capacity: Int = DEFAULT_CACHE_CAPACITY) {
    private val maxEntries: Int = capacity.coerceAtLeast(1)
    val capacity: Int get() = maxEntries

    private val entries = object : LinkedHashMap<TheaterPlateCacheKey, TheaterPlateAnalysis>(maxEntries, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<TheaterPlateCacheKey, TheaterPlateAnalysis>?): Boolean =
            size > maxEntries
    }

    @Synchronized
    fun len(): Int = entries.size

    @Synchronized
    fun isEmpty(): Boolean = entries.isEmpty()

    @Synchronized
    fun peek(key: TheaterPlateCacheKey): TheaterPlateAnalysis? = entries.entries.firstOrNull { it.key == key }?.value

    @Synchronized
    operator fun get(key: TheaterPlateCacheKey): TheaterPlateAnalysis? = entries[key]

    @Synchronized
    fun put(key: TheaterPlateCacheKey, analysis: TheaterPlateAnalysis) {
        entries[key] = analysis
    }

    @Synchronized
    fun getOrPut(key: TheaterPlateCacheKey, analyze: () -> TheaterPlateAnalysis): TheaterPlateAnalysis {
        entries[key]?.let { return it }
        return analyze().also { entries[key] = it }
    }

    @Synchronized
    fun keysSnapshot(): List<TheaterPlateCacheKey> = entries.keys.toList()
}

private fun Float.normalizedDimension(): Int = if (isFinite() && this > 0f) {
    coerceAtMost(Int.MAX_VALUE.toFloat()).roundToInt().coerceAtLeast(1)
} else {
    1
}

private fun Float.safeFinite(): Float = if (isFinite()) this else 0f
