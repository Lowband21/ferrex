package com.ferrex.android.ui.qa

import androidx.compose.ui.graphics.Color
import com.ferrex.android.ui.components.FerrexActionRole
import com.ferrex.android.ui.components.FerrexStatusTone
import com.ferrex.android.ui.theme.FerrexDesignTokens

/** Stable Compose semantics tags used by visual QA, accessibility smoke tests, and manual runbooks. */
object FerrexQaTags {
    object Phone {
        const val Shell = "phone.shell"
        const val ShellNav = "phone.shell.nav"
        const val Home = "phone.home"
        const val HomeHeader = "phone.home.header"
        const val ContinueWatching = "phone.home.continue-watching"
        const val BrowseFind = "phone.home.browse-find"
        const val ServerRecovery = "phone.home.server-recovery"
        const val Libraries = "phone.libraries"
        const val LibraryTabs = "phone.libraries.tabs"
        const val LibraryChooser = "phone.libraries.chooser"
        const val LibraryGrid = "phone.libraries.grid"
        const val LibraryRecovery = "phone.library.recovery"
        const val Search = "phone.search"
        const val SearchPanel = "phone.search.panel"
        const val SearchField = "phone.search.field"
        const val SearchActions = "phone.search.actions"
        const val SearchResults = "phone.search.results"
        const val AccountServer = "phone.account-server"
        const val AccountSummary = "phone.account-server.summary"

        fun navItem(destination: String): String = namespaced("phone", "shell", "nav", destination)
    }

    object Tv {
        const val Home = "tv.home"
        const val Search = "tv.search"
        const val SearchField = "tv.search.field"
        const val SearchResults = "tv.search.results"
        const val Detail = "tv.detail"

        fun surface(surfaceKey: String): String = namespaced("tv", "surface", surfaceKey)
        fun action(surfaceKey: String, actionKey: String): String = namespaced("tv", "action", surfaceKey, actionKey)
        fun poster(surfaceKey: String, itemKey: String): String = namespaced("tv", "poster", surfaceKey, itemKey)
    }

    object Shared {
        fun statusCard(id: String): String = namespaced("status-card", id)
    }

    fun namespaced(vararg parts: String): String = parts.joinToString(separator = ".") { segment(it) }

    fun segment(raw: String): String = tagUnsafeCharacters
        .replace(raw.lowercase(), "-")
        .trim('-')
        .ifBlank { "item" }

    private val tagUnsafeCharacters = Regex("[^a-z0-9_-]+")
}

data class VisualQaSurfaceSample(
    val id: String,
    val testTag: String,
    val contentDescription: String,
    val evidencePath: String,
)

data class VisualQaStatusToneSample(
    val id: String,
    val tone: FerrexStatusTone,
    val actionRole: FerrexActionRole,
    val container: Color,
    val content: Color,
    val accent: Color,
    val blendBackground: Color,
    val testTag: String,
    val contentDescription: String,
)

/** Deterministic sample states consumed by unit checks and manual visual QA documentation. */
object FerrexVisualQaSamples {
    val phoneSurfaces = listOf(
        VisualQaSurfaceSample(
            id = "phone-home",
            testTag = FerrexQaTags.Phone.Home,
            contentDescription = "Phone Home surface with resume, browse, and recovery sections",
            evidencePath = "Home → Resume / Browse and find / Server & recovery",
        ),
        VisualQaSurfaceSample(
            id = "phone-libraries",
            testTag = FerrexQaTags.Phone.Libraries,
            contentDescription = "Phone Libraries surface with tabs, chooser, grid, and cache recovery",
            evidencePath = "Libraries → Movie/Series tabs → Library chooser → Grid → Recovery panel",
        ),
        VisualQaSurfaceSample(
            id = "phone-search",
            testTag = FerrexQaTags.Phone.Search,
            contentDescription = "Phone Search surface with query field, retry actions, and result rows",
            evidencePath = "Search → query field → Retry/Clear → results or cache-miss recovery",
        ),
        VisualQaSurfaceSample(
            id = "phone-account-server",
            testTag = FerrexQaTags.Phone.AccountServer,
            contentDescription = "Phone Account and Server recovery surface",
            evidencePath = "Account & Server → retry/change/sign out/reset/diagnostics actions",
        ),
    )

