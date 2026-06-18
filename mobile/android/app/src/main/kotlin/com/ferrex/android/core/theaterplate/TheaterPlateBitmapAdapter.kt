package com.ferrex.android.core.theaterplate

import android.graphics.Bitmap

/** Adapter for Coil-decoded Android bitmaps; analysis itself still runs through [TheaterPlateAnalyzer]. */
class AndroidTheaterPlateDecodedBitmap(
    private val bitmap: Bitmap,
) : TheaterPlateDecodedBitmap {
    override val width: Int get() = if (bitmap.isRecycled) 0 else bitmap.width
    override val height: Int get() = if (bitmap.isRecycled) 0 else bitmap.height

    override fun copyArgbPixels(): IntArray {
        check(!bitmap.isRecycled) { "bitmap is recycled" }
        val pixels = IntArray(bitmap.width * bitmap.height)
        bitmap.getPixels(
            pixels,
            0,
            bitmap.width,
            0,
            0,
            bitmap.width,
            bitmap.height,
        )
        return pixels
    }
}

fun Bitmap.asTheaterPlateDecodedBitmap(): TheaterPlateDecodedBitmap = AndroidTheaterPlateDecodedBitmap(this)
