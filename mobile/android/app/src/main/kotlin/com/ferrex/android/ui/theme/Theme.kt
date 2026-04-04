package com.ferrex.android.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

private val FerrexDarkColorScheme = darkColorScheme(
    primary = Color(0xFF90CAF9),
    onPrimary = Color(0xFF0D1B2A),
    primaryContainer = Color(0xFF1B3A5C),
    onPrimaryContainer = Color(0xFFD6EAFF),
    secondary = Color(0xFFB0BEC5),
    onSecondary = Color(0xFF1C2833),
    background = Color(0xFF0F1318),
    onBackground = Color(0xFFE1E3E6),
    surface = Color(0xFF161A21),
    onSurface = Color(0xFFE1E3E6),
    surfaceVariant = Color(0xFF1E2430),
    onSurfaceVariant = Color(0xFFC0C7D2),
    error = Color(0xFFEF9A9A),
    onError = Color(0xFF370B0B),
)

private val FerrexLightColorScheme = lightColorScheme(
    primary = Color(0xFF1565C0),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFD6EAFF),
    onPrimaryContainer = Color(0xFF0D1B2A),
    secondary = Color(0xFF546E7A),
    onSecondary = Color.White,
    background = Color(0xFFFAFAFA),
    onBackground = Color(0xFF1C1B1F),
    surface = Color.White,
    onSurface = Color(0xFF1C1B1F),
)

@Composable
fun FerrexTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = true,
    content: @Composable () -> Unit,
) {
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context)
            else dynamicLightColorScheme(context)
        }
        darkTheme -> FerrexDarkColorScheme
        else -> FerrexLightColorScheme
    }

    MaterialTheme(
        colorScheme = colorScheme,
        content = content,
    )
}
