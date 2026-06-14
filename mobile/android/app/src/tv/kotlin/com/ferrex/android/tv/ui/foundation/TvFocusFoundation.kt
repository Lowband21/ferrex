package com.ferrex.android.tv.ui.foundation

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.tvfocus.TvFocusKey
import com.ferrex.android.core.tvfocus.TvFocusRestoreState

@Stable
class TvFocusRestorer internal constructor(
    val screen: String,
    initialState: TvFocusRestoreState = TvFocusRestoreState(),
) {
    var state by mutableStateOf(initialState)
        private set

    fun record(surface: String, item: String) {
        state = state.record(TvFocusKey(screen = screen, surface = surface, item = item))
    }

    fun restoreItem(
        surface: String,
        availableItems: Collection<String>,
        fallbackItem: String,
    ): String = state.restore(
        screen = screen,
        surface = surface,
        availableItems = availableItems,
        fallbackItem = fallbackItem,
    ).target.item
}

@Composable
fun rememberTvFocusRestorer(screen: String): TvFocusRestorer = remember(screen) { TvFocusRestorer(screen) }

@Composable
fun TvScaffold(
    modifier: Modifier = Modifier,
    contentMaxWidth: Dp = 920.dp,
    horizontalPadding: Dp = 96.dp,
    verticalPadding: Dp = 64.dp,
    verticalArrangement: Arrangement.Vertical = Arrangement.Center,
    horizontalAlignment: Alignment.Horizontal = Alignment.CenterHorizontally,
    scrollable: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    Surface(
        modifier = modifier
            .fillMaxSize()
            .background(Color(0xFF070A12)),
        color = MaterialTheme.colorScheme.background,
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.horizontalGradient(
                        listOf(Color(0xFF172554), Color(0xFF070A12), Color(0xFF020617)),
                    ),
                )
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(horizontal = horizontalPadding, vertical = verticalPadding),
            contentAlignment = Alignment.Center,
        ) {
            val columnModifier = if (scrollable) {
                Modifier
                    .widthIn(max = contentMaxWidth)
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
            } else {
                Modifier
                    .widthIn(max = contentMaxWidth)
                    .fillMaxWidth()
            }
            Column(
                modifier = columnModifier,
                horizontalAlignment = horizontalAlignment,
                verticalArrangement = verticalArrangement,
                content = content,
            )
        }
    }
}

@Composable
fun TvTitle(title: String, subtitle: String, modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.displaySmall,
            color = MaterialTheme.colorScheme.primary,
            fontWeight = FontWeight.Bold,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(12.dp))
        Text(
            text = subtitle,
            style = MaterialTheme.typography.titleLarge,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(28.dp))
    }
}

enum class TvFocusableStyle {
    Primary,
    Secondary,
    Destructive,
}

@Composable
fun TvFocusableSurface(
    onClick: () -> Unit,
    semanticLabel: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    style: TvFocusableStyle = TvFocusableStyle.Secondary,
    focusRequester: FocusRequester? = null,
    minHeight: Dp = 58.dp,
    onFocused: () -> Unit = {},
    content: @Composable RowScope.() -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val scale by animateFloatAsState(targetValue = if (focused) 1.045f else 1f, label = "tvFocusableScale")
    val shape = RoundedCornerShape(12.dp)
    val colors = tvFocusableColors(style = style, focused = focused, enabled = enabled)
    val borderColor = when {
        focused -> MaterialTheme.colorScheme.primary
        enabled -> MaterialTheme.colorScheme.onSurface.copy(alpha = 0.28f)
        else -> MaterialTheme.colorScheme.onSurface.copy(alpha = 0.12f)
    }

    Surface(
        modifier = modifier
            .heightIn(min = minHeight)
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .onFocusChanged {
                focused = it.isFocused
                if (it.isFocused) onFocused()
            }
            .scale(scale)
            .tvRemoteActivation(enabled = enabled, onActivate = onClick)
            .semantics(mergeDescendants = true) {
                role = Role.Button
                contentDescription = semanticLabel
                if (enabled) {
                    onClick(label = semanticLabel) {
                        onClick()
                        true
                    }
                } else {
                    disabled()
                }
            }
            .focusable(enabled = enabled),
        shape = shape,
        color = colors.container,
        contentColor = colors.content,
        border = BorderStroke(width = if (focused) 3.dp else 1.dp, color = borderColor),
        tonalElevation = if (focused) 6.dp else 0.dp,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 24.dp, vertical = 14.dp),
            horizontalArrangement = Arrangement.Center,
            verticalAlignment = Alignment.CenterVertically,
            content = content,
        )
    }
}

@Composable
fun TvFocusableButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    style: TvFocusableStyle = TvFocusableStyle.Secondary,
    focusRequester: FocusRequester? = null,
    contentDescription: String = label,
    onFocused: () -> Unit = {},
    content: @Composable RowScope.() -> Unit = {
        Text(label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
    },
) {
    TvFocusableSurface(
        onClick = onClick,
        semanticLabel = contentDescription,
        modifier = modifier,
        enabled = enabled,
        style = style,
        focusRequester = focusRequester,
        onFocused = onFocused,
        content = content,
    )
}

