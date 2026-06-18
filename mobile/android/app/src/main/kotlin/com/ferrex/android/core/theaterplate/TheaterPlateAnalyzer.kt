package com.ferrex.android.core.theaterplate

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

private const val DEFAULT_DOWNSAMPLE_MAX_EDGE = 32
private const val DEFAULT_LOCAL_LUMA_COLUMNS = 4
private const val DEFAULT_LOCAL_LUMA_ROWS = 4
private const val EDGE_THRESHOLD = 0.10f
private const val ALPHA_EPSILON = 0.000001f

/** Minimal decoded-bitmap contract so Coil/Bitmap adapters and JVM tests share the same analyzer path. */
interface TheaterPlateDecodedBitmap {
    val width: Int
    val height: Int

    /** Returns ARGB pixels, row-major, using Android Bitmap/Drawable channel ordering. */
    fun copyArgbPixels(): IntArray
}

/** Analyzer configuration. Defaults match the Rust Theater Plate CPU contract. */
class TheaterPlateAnalyzer(
    val downsampleMaxEdge: Int = DEFAULT_DOWNSAMPLE_MAX_EDGE,
    val localLumaColumns: Int = DEFAULT_LOCAL_LUMA_COLUMNS,
    val localLumaRows: Int = DEFAULT_LOCAL_LUMA_ROWS,
    private val bitmapDispatcher: CoroutineDispatcher = Dispatchers.Default,
) {
    fun analyze(
        image: TheaterPlatePixelImage,
        context: TheaterPlateSourceContext,
    ): TheaterPlateAnalysisResult {
        val expectedBytes = expectedByteCount(image.width, image.height, image.pixelFormat.bytesPerPixel)
            ?: return TheaterPlateAnalysisResult.Failure(TheaterPlateAnalysisError.InvalidDimensions)
        if (image.pixels.size < expectedBytes) {
            return TheaterPlateAnalysisResult.Failure(
                TheaterPlateAnalysisError.BufferTooSmall(
                    expected = expectedBytes,
                    actual = image.pixels.size,
                ),
            )
        }

        return TheaterPlateAnalysisResult.Success(
            analyzePixels(
                reader = ByteArrayPixelReader(image),
                context = context,
                sourceDimensions = image.width to image.height,
            ),
        )
    }

    fun analyzeArgbPixels(
        width: Int,
        height: Int,
        argbPixels: IntArray,
        context: TheaterPlateSourceContext,
    ): TheaterPlateAnalysisResult {
        val expectedPixels = expectedPixelCount(width, height)
            ?: return TheaterPlateAnalysisResult.Failure(TheaterPlateAnalysisError.InvalidDimensions)
        if (argbPixels.size < expectedPixels) {
            return TheaterPlateAnalysisResult.Failure(
                TheaterPlateAnalysisError.BufferTooSmall(
                    expected = expectedPixels,
                    actual = argbPixels.size,
                ),
            )
        }

        return TheaterPlateAnalysisResult.Success(
            analyzePixels(
                reader = ArgbPixelReader(width, height, argbPixels),
                context = context,
                sourceDimensions = width to height,
            ),
        )
    }

    fun analyzeDecodedBitmapNow(
        bitmap: TheaterPlateDecodedBitmap,
        context: TheaterPlateSourceContext,
    ): TheaterPlateAnalysisResult {
        val expectedPixels = expectedPixelCount(bitmap.width, bitmap.height)
            ?: return TheaterPlateAnalysisResult.Failure(TheaterPlateAnalysisError.InvalidDimensions)
        val pixels = try {
            bitmap.copyArgbPixels()
        } catch (error: Throwable) {
            return TheaterPlateAnalysisResult.Failure(
                TheaterPlateAnalysisError.BitmapReadFailed(error.message ?: error::class.java.simpleName),
            )
        }
        if (pixels.size < expectedPixels) {
            return TheaterPlateAnalysisResult.Failure(
                TheaterPlateAnalysisError.BufferTooSmall(
                    expected = expectedPixels,
                    actual = pixels.size,
                ),
            )
        }
        return analyzeArgbPixels(bitmap.width, bitmap.height, pixels, context)
    }

    suspend fun analyzeDecodedBitmap(
        bitmap: TheaterPlateDecodedBitmap,
        context: TheaterPlateSourceContext,
    ): TheaterPlateAnalysisResult = withContext(bitmapDispatcher) {
        analyzeDecodedBitmapNow(bitmap, context)
    }

    /**
     * Build a fallback analysis when no usable backdrop is available. Poster color wins over
     * backend/theme color, then the default stage color, while still grading as MissingBackdrop.
     */
    fun analyzeMissingBackdrop(context: TheaterPlateSourceContext): TheaterPlateAnalysis {
        val (sourceKind, seed) = context.fallbackSeed()
        val fallbackContext = context.copy(source = TheaterPlateImageSource.fallback(sourceKind))
        val stage = seed.stageWash()
        val downsample = TheaterPlateDownsample.solid(seed.mix(stage, 0.45f), 8, 5)
        val metrics = calculateMetrics(downsample, localLumaColumns, localLumaRows)
        val palette = TheaterPlatePalette(
            dominant = seed,
            accent = context.themeColor ?: seed,
            muted = seed.mix(stage, 0.65f),
            stage = stage,
        )
        val grade = gradeFromMetrics(fallbackContext, metrics, palette.stage)

        return TheaterPlateAnalysis(
            context = fallbackContext,
            sourceDimensions = null,
            downsample = downsample,
            palette = palette,
            averageLuminance = metrics.averageLuminance,
            medianLuminance = metrics.medianLuminance,
            p95Luminance = metrics.p95Luminance,
            averageSaturation = metrics.averageSaturation,
            edgeDensity = metrics.edgeDensity,
            edgeEnergy = metrics.edgeEnergy,
            localLuma = metrics.localLuma,
            grade = grade,
        )
    }

    fun pendingState(context: TheaterPlateSourceContext): TheaterPlateAnalysisState.Pending =
        TheaterPlateAnalysisState.Pending(analyzeMissingBackdrop(context))

    fun stateFromResult(
        result: TheaterPlateAnalysisResult,
        fallbackContext: TheaterPlateSourceContext,
    ): TheaterPlateAnalysisState {
        val fallback = analyzeMissingBackdrop(fallbackContext)
        return when (result) {
            is TheaterPlateAnalysisResult.Success -> TheaterPlateAnalysisState.Ready(result.analysis)
            is TheaterPlateAnalysisResult.Failure -> TheaterPlateAnalysisState.Failed(result.error, fallback)
        }
    }

    private fun analyzePixels(
        reader: PixelReader,
        context: TheaterPlateSourceContext,
        sourceDimensions: Pair<Int, Int>,
    ): TheaterPlateAnalysis {
        val downsample = downsampleImage(reader, downsampleMaxEdge)
        val metrics = calculateMetrics(downsample, localLumaColumns, localLumaRows)
        val palette = extractPalette(downsample, context)
        val grade = gradeFromMetrics(context, metrics, palette.stage)

        return TheaterPlateAnalysis(
            context = context,
            sourceDimensions = sourceDimensions,
            downsample = downsample,
            palette = palette,
            averageLuminance = metrics.averageLuminance,
            medianLuminance = metrics.medianLuminance,
            p95Luminance = metrics.p95Luminance,
            averageSaturation = metrics.averageSaturation,
            edgeDensity = metrics.edgeDensity,
            edgeEnergy = metrics.edgeEnergy,
            localLuma = metrics.localLuma,
            grade = grade,
        )
    }
}

