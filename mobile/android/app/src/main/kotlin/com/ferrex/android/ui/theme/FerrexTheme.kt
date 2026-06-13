package com.ferrex.android.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val MobileColors = darkColorScheme(
    primary = Color(0xFFFFB35C),
    background = Color(0xFF101217),
    surface = Color(0xFF171A21),
    onPrimary = Color(0xFF241100),
    onBackground = Color(0xFFE9EDF5),
    onSurface = Color(0xFFE9EDF5),
)

private val TvColors = darkColorScheme(
    primary = Color(0xFF83D6FF),
    background = Color(0xFF090B10),
    surface = Color(0xFF121722),
    onPrimary = Color(0xFF001F2E),
    onBackground = Color(0xFFF1F6FF),
    onSurface = Color(0xFFF1F6FF),
)

@Composable
fun FerrexTheme(
    tv: Boolean = false,
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (tv) TvColors else MobileColors,
        content = content,
    )
}
