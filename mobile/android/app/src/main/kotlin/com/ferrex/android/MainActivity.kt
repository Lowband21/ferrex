package com.ferrex.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

internal object FerrexShellCopy {
    const val MOBILE_TITLE = "Ferrex Mobile"
    const val MOBILE_SUBTITLE = "Android Compose shell foundation"
    const val MOBILE_BODY = "Mobile build variant"

    const val TV_TITLE = "Ferrex TV"
    const val TV_SUBTITLE = "Android TV Compose shell foundation"
    const val TV_BODY = "10-foot build variant"
}

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            FerrexMobileShell()
        }
    }
}

@Composable
private fun FerrexMobileShell() {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Color(0xFFFFB35C),
            background = Color(0xFF101217),
            surface = Color(0xFF101217),
            onPrimary = Color(0xFF241100),
            onBackground = Color(0xFFE9EDF5),
            onSurface = Color(0xFFE9EDF5),
        ),
    ) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = MaterialTheme.colorScheme.background,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = 32.dp, vertical = 48.dp),
                horizontalAlignment = Alignment.Start,
                verticalArrangement = Arrangement.Center,
            ) {
                Text(
                    text = FerrexShellCopy.MOBILE_TITLE,
                    style = MaterialTheme.typography.headlineLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
                Text(
                    modifier = Modifier.padding(top = 12.dp),
                    text = FerrexShellCopy.MOBILE_SUBTITLE,
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onBackground,
                )
                Text(
                    modifier = Modifier.padding(top = 24.dp),
                    text = FerrexShellCopy.MOBILE_BODY,
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onBackground,
                    textAlign = TextAlign.Start,
                )
            }
        }
    }
}
