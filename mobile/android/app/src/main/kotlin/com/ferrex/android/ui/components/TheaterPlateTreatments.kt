package com.ferrex.android.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.ferrex.android.ui.theme.FerrexDesignTokens

/** Responsive density roles used by Theater Plate phone, foldable-ish, and TV treatments. */
enum class TheaterPlateDensityRole(val key: String) {
    PhonePortrait("phone-portrait"),
    PhoneLandscape("phone-landscape-foldable"),
    Tv1080p("tv-1080p"),
    Tv4kScaled("tv-4k-scaled"),
}

@Immutable
data class TheaterPlateDensitySpec(
    val role: TheaterPlateDensityRole,
    val contentMaxWidth: Dp,
    val horizontalPadding: Dp,
    val verticalPadding: Dp,
    val actionSpacing: Dp,
    val railSpacing: Dp,
    val minInteractiveHeight: Dp,
    val typeScale: Float,
)

fun TheaterPlateDensityRole.spec(): TheaterPlateDensitySpec = when (this) {
    TheaterPlateDensityRole.PhonePortrait -> TheaterPlateDensitySpec(
        role = this,
        contentMaxWidth = 640.dp,
        horizontalPadding = FerrexDesignTokens.Space.ScreenPhoneHorizontal,
        verticalPadding = FerrexDesignTokens.Space.ScreenPhoneVertical,
        actionSpacing = FerrexDesignTokens.Space.Sm,
        railSpacing = FerrexDesignTokens.Space.Md,
        minInteractiveHeight = 48.dp,
        typeScale = 1.0f,
    )
    TheaterPlateDensityRole.PhoneLandscape -> TheaterPlateDensitySpec(
        role = this,
        contentMaxWidth = 920.dp,
        horizontalPadding = 32.dp,
        verticalPadding = FerrexDesignTokens.Space.Xxl,
        actionSpacing = FerrexDesignTokens.Space.Md,
        railSpacing = FerrexDesignTokens.Space.Lg,
        minInteractiveHeight = 48.dp,
        typeScale = 1.05f,
    )
    TheaterPlateDensityRole.Tv1080p -> TheaterPlateDensitySpec(
        role = this,
        contentMaxWidth = FerrexDesignTokens.Tv.DetailMaxWidth,
        horizontalPadding = FerrexDesignTokens.Space.ScreenTvHorizontal,
        verticalPadding = FerrexDesignTokens.Space.ScreenTvVertical,
        actionSpacing = FerrexDesignTokens.Space.Lg,
        railSpacing = FerrexDesignTokens.Space.Xl,
        minInteractiveHeight = FerrexDesignTokens.Focus.TvButtonMinHeight,
        typeScale = 1.18f,
    )
    TheaterPlateDensityRole.Tv4kScaled -> TheaterPlateDensitySpec(
        role = this,
        contentMaxWidth = 1800.dp,
        horizontalPadding = 96.dp,
        verticalPadding = 72.dp,
        actionSpacing = FerrexDesignTokens.Space.Xxl,
        railSpacing = FerrexDesignTokens.Space.Xxxl,
        minInteractiveHeight = 64.dp,
        typeScale = 1.32f,
    )
}

fun theaterPlateDensityForViewport(
    widthDp: Int,
    heightDp: Int,
    isTv: Boolean,
): TheaterPlateDensityRole {
    val safeWidth = widthDp.coerceAtLeast(1)
    val safeHeight = heightDp.coerceAtLeast(1)
    return if (isTv) {
        if (safeWidth >= 2560 || safeHeight >= 1440) {
            TheaterPlateDensityRole.Tv4kScaled
        } else {
            TheaterPlateDensityRole.Tv1080p
        }
    } else if (safeWidth > safeHeight || safeWidth >= 700) {
        TheaterPlateDensityRole.PhoneLandscape
    } else {
        TheaterPlateDensityRole.PhonePortrait
    }
}

enum class TheaterPlateTypographyGroup {
    Hero,
    Metadata,
    Section,
    Fact,
    Rail,
    Action,
    Status,
    Recovery,
    TvFocus,
}

