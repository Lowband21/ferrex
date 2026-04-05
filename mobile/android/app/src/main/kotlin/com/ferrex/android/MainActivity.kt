package com.ferrex.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import com.ferrex.android.navigation.FerrexNavGraph
import com.ferrex.android.ui.theme.FerrexTheme
import dagger.hilt.android.AndroidEntryPoint

@AndroidEntryPoint
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        // Switch from splash theme to the normal theme before setContentView
        setTheme(R.style.Theme_Ferrex)
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            FerrexTheme {
                FerrexNavGraph()
            }
        }
    }
}
