package com.ferrex.android.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import com.ferrex.android.ui.theme.FerrexDesignTokens

/** Semantic recovery/status tones used by phone and TV components. */
enum class FerrexStatusTone {
    Primary,
    Secondary,
    Retry,
    DestructiveReset,
    Cache,
    StaleOffline,
    Error,
}

/** Shared action roles for buttons that must keep recovery callbacks visible. */
enum class FerrexActionRole(val tone: FerrexStatusTone) {
    Primary(FerrexStatusTone.Primary),
    Secondary(FerrexStatusTone.Secondary),
    Retry(FerrexStatusTone.Retry),
    DestructiveReset(FerrexStatusTone.DestructiveReset),
    Cache(FerrexStatusTone.Cache),
    StaleOffline(FerrexStatusTone.StaleOffline),
    Error(FerrexStatusTone.Error),
}

fun FerrexActionRole.statusTone(): FerrexStatusTone = tone

@Immutable
data class FerrexStatusAction(
    val label: String,
    val subtitle: String? = null,
    val role: FerrexActionRole = FerrexActionRole.Primary,
    val enabled: Boolean = true,
    val onClick: () -> Unit,
)

@Immutable
data class FerrexStatusColors(
    val container: Color,
    val content: Color,
    val accent: Color,
    val border: Color,
)

@Composable
fun FerrexStatusTone.colors(): FerrexStatusColors {
    val scheme = MaterialTheme.colorScheme
    return when (this) {
        FerrexStatusTone.Primary -> FerrexStatusColors(
            container = scheme.primaryContainer.copy(alpha = FerrexDesignTokens.StatusAlpha.PrimaryContainer),
            content = scheme.onSurface,
            accent = scheme.primary,
            border = scheme.primary.copy(alpha = 0.72f),
        )
        FerrexStatusTone.Secondary -> FerrexStatusColors(
            container = scheme.surfaceVariant.copy(alpha = FerrexDesignTokens.StatusAlpha.SecondaryContainer),
            content = scheme.onSurface,
            accent = scheme.secondary,
            border = scheme.outline.copy(alpha = 0.65f),
        )
        FerrexStatusTone.Retry -> FerrexStatusColors(
            container = scheme.primaryContainer.copy(alpha = 0.24f),
            content = scheme.onSurface,
            accent = scheme.primary,
            border = scheme.primary,
        )
        FerrexStatusTone.DestructiveReset -> FerrexStatusColors(
            container = scheme.errorContainer.copy(alpha = 0.48f),
            content = scheme.onErrorContainer,
            accent = scheme.error,
            border = scheme.error,
        )
        FerrexStatusTone.Cache -> FerrexStatusColors(
            container = scheme.secondaryContainer.copy(alpha = 0.34f),
            content = scheme.onSurface,
            accent = scheme.secondary,
            border = scheme.secondary.copy(alpha = 0.8f),
        )
        FerrexStatusTone.StaleOffline -> FerrexStatusColors(
            container = scheme.surfaceVariant.copy(alpha = 0.52f),
            content = scheme.onSurfaceVariant,
            accent = FerrexDesignTokens.Palette.Offline,
            border = scheme.outline.copy(alpha = 0.52f),
        )
        FerrexStatusTone.Error -> FerrexStatusColors(
            container = scheme.errorContainer.copy(alpha = 0.58f),
            content = scheme.onErrorContainer,
            accent = scheme.error,
            border = scheme.error,
        )
    }
}

@Composable
fun FerrexStatusCard(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    tone: FerrexStatusTone = FerrexStatusTone.Secondary,
    loading: Boolean = false,
    action: FerrexStatusAction? = null,
    testTag: String? = null,
    contentDescription: String? = null,
) {
    val colors = tone.colors()
    Card(
        modifier = modifier
            .fillMaxWidth()
            .withFerrexTestTag(testTag)
            .withFerrexContentDescription(contentDescription),
        shape = FerrexDesignTokens.Shapes.RecoveryCard,
        colors = CardDefaults.cardColors(
            containerColor = colors.container,
            contentColor = colors.content,
        ),
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, colors.border.copy(alpha = 0.72f)),
    ) {
        Column(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Lg),
            verticalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(FerrexDesignTokens.Space.Sm),
            ) {
                if (loading) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(FerrexDesignTokens.Space.Xxl),
                        color = colors.accent,
                        strokeWidth = FerrexDesignTokens.Focus.TvRestingBorder,
                    )
                }
                TheaterPlateText(
                    text = title,
                    role = TheaterPlateTypographyRole.StatusTitle,
                    color = colors.accent,
                )
            }
            TheaterPlateText(
                text = body,
                role = TheaterPlateTypographyRole.StatusCopy,
                color = colors.content,
            )
            action?.let {
                FerrexActionButton(
                    label = it.label,
                    subtitle = it.subtitle,
                    role = it.role,
                    enabled = it.enabled,
                    onClick = it.onClick,
                )
            }
        }
    }
}