private data class PixelSample(
    val r: Int,
    val g: Int,
    val b: Int,
    val a: Int,
)

private interface PixelReader {
    val width: Int
    val height: Int
    fun pixelAt(x: Int, y: Int): PixelSample
}

private class ByteArrayPixelReader(
    private val image: TheaterPlatePixelImage,
) : PixelReader {
    override val width: Int = image.width
    override val height: Int = image.height

    override fun pixelAt(x: Int, y: Int): PixelSample {
        val bpp = image.pixelFormat.bytesPerPixel
        val offset = (y * image.width + x) * bpp
        val r = image.pixels[offset].unsigned()
        val g = image.pixels[offset + 1].unsigned()
        val b = image.pixels[offset + 2].unsigned()
        val a = if (image.pixelFormat == TheaterPlatePixelFormat.Rgba8) {
            image.pixels[offset + 3].unsigned()
        } else {
            255
        }
        return PixelSample(r, g, b, a)
    }
}

private class ArgbPixelReader(
    override val width: Int,
    override val height: Int,
    private val pixels: IntArray,
) : PixelReader {
    override fun pixelAt(x: Int, y: Int): PixelSample {
        val argb = pixels[y * width + x]
        return PixelSample(
            r = (argb ushr 16) and 0xff,
            g = (argb ushr 8) and 0xff,
            b = argb and 0xff,
            a = (argb ushr 24) and 0xff,
        )
    }
}