/** Editorial Theater Plate text roles shared by phone and TV surfaces. */
enum class TheaterPlateTypographyRole(
    val key: String,
    val group: TheaterPlateTypographyGroup,
) {
    HeroEyebrow("hero-eyebrow", TheaterPlateTypographyGroup.Hero),
    HeroTitle("hero-title", TheaterPlateTypographyGroup.Hero),
    HeroSubtitle("hero-subtitle", TheaterPlateTypographyGroup.Hero),
    HeroBody("hero-body", TheaterPlateTypographyGroup.Hero),
    Metadata("metadata", TheaterPlateTypographyGroup.Metadata),
    SectionTitle("section-title", TheaterPlateTypographyGroup.Section),
    FactLabel("fact-label", TheaterPlateTypographyGroup.Fact),
    FactValue("fact-value", TheaterPlateTypographyGroup.Fact),
    RailTitle("rail-title", TheaterPlateTypographyGroup.Rail),
    RailSubtitle("rail-subtitle", TheaterPlateTypographyGroup.Rail),
    ActionLabel("action-label", TheaterPlateTypographyGroup.Action),
    ActionSubtitle("action-subtitle", TheaterPlateTypographyGroup.Action),
    StatusTitle("status-title", TheaterPlateTypographyGroup.Status),
    StatusCopy("status-copy", TheaterPlateTypographyGroup.Status),
    RecoveryTitle("recovery-title", TheaterPlateTypographyGroup.Recovery),
    RecoveryCopy("recovery-copy", TheaterPlateTypographyGroup.Recovery),
    TvFocusHelperLabel("tv-focus-helper-label", TheaterPlateTypographyGroup.TvFocus),
}

fun TheaterPlateTypographyRole.defaultMaxLines(densityRole: TheaterPlateDensityRole): Int = when (this) {
    TheaterPlateTypographyRole.HeroTitle -> if (densityRole == TheaterPlateDensityRole.PhonePortrait) 3 else 2
    TheaterPlateTypographyRole.HeroBody,
    TheaterPlateTypographyRole.StatusCopy,
    TheaterPlateTypographyRole.RecoveryCopy -> when (densityRole) {
        TheaterPlateDensityRole.Tv1080p,
        TheaterPlateDensityRole.Tv4kScaled -> 4
        TheaterPlateDensityRole.PhonePortrait,
        TheaterPlateDensityRole.PhoneLandscape -> 5
    }
    TheaterPlateTypographyRole.RailSubtitle,
    TheaterPlateTypographyRole.ActionSubtitle,
    TheaterPlateTypographyRole.TvFocusHelperLabel -> 2
    else -> 1
}

@Composable
fun TheaterPlateText(
    text: String,
    role: TheaterPlateTypographyRole,
    modifier: Modifier = Modifier,
    densityRole: TheaterPlateDensityRole = TheaterPlateDensityRole.PhonePortrait,
    color: Color = role.defaultColor(),
    textAlign: TextAlign? = null,
    maxLines: Int = role.defaultMaxLines(densityRole),
    overflow: TextOverflow = TextOverflow.Ellipsis,
) {
    Text(
        text = text,
        modifier = modifier,
        style = role.textStyle(densityRole),
        color = color,
        textAlign = textAlign,
        maxLines = maxLines,
        overflow = overflow,
    )
}