enum class TvActionRole {
    Primary,
    Retry,
    Back,
    Cache,
    Recovery,
    SettingsExit,
    Destructive,
}

data class TvActionPanelAction(
    val key: String,
    val label: String,
    val role: TvActionRole,
    val enabled: Boolean = true,
    val busy: Boolean = false,
    val contentDescription: String = label,
    val onSelect: () -> Unit,
) {
    init {
        require(key.isNotBlank()) { "key must not be blank" }
        require(label.isNotBlank()) { "label must not be blank" }
        require(contentDescription.isNotBlank()) { "contentDescription must not be blank" }
    }
}

@Composable
fun TvActionPanel(
    actions: List<TvActionPanelAction>,
    modifier: Modifier = Modifier,
    title: String? = null,
    supportingText: String? = null,
    focusRestorer: TvFocusRestorer? = null,
    surfaceKey: String = "actions",
    autoFocus: Boolean = true,
    buttonMaxWidth: Dp = 420.dp,
) {
    if (actions.isEmpty()) return

    val keys = actions.map { it.key }
    val requesters = remember(keys) { actions.associate { it.key to FocusRequester() } }
    val enabledActions = actions.filter { it.enabled && !it.busy }
    val fallbackKey = enabledActions.firstOrNull()?.key
    val restoredKey = fallbackKey?.let { fallback ->
        focusRestorer?.restoreItem(
            surface = surfaceKey,
            availableItems = enabledActions.map { it.key },
            fallbackItem = fallback,
        ) ?: fallback
    }

    androidx.compose.runtime.LaunchedEffect(autoFocus, keys, restoredKey) {
        if (autoFocus && restoredKey != null) {
            runCatching {
                (requesters[restoredKey] ?: requesters[fallbackKey])?.requestFocus()
            }
        }
    }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .focusGroup(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        title?.let {
            Text(
                text = it,
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
                textAlign = TextAlign.Center,
            )
        }
        supportingText?.let {
            Text(
                text = it,
                style = MaterialTheme.typography.titleMedium,
                textAlign = TextAlign.Center,
            )
        }
        actions.forEach { action ->
            TvFocusableButton(
                label = action.label,
                enabled = action.enabled && !action.busy,
                style = action.role.focusableStyle(),
                contentDescription = action.contentDescription,
                onClick = action.onSelect,
                focusRequester = requesters[action.key],
                onFocused = { focusRestorer?.record(surface = surfaceKey, item = action.key) },
                modifier = Modifier
                    .widthIn(max = buttonMaxWidth)
                    .fillMaxWidth(),
            ) {
                if (action.busy) {
                    CircularProgressIndicator(
                        modifier = Modifier
                            .padding(end = 12.dp)
                            .size(22.dp),
                        strokeWidth = 2.dp,
                    )
                }
                Text(action.label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            }
        }
    }
}

private data class TvFocusableColors(
    val container: Color,
    val content: Color,
)

@Composable
private fun tvFocusableColors(
    style: TvFocusableStyle,
    focused: Boolean,
    enabled: Boolean,
): TvFocusableColors {
    val scheme = MaterialTheme.colorScheme
    if (!enabled) {
        return TvFocusableColors(
            container = scheme.onSurface.copy(alpha = 0.05f),
            content = scheme.onSurface.copy(alpha = 0.42f),
        )
    }

    return when (style) {
        TvFocusableStyle.Primary -> TvFocusableColors(
            container = if (focused) scheme.primary else scheme.primary.copy(alpha = 0.88f),
            content = scheme.onPrimary,
        )
        TvFocusableStyle.Secondary -> TvFocusableColors(
            container = if (focused) Color.White.copy(alpha = 0.2f) else Color.White.copy(alpha = 0.09f),
            content = scheme.onSurface,
        )
        TvFocusableStyle.Destructive -> TvFocusableColors(
            container = if (focused) scheme.error else scheme.error.copy(alpha = 0.78f),
            content = scheme.onError,
        )
    }
}

private fun TvActionRole.focusableStyle(): TvFocusableStyle = when (this) {
    TvActionRole.Primary,
    TvActionRole.Retry -> TvFocusableStyle.Primary
    TvActionRole.Destructive -> TvFocusableStyle.Destructive
    TvActionRole.Back,
    TvActionRole.Cache,
    TvActionRole.Recovery,
    TvActionRole.SettingsExit -> TvFocusableStyle.Secondary
}

private fun Modifier.tvRemoteActivation(
    enabled: Boolean,
    onActivate: () -> Unit,
): Modifier = if (!enabled) {
    this
} else {
    onPreviewKeyEvent { event ->
        if (!event.key.isTvActivationKey()) return@onPreviewKeyEvent false
        when (event.type) {
            KeyEventType.KeyDown -> true
            KeyEventType.KeyUp -> {
                onActivate()
                true
            }
            else -> false
        }
    }
}

private fun Key.isTvActivationKey(): Boolean = this == Key.DirectionCenter || this == Key.Enter || this == Key.NumPadEnter
