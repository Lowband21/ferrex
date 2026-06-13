package com.ferrex.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import com.ferrex.android.core.di.AndroidAuthDependencies
import com.ferrex.android.navigation.FerrexNavGraph
import com.ferrex.android.ui.theme.FerrexTheme

internal object FerrexShellCopy {
    const val MOBILE_TITLE = "Ferrex Mobile"
    const val MOBILE_SUBTITLE = "Recovery-first Android auth"
    const val MOBILE_BODY = "Protected media features stay closed until Ferrex validates the saved session."

    const val TV_TITLE = "Ferrex TV"
    const val TV_SUBTITLE = "D-pad recovery-first auth"
    const val TV_BODY = "TV recovery actions keep sign-in, server changes, and reset reachable without wiping app data."
}

class MainActivity : ComponentActivity() {
    private val dependencies by lazy {
        AndroidAuthDependencies(
            context = applicationContext,
            deviceName = "Ferrex Android",
        )
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            FerrexTheme {
                FerrexNavGraph(
                    authManager = dependencies.authManager,
                    libraryRepository = dependencies.libraryRepository,
                    imageRepository = dependencies.imageRepository,
                    imagePipeline = dependencies.imagePipeline,
                )
            }
        }
    }
}