@Composable
private fun TheaterPlateTypographyRole.textStyle(densityRole: TheaterPlateDensityRole): TextStyle {
    val typography = MaterialTheme.typography
    return when (this) {
        TheaterPlateTypographyRole.HeroEyebrow -> typography.labelLarge.copy(fontWeight = FontWeight.SemiBold)
        TheaterPlateTypographyRole.HeroTitle -> when (densityRole) {
            TheaterPlateDensityRole.PhonePortrait -> typography.headlineLarge
            TheaterPlateDensityRole.PhoneLandscape -> typography.displaySmall
            TheaterPlateDensityRole.Tv1080p,
            TheaterPlateDensityRole.Tv4kScaled -> typography.displayLarge
        }
        TheaterPlateTypographyRole.HeroSubtitle -> typography.titleLarge
        TheaterPlateTypographyRole.HeroBody -> typography.bodyLarge
        TheaterPlateTypographyRole.Metadata -> typography.labelMedium
        TheaterPlateTypographyRole.SectionTitle -> typography.titleLarge.copy(fontWeight = FontWeight.SemiBold)
        TheaterPlateTypographyRole.FactLabel -> typography.labelMedium
        TheaterPlateTypographyRole.FactValue -> typography.bodyMedium.copy(fontWeight = FontWeight.SemiBold)
        TheaterPlateTypographyRole.RailTitle -> typography.titleMedium.copy(fontWeight = FontWeight.SemiBold)
        TheaterPlateTypographyRole.RailSubtitle -> typography.bodySmall
        TheaterPlateTypographyRole.ActionLabel -> typography.labelLarge.copy(fontWeight = FontWeight.SemiBold)
        TheaterPlateTypographyRole.ActionSubtitle -> typography.labelSmall
        TheaterPlateTypographyRole.StatusTitle -> typography.titleSmall.copy(fontWeight = FontWeight.SemiBold)
        TheaterPlateTypographyRole.StatusCopy -> typography.bodyMedium
        TheaterPlateTypographyRole.RecoveryTitle -> typography.titleMedium.copy(fontWeight = FontWeight.SemiBold)
        TheaterPlateTypographyRole.RecoveryCopy -> typography.bodyMedium
        TheaterPlateTypographyRole.TvFocusHelperLabel -> typography.labelMedium.copy(fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun TheaterPlateTypographyRole.defaultColor(): Color {
    val scheme = MaterialTheme.colorScheme
    return when (this) {
        TheaterPlateTypographyRole.HeroEyebrow,
        TheaterPlateTypographyRole.SectionTitle,
        TheaterPlateTypographyRole.RailTitle,
        TheaterPlateTypographyRole.StatusTitle,
        TheaterPlateTypographyRole.RecoveryTitle -> scheme.primary
        TheaterPlateTypographyRole.ActionLabel,
        TheaterPlateTypographyRole.HeroTitle,
        TheaterPlateTypographyRole.HeroSubtitle,
        TheaterPlateTypographyRole.FactValue -> scheme.onSurface
        TheaterPlateTypographyRole.Metadata,
        TheaterPlateTypographyRole.HeroBody,
        TheaterPlateTypographyRole.FactLabel,
        TheaterPlateTypographyRole.RailSubtitle,
        TheaterPlateTypographyRole.ActionSubtitle,
        TheaterPlateTypographyRole.StatusCopy,
        TheaterPlateTypographyRole.RecoveryCopy,
        TheaterPlateTypographyRole.TvFocusHelperLabel -> scheme.onSurfaceVariant
    }
}

enum class FerrexRecoveryActionKind(
    val key: String,
    val defaultLabel: String,
    val defaultSubtitle: String,
    val role: FerrexActionRole,
) {
    Retry(
        key = "retry",
        defaultLabel = "Retry",
        defaultSubtitle = "Try the request again without changing saved data.",
        role = FerrexActionRole.Retry,
    ),
    SignOut(
        key = "sign-out",
        defaultLabel = "Sign out",
        defaultSubtitle = "Clear the local session and return to sign in.",
        role = FerrexActionRole.Secondary,
    ),
    ChangeServer(
        key = "change-server",
        defaultLabel = "Change server",
        defaultSubtitle = "Use a different server URL while preserving in-app recovery.",
        role = FerrexActionRole.Secondary,
    ),
    ClearCache(
        key = "clear-cache",
        defaultLabel = "Clear cache",
        defaultSubtitle = "Remove scoped media or image cache only.",
        role = FerrexActionRole.Cache,
    ),
    ResetConnection(
        key = "reset-connection",
        defaultLabel = "Reset connection",
        defaultSubtitle = "Clear saved connection state and scoped caches, not OS app data.",
        role = FerrexActionRole.DestructiveReset,
    ),
    Diagnostics(
        key = "diagnostics",
        defaultLabel = "Diagnostics / Export diagnostics",
        defaultSubtitle = "Export redacted diagnostics for support.",
        role = FerrexActionRole.Secondary,
    ),
}

@Immutable
data class FerrexRecoveryActionDescriptor(
    val kind: FerrexRecoveryActionKind,
    val key: String = kind.key,
    val label: String = kind.defaultLabel,
    val subtitle: String = kind.defaultSubtitle,
    val role: FerrexActionRole = kind.role,
) {
    init {
        require(key.isNotBlank()) { "recovery action key must not be blank" }
        require(label.isNotBlank()) { "recovery action label must not be blank" }
        require(subtitle.isNotBlank()) { "recovery action subtitle must not be blank" }
    }

    val requiresAppDataWipe: Boolean = false
    val tone: FerrexStatusTone get() = role.statusTone()
}

@Immutable
data class FerrexRecoveryPanelAction(
    val descriptor: FerrexRecoveryActionDescriptor,
    val enabled: Boolean = true,
    val onClick: () -> Unit,
)

fun FerrexRecoveryActionKind.descriptor(): FerrexRecoveryActionDescriptor = FerrexRecoveryActionDescriptor(this)

fun requiredTheaterPlateRecoveryActions(includeCacheClear: Boolean = true): List<FerrexRecoveryActionDescriptor> = buildList {
    add(FerrexRecoveryActionKind.Retry.descriptor())
    add(FerrexRecoveryActionKind.SignOut.descriptor())
    add(FerrexRecoveryActionKind.ChangeServer.descriptor())
    if (includeCacheClear) add(FerrexRecoveryActionKind.ClearCache.descriptor())
    add(FerrexRecoveryActionKind.ResetConnection.descriptor())
    add(FerrexRecoveryActionKind.Diagnostics.descriptor())
}

@Composable
fun FerrexRecoveryActionPanel(
    title: String,
    body: String,
    actions: List<FerrexRecoveryPanelAction>,
    modifier: Modifier = Modifier,
    densityRole: TheaterPlateDensityRole = TheaterPlateDensityRole.PhonePortrait,
    tone: FerrexStatusTone = FerrexStatusTone.StaleOffline,
    testTag: String? = null,
    contentDescription: String? = null,
) {
    val colors = tone.colors()
    Card(
        modifier = modifier
            .fillMaxWidth()
            .withOptionalTheaterPlateTag(testTag)
            .withOptionalTheaterPlateContentDescription(contentDescription),
        shape = FerrexDesignTokens.Shapes.RecoveryCard,
        colors = CardDefaults.cardColors(
            containerColor = colors.container,
            contentColor = colors.content,
        ),
        border = BorderStroke(FerrexDesignTokens.Focus.TvRestingBorder, colors.border.copy(alpha = 0.72f)),
    ) {
        Column(
            modifier = Modifier.padding(FerrexDesignTokens.Space.Lg),
            verticalArrangement = Arrangement.spacedBy(densityRole.spec().actionSpacing),
        ) {
            TheaterPlateText(
                text = title,
                role = TheaterPlateTypographyRole.RecoveryTitle,
                densityRole = densityRole,
                color = colors.accent,
            )
            TheaterPlateText(
                text = body,
                role = TheaterPlateTypographyRole.RecoveryCopy,
                densityRole = densityRole,
                color = colors.content,
            )
            actions.forEach { action ->
                FerrexActionButton(
                    label = action.descriptor.label,
                    subtitle = action.descriptor.subtitle,
                    role = action.descriptor.role,
                    enabled = action.enabled,
                    onClick = action.onClick,
                    modifier = Modifier.fillMaxWidth(),
                    contentDescription = action.descriptor.label,
                )
            }
        }
    }
}

private fun Modifier.withOptionalTheaterPlateTag(tag: String?): Modifier = if (tag == null) this else testTag(tag)

private fun Modifier.withOptionalTheaterPlateContentDescription(description: String?): Modifier = if (description == null) {
    this
} else {
    semantics(mergeDescendants = true) { contentDescription = description }
}