private data class Metrics(
    val averageLuminance: Float,
    val medianLuminance: Float,
    val p95Luminance: Float,
    val averageSaturation: Float,
    val edgeDensity: Float,
    val edgeEnergy: Float,
    val localLuma: TheaterPlateLocalLuma,
)

private fun downsampleImage(reader: PixelReader, maxEdge: Int): TheaterPlateDownsample {
    val (targetWidth, targetHeight) = downsampleDimensions(reader.width, reader.height, maxEdge)
    val pixels = ArrayList<TheaterPlateColor>(targetWidth * targetHeight)

    for (ty in 0 until targetHeight) {
        val y0 = ty * reader.height / targetHeight
        val y1 = (((ty + 1) * reader.height / targetHeight).coerceAtLeast(y0 + 1)).coerceAtMost(reader.height)
        for (tx in 0 until targetWidth) {
            val x0 = tx * reader.width / targetWidth
            val x1 = (((tx + 1) * reader.width / targetWidth).coerceAtLeast(x0 + 1)).coerceAtMost(reader.width)
            pixels += averageRegion(reader, x0, y0, x1, y1)
        }
    }

    return TheaterPlateDownsample(
        width = targetWidth,
        height = targetHeight,
        pixels = pixels,
    )
}

private fun downsampleDimensions(width: Int, height: Int, maxEdge: Int): Pair<Int, Int> {
    val safeMaxEdge = maxEdge.coerceAtLeast(1)
    val longEdge = maxOf(width, height)
    if (longEdge <= safeMaxEdge) {
        return width.coerceAtLeast(1) to height.coerceAtLeast(1)
    }

    return if (width >= height) {
        val targetHeight = ((height.toLong() * safeMaxEdge + width.toLong() / 2L) / width.toLong())
            .coerceAtLeast(1L)
            .toInt()
        safeMaxEdge to targetHeight
    } else {
        val targetWidth = ((width.toLong() * safeMaxEdge + height.toLong() / 2L) / height.toLong())
            .coerceAtLeast(1L)
            .toInt()
        targetWidth to safeMaxEdge
    }
}

private fun averageRegion(
    reader: PixelReader,
    x0: Int,
    y0: Int,
    x1: Int,
    y1: Int,
): TheaterPlateColor {
    var rSum = 0f
    var gSum = 0f
    var bSum = 0f
    var weightSum = 0f

    for (y in y0 until y1) {
        for (x in x0 until x1) {
            val pixel = reader.pixelAt(x, y)
            val weight = pixel.a / 255f
            if (weight <= ALPHA_EPSILON) continue
            rSum += pixel.r * weight
            gSum += pixel.g * weight
            bSum += pixel.b * weight
            weightSum += weight
        }
    }

    return if (weightSum <= ALPHA_EPSILON) {
        TheaterPlateColor.DefaultStage
    } else {
        TheaterPlateColor.rgb(
            r = (rSum / weightSum).roundToInt().coerceIn(0, 255),
            g = (gSum / weightSum).roundToInt().coerceIn(0, 255),
            b = (bSum / weightSum).roundToInt().coerceIn(0, 255),
        )
    }
}

private fun calculateMetrics(
    downsample: TheaterPlateDownsample,
    localColumns: Int,
    localRows: Int,
): Metrics {
    val luminances = downsample.pixels.map { it.luminance() }.sorted()
    val sampleCount = luminances.size.coerceAtLeast(1).toFloat()
    val averageLuminance = luminances.sum() / sampleCount
    val medianLuminance = percentile(luminances, 0.50f)
    val p95Luminance = percentile(luminances, 0.95f)
    val averageSaturation = downsample.pixels.sumOf { it.saturation().toDouble() }.toFloat() / sampleCount
    val (edgeDensity, edgeEnergy) = edgeMetrics(downsample)
    val localLuma = localLumaGrid(
        downsample = downsample,
        columns = localColumns.coerceAtLeast(1),
        rows = localRows.coerceAtLeast(1),
    )

    return Metrics(
        averageLuminance = averageLuminance,
        medianLuminance = medianLuminance,
        p95Luminance = p95Luminance,
        averageSaturation = averageSaturation,
        edgeDensity = edgeDensity,
        edgeEnergy = edgeEnergy,
        localLuma = localLuma,
    )
}

