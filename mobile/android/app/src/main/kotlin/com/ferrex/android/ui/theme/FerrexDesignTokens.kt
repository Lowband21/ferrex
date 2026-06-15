package com.ferrex.android.ui.theme

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/**
 * Shared Android design tokens for the Ferrex Signal / private-cinema identity.
 *
 * The palette intentionally avoids Material You dynamic color by default. Phone and TV share the
 * same dark slate base, signal-cyan primary, private-violet secondary, and calm status colors so
 * recovery, cache, stale/offline, and error states do not drift between device classes.
 */
object FerrexDesignTokens {
    /** Approved dark slate / cyan / violet palette for active Android chrome and Compose surfaces. */
    object Palette {
        val SlateBlack = Color(0xFF020617)
        val SlateCanvas = Color(0xFF050A12)
        val SlatePanel = Color(0xFF0B1220)
        val SlateSurface = Color(0xFF111827)
        val SlateElevated = Color(0xFF172033)
        val SlateLine = Color(0xFF334155)
        val TextPrimary = Color(0xFFF8FAFC)
        val TextSecondary = Color(0xFFCBD5E1)
        val TextMuted = Color(0xFF94A3B8)

        val SignalCyan = Color(0xFF67E8F9)
        val SignalCyanDim = Color(0xFF164E63)
        val SignalBlue = Color(0xFF38BDF8)
        val PrivateViolet = Color(0xFFA78BFA)
        val PrivateVioletDim = Color(0xFF312E81)

        val Success = Color(0xFF34D399)
        val Warning = Color(0xFFFBBF24)
        val Error = Color(0xFFFB7185)
        val ErrorDim = Color(0xFF7F1D1D)
        val Cache = PrivateViolet
        val Offline = TextMuted

        val PosterScrim = Color(0xCC020617)
        val PosterFallback = Color(0xFF1E293B)
        val FocusWash = Color(0x3338BDF8)
        val VioletWash = Color(0x2EA78BFA)
    }

    /** Spacing scale used by primary phone surfaces and TV focus components. */
    object Space {
        val None = 0.dp
        val Xxs = 4.dp
        val Xs = 6.dp
        val Sm = 8.dp
        val Md = 12.dp
        val Lg = 16.dp
        val Xl = 20.dp
        val Xxl = 24.dp
        val Xxxl = 28.dp
        val ScreenPhoneHorizontal = 24.dp
        val ScreenPhoneVertical = 32.dp
        val ScreenTvHorizontal = 56.dp
        val ScreenTvVertical = 40.dp
    }

    /** Shape tokens for action buttons, status cards, posters, and TV focus rings. */
    object Shapes {
        val Button = RoundedCornerShape(14.dp)
        val Card = RoundedCornerShape(20.dp)
        val PosterCard = RoundedCornerShape(18.dp)
        val PosterImage = RoundedCornerShape(14.dp)
        val RecoveryCard = RoundedCornerShape(22.dp)
        val FocusSurface = RoundedCornerShape(16.dp)
        val Pill = RoundedCornerShape(999.dp)
    }

    /** Motion durations in milliseconds; keep short enough for D-pad repeat and recovery flows. */
    object Motion {
        const val FocusMillis = 120
        const val SurfaceMillis = 160
        const val PosterRevealMillis = 180
    }

    /** Poster and card treatment tokens shared by phone and TV media rows/grids. */
    object Poster {
        const val AspectRatio = 2f / 3f
        val PhoneCompactWidth = 150.dp
        val PhoneWidth = 180.dp
        val PhoneGridMin = 148.dp
        val TvWidth = 190.dp
        val TvGridMin = 190.dp
        val TvCardMinHeight = 338.dp
    }

    /** 10-foot layout and player-chrome dimensions that preserve TV ergonomics. */
    object Tv {
        val HomeMaxWidth = 1560.dp
        val DetailMaxWidth = 1320.dp
        val DiagnosticsMaxWidth = 1180.dp
        val FormActionMaxWidth = 920.dp
        val ActionPanelMaxWidth = 420.dp
        val RecoveryActionMaxWidth = 560.dp
        val DiagnosticsActionMaxWidth = 620.dp
        val PlayerPanelMaxWidth = 720.dp
        val PlayerPickerMaxWidth = 780.dp
        val PlayerActionMaxWidth = 520.dp
        val PlayerProgressWidth = 560.dp
        val PlayerSafeButtonWidth = 112.dp
        val SearchThumbnailWidth = 84.dp
        val DetailArtworkMinHeight = 220.dp
        val DetailArtworkMaxHeight = 340.dp
        val TrackListMaxHeight = 380.dp
        val ActionMinWidth = 180.dp
        val ActionMaxWidth = 360.dp
        val SearchResultMinHeight = 132.dp
        val FullScreenVerticalPadding = 36.dp
        val DetailVerticalPadding = 46.dp
        val PlayerChromeTopPadding = 32.dp
        val PlayerChromeBottomPadding = 48.dp
        val PlayerChromeHorizontalPadding = 48.dp
    }

    /** Status and recovery semantics used by shared action/status components. */
    object StatusAlpha {
        const val PrimaryContainer = 0.20f
        const val SecondaryContainer = 0.72f
        const val DisabledContainer = 0.05f
        const val DisabledContent = 0.42f
    }

    /** Focus styling adapters for Android TV D-pad surfaces. */
    object Focus {
        const val TvFocusedScale = 1.045f
        const val TvRestingScale = 1f
        val TvFocusedBorder = 3.dp
        val TvRestingBorder = 1.dp
        val TvFocusedElevation = 6.dp
        val TvButtonMinHeight = 58.dp
    }

    fun privateCinemaGradient(): Brush = Brush.horizontalGradient(
        listOf(
            Palette.PrivateVioletDim,
            Palette.SlateCanvas,
            Palette.SlateBlack,
        ),
    )
}