@Composable
fun FerrexActionButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    role: FerrexActionRole = FerrexActionRole.Primary,
    enabled: Boolean = true,
    subtitle: String? = null,
    testTag: String? = null,
    contentDescription: String = label,
    content: @Composable RowScope.() -> Unit = {
        FerrexActionButtonLabel(label = label, subtitle = subtitle)
    },
) {
    val statusColors = role.statusTone().colors()
    val shape = FerrexDesignTokens.Shapes.Button
    val actionModifier = modifier
        .withFerrexTestTag(testTag)
        .withFerrexContentDescription(contentDescription)
    when (role) {
        FerrexActionRole.Primary,
        FerrexActionRole.Retry -> Button(
            onClick = onClick,
            modifier = actionModifier,
            enabled = enabled,
            shape = shape,
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
                disabledContainerColor = MaterialTheme.colorScheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContainer),
                disabledContentColor = MaterialTheme.colorScheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContent),
            ),
            content = content,
        )
        FerrexActionRole.DestructiveReset,
        FerrexActionRole.Error -> Button(
            onClick = onClick,
            modifier = actionModifier,
            enabled = enabled,
            shape = shape,
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.error,
                contentColor = MaterialTheme.colorScheme.onError,
                disabledContainerColor = MaterialTheme.colorScheme.error.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContainer),
                disabledContentColor = MaterialTheme.colorScheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContent),
            ),
            content = content,
        )
        FerrexActionRole.Secondary,
        FerrexActionRole.Cache,
        FerrexActionRole.StaleOffline -> OutlinedButton(
            onClick = onClick,
            modifier = actionModifier,
            enabled = enabled,
            shape = shape,
            border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, statusColors.border),
            colors = ButtonDefaults.outlinedButtonColors(
                contentColor = statusColors.accent,
                disabledContentColor = MaterialTheme.colorScheme.onSurface.copy(alpha = FerrexDesignTokens.StatusAlpha.DisabledContent),
            ),
            content = content,
        )
    }
}

@Composable
private fun FerrexActionButtonLabel(label: String, subtitle: String?) {
    val contentColor = LocalContentColor.current
    if (subtitle == null) {
        TheaterPlateText(
            text = label,
            role = TheaterPlateTypographyRole.ActionLabel,
            color = contentColor,
        )
    } else {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            TheaterPlateText(
                text = label,
                role = TheaterPlateTypographyRole.ActionLabel,
                color = contentColor,
            )
            TheaterPlateText(
                text = subtitle,
                role = TheaterPlateTypographyRole.ActionSubtitle,
                color = contentColor.copy(alpha = 0.82f),
                maxLines = 2,
            )
        }
    }
}

@Composable
fun FerrexPosterCard(
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
    testTag: String? = null,
    contentDescription: String? = null,
    content: @Composable () -> Unit,
) {
    val baseModifier = modifier
        .withFerrexTestTag(testTag)
        .withFerrexContentDescription(contentDescription)
    val cardModifier = if (onClick != null) baseModifier.clickable(onClick = onClick) else baseModifier
    Card(
        modifier = cardModifier,
        shape = FerrexDesignTokens.Shapes.PosterCard,
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, MaterialTheme.colorScheme.outline.copy(alpha = 0.45f)),
    ) {
        content()
    }
}

@Composable
fun FerrexPosterPlaceholder(
    label: String,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .aspectRatio(FerrexDesignTokens.Poster.AspectRatio)
            .background(FerrexDesignTokens.Palette.PosterFallback, FerrexDesignTokens.Shapes.PosterImage),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
fun FerrexSectionTitle(title: String, modifier: Modifier = Modifier) {
    Text(
        text = title,
        modifier = modifier,
        style = MaterialTheme.typography.titleLarge,
        color = MaterialTheme.colorScheme.primary,
        fontWeight = FontWeight.SemiBold,
    )
}

private fun Modifier.withFerrexTestTag(tag: String?): Modifier = if (tag == null) this else testTag(tag)

private fun Modifier.withFerrexContentDescription(description: String?): Modifier = if (description == null) {
    this
} else {
    semantics(mergeDescendants = true) { contentDescription = description }
}