    val tvFocusableSurfaces = listOf(
        VisualQaSurfaceSample(
            id = "tv-home-actions",
            testTag = FerrexQaTags.Tv.surface("home-actions"),
            contentDescription = "TV Home action row with search, diagnostics, and retry focus targets",
            evidencePath = "TV Home → D-pad to Home actions → OK/Back",
        ),
        VisualQaSurfaceSample(
            id = "tv-continue-watching",
            testTag = FerrexQaTags.Tv.surface("continue-watching"),
            contentDescription = "TV Continue Watching row with focusable poster cards",
            evidencePath = "TV Home → Continue Watching row → poster focus/OK",
        ),
        VisualQaSurfaceSample(
            id = "tv-library-actions",
            testTag = FerrexQaTags.Tv.surface("library-actions"),
            contentDescription = "TV Library action row with browse and sync controls",
            evidencePath = "TV Home → Library → Browse all / Retry selected library",
        ),
        VisualQaSurfaceSample(
            id = "tv-recovery-actions",
            testTag = FerrexQaTags.Tv.surface("recovery-actions"),
            contentDescription = "TV recovery action panel with retry, cache, sign-out, change-server, reset, and diagnostics exits",
            evidencePath = "TV Home/Grid/Detail → recovery actions → no OS app-data wipe required",
        ),
        VisualQaSurfaceSample(
            id = "tv-search",
            testTag = FerrexQaTags.Tv.Search,
            contentDescription = "TV Search screen with field, actions, result rows, and cache-miss recovery",
            evidencePath = "TV Search → field → Retry/Clear → results/cache-miss recovery",
        ),
        VisualQaSurfaceSample(
            id = "tv-detail",
            testTag = FerrexQaTags.Tv.Detail,
            contentDescription = "TV Detail screen with back, playback, watch-state, and recovery actions",
            evidencePath = "TV Detail → Back → Playback/watch actions → recovery panel",
        ),
    )

    val statusToneSamples = listOf(
        VisualQaStatusToneSample(
            id = "primary",
            tone = FerrexStatusTone.Primary,
            actionRole = FerrexActionRole.Primary,
            container = FerrexDesignTokens.Palette.SignalCyanDim.copy(alpha = FerrexDesignTokens.StatusAlpha.PrimaryContainer),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.SignalCyan,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("primary"),
            contentDescription = "Primary action/status tone",
        ),
        VisualQaStatusToneSample(
            id = "secondary",
            tone = FerrexStatusTone.Secondary,
            actionRole = FerrexActionRole.Secondary,
            container = FerrexDesignTokens.Palette.SlateElevated.copy(alpha = FerrexDesignTokens.StatusAlpha.SecondaryContainer),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.PrivateViolet,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("secondary"),
            contentDescription = "Secondary action/status tone",
        ),
        VisualQaStatusToneSample(
            id = "retry",
            tone = FerrexStatusTone.Retry,
            actionRole = FerrexActionRole.Retry,
            container = FerrexDesignTokens.Palette.SignalCyanDim.copy(alpha = 0.24f),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.SignalCyan,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("retry"),
            contentDescription = "Retry recovery tone",
        ),
        VisualQaStatusToneSample(
            id = "destructive-reset",
            tone = FerrexStatusTone.DestructiveReset,
            actionRole = FerrexActionRole.DestructiveReset,
            container = FerrexDesignTokens.Palette.ErrorDim.copy(alpha = 0.48f),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.Error,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("destructive-reset"),
            contentDescription = "Destructive reset recovery tone",
        ),
        VisualQaStatusToneSample(
            id = "cache",
            tone = FerrexStatusTone.Cache,
            actionRole = FerrexActionRole.Cache,
            container = FerrexDesignTokens.Palette.PrivateVioletDim.copy(alpha = 0.34f),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.PrivateViolet,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("cache"),
            contentDescription = "Cache repair tone",
        ),
        VisualQaStatusToneSample(
            id = "stale-offline",
            tone = FerrexStatusTone.StaleOffline,
            actionRole = FerrexActionRole.StaleOffline,
            container = FerrexDesignTokens.Palette.SlateElevated.copy(alpha = 0.52f),
            content = FerrexDesignTokens.Palette.TextSecondary,
            accent = FerrexDesignTokens.Palette.TextMuted,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("stale-offline"),
            contentDescription = "Stale or offline tone",
        ),
        VisualQaStatusToneSample(
            id = "error",
            tone = FerrexStatusTone.Error,
            actionRole = FerrexActionRole.Error,
            container = FerrexDesignTokens.Palette.ErrorDim.copy(alpha = 0.58f),
            content = FerrexDesignTokens.Palette.TextPrimary,
            accent = FerrexDesignTokens.Palette.Error,
            blendBackground = FerrexDesignTokens.Palette.SlatePanel,
            testTag = FerrexQaTags.Shared.statusCard("error"),
            contentDescription = "Error tone",
        ),
    )
}
