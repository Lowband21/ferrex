package com.ferrex.android.ui.player

import android.app.Activity
import android.content.pm.ActivityInfo
import android.view.WindowManager
import com.ferrex.android.core.diagnostics.HdrDiagnostics
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalContext
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.ferrex.android.core.diagnostics.DiagnosticLog

private const val TAG = "ImmersiveMode"

/**
 * Composable side-effect that puts the hosting Activity into immersive
 * fullscreen mode when entered and restores normal system UI when disposed.
 *
 * What it does:
 * - **Hides** the status bar (notifications) and navigation bar (back/home/recents).
 * - Uses **BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE** — a swipe from the edge
 *   temporarily reveals the bars as translucent overlays, then they auto-hide
 *   again. Content is never resized.
 * - **Keeps the screen on** via FLAG_KEEP_SCREEN_ON so playback doesn't get
 *   interrupted by the device sleep timer.
 * - **Locks landscape orientation** — media playback is always landscape.
 *
 * All changes are reversed in [DisposableEffect.onDispose], so navigating
 * away from the player cleanly restores the normal system chrome.
 *
 * Lifecycle safety:
 * - Keyed on `Unit` (runs once when the composable enters/leaves composition).
 * - Uses [WindowInsetsControllerCompat] (AndroidX) which wraps the modern
 *   WindowInsetsController (API 30+) and the legacy View.systemUiVisibility
 *   flags (API 28–29) behind a single API. Since minSdk is 28, this covers
 *   all supported devices.
 * - The Activity reference is resolved from LocalContext; if the context
 *   chain doesn't contain an Activity (shouldn't happen in our nav graph),
 *   the effect is a no-op with a diagnostic log.
 */
@Composable
fun ImmersiveMode() {
    val context = LocalContext.current

    DisposableEffect(Unit) {
        val activity = context as? Activity
        if (activity == null) {
            DiagnosticLog.w(TAG, "Context is not an Activity — cannot enter immersive mode")
            return@DisposableEffect onDispose {}
        }

        val window = activity.window
        val decorView = window.decorView

        // Save original orientation so we can restore it.
        val originalOrientation = activity.requestedOrientation

        // ── Enter immersive mode ────────────────────────────────

        // 1. Tell the framework we're managing insets ourselves (decor
        //    fitting is disabled → content draws behind system bars).
        WindowCompat.setDecorFitsSystemWindows(window, false)

        // 2. Get the compat controller — this abstracts API 28–29
        //    (legacy systemUiVisibility flags) vs API 30+ (WindowInsetsController).
        val insetsController = WindowCompat.getInsetsController(window, decorView)

        // 3. BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE: bars appear as semi-
        //    transparent overlays on edge swipe, then auto-hide.  Content
        //    layout is never disturbed (unlike BEHAVIOR_DEFAULT which
        //    resizes the content area).
        insetsController.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE

        // 4. Hide status bar + navigation bar.
        insetsController.hide(WindowInsetsCompat.Type.systemBars())

        // 5. Keep screen on during playback.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        // 6. Lock to landscape (sensor-based so the user can flip the phone
        //    180° and the player follows).
        activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE

        // 7. Request HDR color mode for the Activity window.
        //    SurfaceView has its own surface that *should* get HDR automatically,
        //    but Samsung OneUI checks the Activity color mode as well.
        val originalColorMode = window.colorMode
        window.colorMode = ActivityInfo.COLOR_MODE_HDR
        DiagnosticLog.i(TAG, "Window colorMode: $originalColorMode → ${window.colorMode} (requested HDR=${ActivityInfo.COLOR_MODE_HDR})")

        // Log display HDR capabilities for diagnostics.
        HdrDiagnostics.logDisplayCapabilities(activity, window)

        DiagnosticLog.i(TAG, "Entered immersive fullscreen (landscape, screen-on, HDR)")

        // ── Restore on exit ─────────────────────────────────────
        onDispose {
            DiagnosticLog.i(TAG, "Exiting immersive fullscreen")

            // Restore system bars.
            insetsController.show(WindowInsetsCompat.Type.systemBars())

            // Re-enable decor fitting so the rest of the app respects insets.
            WindowCompat.setDecorFitsSystemWindows(window, false) // keep edge-to-edge

            // Remove keep-screen-on.
            window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

            // Restore color mode.
            window.colorMode = originalColorMode

            // Restore original orientation (typically SCREEN_ORIENTATION_UNSPECIFIED
            // which follows the sensor).
            activity.requestedOrientation = originalOrientation
        }
    }
}