private fun percentile(sorted: List<Float>, percentile: Float): Float {
    if (sorted.isEmpty()) return 0f
    val idx = ((sorted.size - 1) * percentile.coerceIn(0f, 1f)).roundToInt()
    return sorted[idx]
}

private fun edgeMetrics(downsample: TheaterPlateDownsample): Pair<Float, Float> {
    var comparisons = 0
    var edgeCount = 0
    var energy = 0f

    for (y in 0 until downsample.height) {
        for (x in 0 until downsample.width) {
            val current = downsample.pixels[y * downsample.width + x].luminance()
            if (x + 1 < downsample.width) {
                val right = downsample.pixels[y * downsample.width + x + 1].luminance()
                val diff = kotlin.math.abs(current - right)
                comparisons += 1
                energy += diff
                if (diff >= EDGE_THRESHOLD) edgeCount += 1
            }
            if (y + 1 < downsample.height) {
                val below = downsample.pixels[(y + 1) * downsample.width + x].luminance()
                val diff = kotlin.math.abs(current - below)
                comparisons += 1
                energy += diff
                if (diff >= EDGE_THRESHOLD) edgeCount += 1
            }
        }
    }

    return if (comparisons == 0) {
        0f to 0f
    } else {
        edgeCount / comparisons.toFloat() to energy / comparisons.toFloat()
    }
}

private fun localLumaGrid(
    downsample: TheaterPlateDownsample,
    columns: Int,
    rows: Int,
): TheaterPlateLocalLuma {
    val cells = ArrayList<Float>(columns * rows)
    var min = 1f
    var max = 0f

    for (row in 0 until rows) {
        val y0 = row * downsample.height / rows
        val y1 = (((row + 1) * downsample.height / rows).coerceAtLeast(y0 + 1)).coerceAtMost(downsample.height)
        for (col in 0 until columns) {
            val x0 = col * downsample.width / columns
            val x1 = (((col + 1) * downsample.width / columns).coerceAtLeast(x0 + 1)).coerceAtMost(downsample.width)
            var sum = 0f
            var count = 0
            for (y in y0 until y1) {
                for (x in x0 until x1) {
                    sum += downsample.pixels[y * downsample.width + x].luminance()
                    count += 1
                }
            }
            val luma = if (count == 0) 0f else sum / count
            min = minOf(min, luma)
            max = maxOf(max, luma)
            cells += luma
        }
    }

    return TheaterPlateLocalLuma(
        columns = columns,
        rows = rows,
        cells = cells,
        min = min,
        max = max,
    )
}

private data class PaletteBucketKey(
    val r: Int,
    val g: Int,
    val b: Int,
)

private class PaletteBucket {
    var count: Int = 0
        private set
    private var rSum: Int = 0
    private var gSum: Int = 0
    private var bSum: Int = 0

    fun add(color: TheaterPlateColor) {
        count += 1
        rSum += color.r
        gSum += color.g
        bSum += color.b
    }

    fun color(): TheaterPlateColor = if (count == 0) {
        TheaterPlateColor.DefaultStage
    } else {
        TheaterPlateColor.rgb(
            r = rSum / count,
            g = gSum / count,
            b = bSum / count,
        )
    }
}

private fun extractPalette(
    downsample: TheaterPlateDownsample,
    context: TheaterPlateSourceContext,
): TheaterPlatePalette {
    val buckets = linkedMapOf<PaletteBucketKey, PaletteBucket>()
    downsample.pixels.forEach { color ->
        val key = PaletteBucketKey(color.r / 32, color.g / 32, color.b / 32)
        buckets.getOrPut(key) { PaletteBucket() }.add(color)
    }

    val dominant = buckets.values
        .maxByOrNull { it.count }
        ?.color()
        ?: context.defaultColor
    val accent = buckets.values
        .map { it.color() }
        .maxByOrNull(::colorfulnessScore)
        ?: dominant
    val muted = buckets.values
        .map { it.color() }
        .minByOrNull { it.saturation() + kotlin.math.abs(it.luminance() - 0.32f) * 0.2f }
        ?: dominant
    val fallback = context.posterColor ?: context.themeColor
    val stageSeed = fallback?.let { dominant.mix(it, 0.35f) } ?: dominant
    val stage = stageSeed.stageWash()

    return TheaterPlatePalette(
        dominant = dominant,
        accent = accent,
        muted = muted,
        stage = stage,
    )
}

