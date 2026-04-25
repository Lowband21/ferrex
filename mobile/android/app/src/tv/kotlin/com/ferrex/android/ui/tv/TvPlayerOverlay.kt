package com.ferrex.android.ui.tv

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.ferrex.android.ui.player.PlayerScreen
import com.ferrex.android.ui.player.PlayerViewModel
import okhttp3.OkHttpClient

/** TV player shell that keeps player plumbing shared and overlays TV chrome. */
@Composable
fun TvPlayerScreen(
    viewModel: PlayerViewModel,
    okHttpClient: OkHttpClient,
    onBack: () -> Unit,
) {
    BackHandler(onBack = onBack)

    Box(modifier = Modifier.fillMaxSize()) {
        PlayerScreen(
            viewModel = viewModel,
            okHttpClient = okHttpClient,
        )
        TvPlayerOverlay(onBack = onBack)
    }
}

/** Overlay scaffold for TV-specific player controls. */
@Composable
fun TvPlayerOverlay(
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxSize()
            .background(
                Brush.verticalGradient(
                    colors = listOf(
                        Color.Black.copy(alpha = 0.55f),
                        Color.Transparent,
                        Color.Black.copy(alpha = 0.35f),
                    ),
                ),
            ),
    ) {
        Button(
            onClick = onBack,
            modifier = Modifier
                .align(Alignment.TopStart)
                .padding(40.dp)
                .semantics { contentDescription = "Back" },
            colors = ButtonDefaults.buttonColors(
                containerColor = Color.Black.copy(alpha = 0.62f),
                contentColor = Color.White,
            ),
        ) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null)
            Spacer(Modifier.width(8.dp))
            Text("Back")
        }

        Text(
            text = "D-pad to move • OK to select • Back to return",
            style = MaterialTheme.typography.titleMedium,
            color = Color.White.copy(alpha = 0.78f),
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .padding(bottom = 34.dp),
        )
    }
}
