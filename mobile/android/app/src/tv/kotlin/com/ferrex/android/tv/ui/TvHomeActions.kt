package com.ferrex.android.tv.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.testTag
import com.ferrex.android.tv.ui.foundation.TvActionRole
import com.ferrex.android.tv.ui.foundation.TvFocusableButton
import com.ferrex.android.tv.ui.foundation.TvFocusableStyle
import com.ferrex.android.tv.ui.foundation.TvFocusRestorer
import com.ferrex.android.ui.components.TheaterPlateDensityRole
import com.ferrex.android.ui.components.TheaterPlateText
import com.ferrex.android.ui.components.TheaterPlateTypographyRole
import com.ferrex.android.ui.components.statusTone
import com.ferrex.android.ui.qa.FerrexQaTags
import com.ferrex.android.ui.theaterplate.FerrexStageDensityFamily
import com.ferrex.android.ui.theaterplate.FerrexStageSurface
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceTone
import com.ferrex.android.ui.theaterplate.FerrexStageSurfaceVariant
import com.ferrex.android.ui.theme.FerrexDesignTokens

@Composable
internal fun TvButtonRow(
    actions: List<TvButtonAction>,
    modifier: Modifier = Modifier,
    title: String? = null,
    supportingText: String? = null,
    focusRestorer: TvFocusRestorer? = null,
    surfaceKey: String = "actions",
    autoFocus: Boolean = false,
) {
    if (actions.isEmpty()) return
    val keys = actions.map { it.key }
    val requesters = remember(keys) { actions.associate { it.key to FocusRequester() } }
    val enabledKeys = actions.filter { it.enabled }.map { it.key }
    val restoredKey = enabledKeys.firstOrNull()?.let { fallback ->
        focusRestorer?.restoreItem(surfaceKey, enabledKeys, fallback) ?: fallback
    }
    LaunchedEffect(autoFocus, restoredKey, keys) {
        if (autoFocus && restoredKey != null) {
            runCatching { requesters[restoredKey]?.requestFocus() }
        }
    }
    Column(
        modifier = modifier
            .fillMaxWidth()
            .testTag(FerrexQaTags.Tv.surface(surfaceKey)),
        verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
    ) {
        title?.let { TvSectionHeader(it) }
        supportingText?.takeIf { it.isNotBlank() }?.let {
            TheaterPlateText(
                text = it,
                role = TheaterPlateTypographyRole.ActionSubtitle,
                densityRole = TheaterPlateDensityRole.Tv1080p,
            )
        }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            actions.forEach { action ->
                TvFocusableButton(
                    label = action.label,
                    onClick = action.onSelect,
                    enabled = action.enabled,
                    style = action.role.toFocusableStyle(),
                    tone = action.role.sharedActionRole.statusTone(),
                    focusRequester = requesters[action.key],
                    testTag = FerrexQaTags.Tv.action(surfaceKey, action.key),
                    onFocused = { focusRestorer?.record(surfaceKey, action.key) },
                    modifier = Modifier.widthIn(
                        min = FerrexDesignTokens.Tv.ActionMinWidth,
                        max = FerrexDesignTokens.Tv.ActionMaxWidth,
                    ),
                )
            }
        }
    }
}

@Composable
internal fun TvStateCopy(title: String, body: String, loading: Boolean = false) {
    val isError = title.contains("failed", ignoreCase = true) || title.contains("unavailable", ignoreCase = true)
    FerrexStageSurface(
        variant = if (loading) FerrexStageSurfaceVariant.StatusSlab else FerrexStageSurfaceVariant.EmptyState,
        density = FerrexStageDensityFamily.TenFoot,
        tone = if (isError) FerrexStageSurfaceTone.Error else FerrexStageSurfaceTone.StaleOffline,
        modifier = Modifier.fillMaxWidth(),
        contentDescription = title,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Xs)) {
            TheaterPlateText(
                text = title,
                role = TheaterPlateTypographyRole.StatusTitle,
                densityRole = TheaterPlateDensityRole.Tv1080p,
            )
            TheaterPlateText(
                text = if (loading) "$body Loading…" else body,
                role = TheaterPlateTypographyRole.StatusCopy,
                densityRole = TheaterPlateDensityRole.Tv1080p,
            )
        }
    }
}

@Composable
internal fun TvSectionHeader(title: String) {
    TheaterPlateText(
        text = title,
        role = TheaterPlateTypographyRole.SectionTitle,
        densityRole = TheaterPlateDensityRole.Tv1080p,
    )
}

@Composable
internal fun TvFullScreenSurface(content: @Composable BoxScope.() -> Unit) {
    Surface(
        modifier = Modifier.fillMaxSize().background(FerrexDesignTokens.Palette.SlateCanvas),
        color = MaterialTheme.colorScheme.background,
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(FerrexDesignTokens.privateCinemaGradient())
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(
                    horizontal = FerrexDesignTokens.Space.ScreenTvHorizontal,
                    vertical = FerrexDesignTokens.Tv.FullScreenVerticalPadding,
                ),
            content = content,
        )
    }
}

internal fun TvActionRole.toFocusableStyle(): TvFocusableStyle = when (this) {
    TvActionRole.Primary,
    TvActionRole.Retry -> TvFocusableStyle.Primary
    TvActionRole.Destructive -> TvFocusableStyle.Destructive
    TvActionRole.Back,
    TvActionRole.Cache,
    TvActionRole.Recovery,
    TvActionRole.SettingsExit -> TvFocusableStyle.Secondary
}

internal data class TvButtonAction(
    val key: String,
    val label: String,
    val role: TvActionRole,
    val enabled: Boolean = true,
    val onSelect: () -> Unit,
)
