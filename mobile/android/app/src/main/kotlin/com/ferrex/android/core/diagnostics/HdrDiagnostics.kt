package com.ferrex.android.core.diagnostics

import android.content.Context
import android.content.pm.ActivityInfo
import android.hardware.display.DisplayManager
import android.os.Build
import android.view.Display
import android.view.Window

/**
 * Logs HDR-related display and window state.
 *
 * Uses [DisplayManager] (API 17+) for display access — avoids the
 * deprecated Activity.getWindowManager().getDefaultDisplay() path.
 *
 * HDR type queries use the per-[Display.Mode] API on API 33+ and fall
 * back to [Display.HdrCapabilities] on older levels.
 */
object HdrDiagnostics {

    private const val TAG = "HDR"

    /** Log display HDR capabilities and window color mode. */
    fun logDisplayCapabilities(context: Context, window: Window) {
        val dm = context.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager
        val display = dm.getDisplay(Display.DEFAULT_DISPLAY) ?: run {
            DiagnosticLog.w(TAG, "No default display")
            return
        }

        // ── HDR type support ────────────────────────────────────
        // API 33+ exposes per-mode HDR types; older levels use the
        // display-wide HdrCapabilities (deprecated in 34 but needed for 28–32).
        val hdrTypeNames: List<String> = when {
            Build.VERSION.SDK_INT >= 33 ->
                display.mode.supportedHdrTypes.map(::hdrTypeName)
            else -> {
                @Suppress("DEPRECATION")
                display.hdrCapabilities?.supportedHdrTypes?.map(::hdrTypeName) ?: emptyList()
            }
        }
        DiagnosticLog.i(TAG, "Display HDR types: $hdrTypeNames")

        // ── Luminance envelope ──────────────────────────────────
        display.hdrCapabilities?.let { caps ->
            DiagnosticLog.i(TAG,
                "Display luminance: max=%.0f nits  avgMax=%.0f nits  min=%.4f nits".format(
                    caps.desiredMaxLuminance,
                    caps.desiredMaxAverageLuminance,
                    caps.desiredMinLuminance,
                ))
        }

        // ── Window color mode ───────────────────────────────────
        DiagnosticLog.i(TAG, "Window colorMode: ${colorModeName(window.colorMode)}")
        DiagnosticLog.i(TAG, "Display wideColorGamut: ${display.isWideColorGamut}")
    }

    private fun hdrTypeName(type: Int): String = when (type) {
        Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION -> "DolbyVision"
        Display.HdrCapabilities.HDR_TYPE_HDR10 -> "HDR10"
        Display.HdrCapabilities.HDR_TYPE_HDR10_PLUS -> "HDR10+"
        Display.HdrCapabilities.HDR_TYPE_HLG -> "HLG"
        else -> "UNKNOWN($type)"
    }

    private fun colorModeName(mode: Int): String = when (mode) {
        ActivityInfo.COLOR_MODE_DEFAULT -> "DEFAULT"
        ActivityInfo.COLOR_MODE_WIDE_COLOR_GAMUT -> "WIDE_COLOR_GAMUT"
        ActivityInfo.COLOR_MODE_HDR -> "HDR"
        else -> "UNKNOWN($mode)"
    }
}