private fun colorfulnessScore(color: TheaterPlateColor): Float {
    val luma = color.luminance()
    val midLumaPreference = 1f - kotlin.math.abs(luma - 0.52f).coerceAtMost(0.52f) / 0.52f
    return color.saturation() * 0.75f + midLumaPreference * 0.25f
}

private fun gradeFromMetrics(
    context: TheaterPlateSourceContext,
    metrics: Metrics,
    stageColor: TheaterPlateColor,
): TheaterPlateGrade {
    val isMissingBackdrop = context.source.kind.isFallback
    val localContrast = metrics.localLuma.contrast()

    val isBright = !isMissingBackdrop && (
        metrics.p95Luminance >= 0.82f ||
            metrics.averageLuminance >= 0.68f ||
            metrics.localLuma.max >= 0.86f
        )
    val isDark = !isMissingBackdrop &&
        metrics.averageLuminance <= 0.22f &&
        metrics.p95Luminance <= 0.48f
    val isBusy = !isMissingBackdrop && (
        metrics.edgeDensity >= 0.28f ||
            metrics.edgeEnergy >= 0.16f ||
            (metrics.edgeDensity >= 0.20f && localContrast >= 0.25f)
        )
    val isSaturated = !isMissingBackdrop && metrics.averageSaturation >= 0.50f
    val isLowDetail = !isMissingBackdrop &&
        !isBright &&
        !isDark &&
        !isBusy &&
        metrics.averageSaturation < 0.45f &&
        metrics.edgeDensity <= 0.06f &&
        localContrast <= 0.12f

    val gradeClass = when {
        isMissingBackdrop -> TheaterPlateGradeClass.MissingBackdrop
        isBusy -> TheaterPlateGradeClass.Busy
        isBright -> TheaterPlateGradeClass.Bright
        isDark -> TheaterPlateGradeClass.Dark
        isSaturated -> TheaterPlateGradeClass.Saturated
        isLowDetail -> TheaterPlateGradeClass.LowDetail
        else -> TheaterPlateGradeClass.Balanced
    }

    val controls = when (gradeClass) {
        TheaterPlateGradeClass.MissingBackdrop -> TheaterPlateGradeControls(0.25f, 0.48f, 0.62f, 0.0f, 0.05f, 0.015f)
        TheaterPlateGradeClass.Busy -> TheaterPlateGradeControls(0.70f, 0.72f, 0.54f, 0.38f, 0.28f, 0.035f)
        TheaterPlateGradeClass.Bright -> TheaterPlateGradeControls(0.78f, 0.66f, 0.40f, 0.50f, 0.16f, 0.020f)
        TheaterPlateGradeClass.Dark -> TheaterPlateGradeControls(0.18f, 0.34f, 0.48f, 0.66f, 0.04f, 0.012f)
        TheaterPlateGradeClass.Saturated -> TheaterPlateGradeControls(0.38f, 0.54f, 0.50f, 0.58f, 0.22f, 0.018f)
        TheaterPlateGradeClass.LowDetail -> TheaterPlateGradeControls(0.30f, 0.46f, 0.62f, 0.34f, 0.08f, 0.020f)
        TheaterPlateGradeClass.Balanced -> TheaterPlateGradeControls(0.34f, 0.50f, 0.46f, 0.60f, 0.08f, 0.016f)
    }

    return TheaterPlateGrade(
        gradeClass = gradeClass,
        isMissingBackdrop = isMissingBackdrop,
        isBright = isBright,
        isDark = isDark,
        isBusy = isBusy,
        isSaturated = isSaturated,
        isLowDetail = isLowDetail,
        controls = controls,
        stageColor = stageColor,
    )
}

private fun expectedPixelCount(width: Int, height: Int): Int? {
    if (width <= 0 || height <= 0) return null
    val count = width.toLong() * height.toLong()
    if (count > Int.MAX_VALUE) return null
    return count.toInt()
}

private fun expectedByteCount(width: Int, height: Int, bytesPerPixel: Int): Int? {
    val pixelCount = expectedPixelCount(width, height) ?: return null
    val bytes = pixelCount.toLong() * bytesPerPixel.toLong()
    if (bytes > Int.MAX_VALUE) return null
    return bytes.toInt()
}

private fun Byte.unsigned(): Int = toInt() and 0xff
