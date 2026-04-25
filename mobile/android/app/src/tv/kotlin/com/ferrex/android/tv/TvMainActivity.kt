package com.ferrex.android.tv

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import com.ferrex.android.R
import com.ferrex.android.tv.navigation.TvFerrexNavGraph
import com.ferrex.android.ui.theme.FerrexTheme
import dagger.hilt.android.AndroidEntryPoint

@AndroidEntryPoint
class TvMainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        setTheme(R.style.Theme_Ferrex)
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            FerrexTheme {
                TvFerrexNavGraph()
            }
        }
    }
}
