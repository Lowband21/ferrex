package com.ferrex.android.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

private val FerrexAndroidColors = darkColorScheme(
    primary = FerrexDesignTokens.Palette.SignalCyan,
    onPrimary = FerrexDesignTokens.Palette.SlateBlack,
    primaryContainer = FerrexDesignTokens.Palette.SignalCyanDim,
    onPrimaryContainer = FerrexDesignTokens.Palette.TextPrimary,
    secondary = FerrexDesignTokens.Palette.PrivateViolet,
    onSecondary = FerrexDesignTokens.Palette.SlateBlack,
    secondaryContainer = FerrexDesignTokens.Palette.PrivateVioletDim,
    onSecondaryContainer = FerrexDesignTokens.Palette.TextPrimary,
    tertiary = FerrexDesignTokens.Palette.SignalBlue,
    onTertiary = FerrexDesignTokens.Palette.SlateBlack,
    background = FerrexDesignTokens.Palette.SlateCanvas,
    onBackground = FerrexDesignTokens.Palette.TextPrimary,
    surface = FerrexDesignTokens.Palette.SlatePanel,
    onSurface = FerrexDesignTokens.Palette.TextPrimary,
    surfaceVariant = FerrexDesignTokens.Palette.SlateElevated,
    onSurfaceVariant = FerrexDesignTokens.Palette.TextSecondary,
    error = FerrexDesignTokens.Palette.Error,
    onError = FerrexDesignTokens.Palette.SlateBlack,
    errorContainer = FerrexDesignTokens.Palette.ErrorDim,
    onErrorContainer = FerrexDesignTokens.Palette.TextPrimary,
    outline = FerrexDesignTokens.Palette.SlateLine,
    outlineVariant = FerrexDesignTokens.Palette.SlateSurface,
    scrim = FerrexDesignTokens.Palette.PosterScrim,
)

private val FerrexTypography = Typography(
    displayLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Bold,
        fontSize = 48.sp,
        lineHeight = 56.sp,
        letterSpacing = (-0.8).sp,
    ),
    displayMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Bold,
        fontSize = 42.sp,
        lineHeight = 50.sp,
        letterSpacing = (-0.5).sp,
    ),
    displaySmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Bold,
        fontSize = 34.sp,
        lineHeight = 42.sp,
        letterSpacing = (-0.25).sp,
    ),
    headlineLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Bold,
        fontSize = 30.sp,
        lineHeight = 38.sp,
    ),
    headlineMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.SemiBold,
        fontSize = 26.sp,
        lineHeight = 34.sp,
    ),
    headlineSmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.SemiBold,
        fontSize = 22.sp,
        lineHeight = 30.sp,
    ),
    titleLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.SemiBold,
        fontSize = 20.sp,
        lineHeight = 28.sp,
    ),
    titleMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.SemiBold,
        fontSize = 16.sp,
        lineHeight = 24.sp,
    ),
    titleSmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.SemiBold,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    bodyLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Normal,
        fontSize = 16.sp,
        lineHeight = 24.sp,
    ),
    bodyMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Normal,
        fontSize = 14.sp,
        lineHeight = 21.sp,
    ),
    bodySmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Normal,
        fontSize = 12.sp,
        lineHeight = 18.sp,
    ),
    labelLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.SemiBold,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    labelMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.SemiBold,
        fontSize = 12.sp,
        lineHeight = 16.sp,
    ),
    labelSmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Medium,
        fontSize = 11.sp,
        lineHeight = 14.sp,
    ),
)

private val FerrexTvTypography = FerrexTypography.copy(
    displaySmall = FerrexTypography.displaySmall.copy(fontSize = 42.sp, lineHeight = 50.sp),
    headlineSmall = FerrexTypography.headlineSmall.copy(fontSize = 26.sp, lineHeight = 34.sp),
    titleLarge = FerrexTypography.titleLarge.copy(fontSize = 24.sp, lineHeight = 32.sp),
    titleMedium = FerrexTypography.titleMedium.copy(fontSize = 20.sp, lineHeight = 28.sp),
    bodyLarge = FerrexTypography.bodyLarge.copy(fontSize = 18.sp, lineHeight = 26.sp),
)

private val FerrexShapes = Shapes(
    extraSmall = FerrexDesignTokens.Shapes.Button,
    small = FerrexDesignTokens.Shapes.Button,
    medium = FerrexDesignTokens.Shapes.PosterCard,
    large = FerrexDesignTokens.Shapes.Card,
    extraLarge = FerrexDesignTokens.Shapes.DialogPicker,
)

@Composable
fun FerrexTheme(
    tv: Boolean = false,
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = FerrexAndroidColors,
        typography = if (tv) FerrexTvTypography else FerrexTypography,
        shapes = FerrexShapes,
        content = content,
    )
}
