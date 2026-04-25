package com.ferrex.android.ui.tv

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/** D-pad navigable poster row for Android TV home. */
@Composable
fun TvPosterRow(
    title: String,
    items: List<TvPosterItem>,
    style: TvPosterCardStyle,
    onItemClick: (TvPosterItem) -> Unit,
    onItemFocused: (TvPosterItem) -> Unit,
    modifier: Modifier = Modifier,
    autoFocusFirst: Boolean = false,
) {
    if (items.isEmpty()) return

    val firstFocusRequester = remember { FocusRequester() }

    LaunchedEffect(items.size, autoFocusFirst) {
        if (autoFocusFirst) {
            runCatching { firstFocusRequester.requestFocus() }
        }
    }

    Column(modifier = modifier.fillMaxWidth()) {
        Text(
            text = title,
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onBackground,
            modifier = Modifier.padding(horizontal = 56.dp),
        )
        Spacer(Modifier.height(16.dp))
        LazyRow(
            contentPadding = PaddingValues(horizontal = 56.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(28.dp),
        ) {
            items(
                items = items,
                key = { it.id },
            ) { item ->
                TvPosterCard(
                    item = item,
                    style = style,
                    onClick = { onItemClick(item) },
                    onFocused = onItemFocused,
                    focusRequester = if (autoFocusFirst && item.id == items.firstOrNull()?.id) {
                        firstFocusRequester
                    } else {
                        null
                    },
                )
            }
        }
    }
}
