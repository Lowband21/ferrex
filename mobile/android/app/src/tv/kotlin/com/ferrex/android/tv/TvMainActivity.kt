package com.ferrex.android.tv

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import com.ferrex.android.core.di.AndroidAuthDependencies
import com.ferrex.android.tv.navigation.TvFerrexNavGraph
import com.ferrex.android.ui.theme.FerrexTheme

class TvMainActivity : ComponentActivity() {
    private val dependencies by lazy {
        AndroidAuthDependencies(
            context = applicationContext,
            deviceName = "Ferrex Android TV",
        )
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            FerrexTheme(tv = true) {
                TvFerrexNavGraph(
                    authManager = dependencies.authManager,
                    libraryRepository = dependencies.libraryRepository,
                    imageRepository = dependencies.imageRepository,
                    imagePipeline = dependencies.imagePipeline,
                )
            }
        }
    }
}
