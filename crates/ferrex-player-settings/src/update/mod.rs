//! UI-agnostic settings reducers and effect descriptions.
//!
//! The desktop player maps these effects to runtime tasks and cross-domain
//! events, while this crate owns the settings state mutations and validation.

pub mod device_management;
pub mod navigation;
pub mod preferences;
pub mod profile;
pub mod security;

use ferrex_core::player_prelude::UserScale;
use ferrex_player_auth::{messages::AuthCommand, pin_policy::PinPolicyRules};
use iced_core::Color;
use uuid::Uuid;

use crate::{
    ScalePreset, SettingsMessage, SettingsSection,
    sections::{
        devices::state::DeviceManagementState,
        display::{DisplayMessage, DisplayState},
        performance::{PerformanceMessage, PerformanceState},
        playback::{PlaybackMessage, PlaybackState},
        theme::{ThemeMessage, ThemeState},
    },
    state::{PreferencesState, ProfileState, SecurityState},
};

/// Runtime inputs the reducer needs from adjacent domains.
#[derive(Debug, Clone, Copy)]
pub struct SettingsUpdateContext {
    /// Active PIN policy used to filter and validate PIN edits.
    pub pin_policy: PinPolicyRules,
}

impl Default for SettingsUpdateContext {
    fn default() -> Self {
        Self {
            pin_policy: PinPolicyRules::default(),
        }
    }
}

/// Mutable settings state borrowed by the top-level reducer.
#[derive(Debug)]
pub struct SettingsUpdateTarget<'a> {
    pub current_section: &'a mut SettingsSection,
    pub security: &'a mut SecurityState,
    pub profile: &'a mut ProfileState,
    pub preferences: &'a mut PreferencesState,
    pub device_management: &'a mut DeviceManagementState,
    pub playback: &'a mut PlaybackState,
    pub display: &'a mut DisplayState,
    pub theme: &'a mut ThemeState,
    pub performance: &'a mut PerformanceState,
}

/// Side effects requested by settings reducers.
#[derive(Debug, Clone)]
pub enum SettingsEffect {
    /// Ask the auth service whether the active user has a PIN configured.
    CheckUserHasPin,
    /// Load the active user's trusted devices.
    LoadDevices,
    /// Revoke a trusted device through the settings service.
    RevokeDevice { device_id: Uuid, original: String },
    /// Persist auto-login scope changes through the auth service.
    ToggleAutoLogin { enabled: bool },
    /// Execute an auth-domain command through the shell event bus.
    AuthCommandRequested(AuthCommand),
    /// Apply a user scale change to app/UI runtime state.
    ApplyUserScale(UserScale),
    /// Apply a named scale preset to app/UI runtime state.
    ApplyScalePreset(ScalePreset),
    /// Recompute layout caches after settings changed grid dimensions.
    RefreshGridLayout,
    /// Apply a live accent color preview.
    ApplyAccentColor(Color),
    /// Apply runtime hover animation tuning to the poster shader/config.
    ApplyHoverAnimation {
        scale: Option<f32>,
        transition_ms: Option<u64>,
        scale_down_delay_ms: Option<u64>,
    },
    /// Submit profile changes through an app-provided profile service.
    SubmitProfileChanges,
    /// Request logout from the shell/auth boundary.
    Logout,
    /// Request a user-switch flow from the shell/auth boundary.
    SwitchUser,
}

/// Reducer result containing side effects for the app shell to run.
#[derive(Debug, Clone, Default)]
pub struct SettingsUpdate {
    pub effects: Vec<SettingsEffect>,
}

impl SettingsUpdate {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn effect(effect: SettingsEffect) -> Self {
        Self {
            effects: vec![effect],
        }
    }

    pub fn push(&mut self, effect: SettingsEffect) {
        self.effects.push(effect);
    }

    pub fn extend(&mut self, other: Self) {
        self.effects.extend(other.effects);
    }
}

/// Reduce a top-level settings message into state changes plus shell effects.
pub fn update_settings(
    target: SettingsUpdateTarget<'_>,
    message: SettingsMessage,
    context: SettingsUpdateContext,
) -> SettingsUpdate {
    match message {
        SettingsMessage::NavigateToSection(section) => navigation::navigate(
            target.current_section,
            target.security,
            section,
        ),
        SettingsMessage::Playback(message) => {
            reduce_playback(target.playback, message)
        }
        SettingsMessage::Display(message) => {
            reduce_display(target.display, message)
        }
        SettingsMessage::Theme(message) => reduce_theme(target.theme, message),
        SettingsMessage::Performance(message) => {
            reduce_performance(target.performance, message)
        }
        SettingsMessage::SetUserScale(user_scale) => {
            preferences::set_user_scale(target.preferences, user_scale)
        }
        SettingsMessage::SetScalePreset(preset) => {
            preferences::set_scale_preset(preset)
        }
        SettingsMessage::ShowChangePassword => {
            security::show_change_password(target.security)
        }
        SettingsMessage::UpdatePasswordCurrent(value) => {
            security::update_password_current(target.security, value)
        }
        SettingsMessage::UpdatePasswordNew(value) => {
            security::update_password_new(target.security, value)
        }
        SettingsMessage::UpdatePasswordConfirm(value) => {
            security::update_password_confirm(target.security, value)
        }
        SettingsMessage::TogglePasswordVisibility => {
            security::toggle_password_visibility(target.security)
        }
        SettingsMessage::SubmitPasswordChange => {
            security::submit_password_change(target.security)
        }
        SettingsMessage::PasswordChangeResult(result) => {
            security::password_change_result(target.security, result)
        }
        SettingsMessage::CancelPasswordChange => {
            security::cancel_password_change(target.security)
        }
        SettingsMessage::CheckUserHasPin => security::check_user_has_pin(),
        SettingsMessage::UserHasPinResult(has_pin) => {
            security::user_has_pin_result(target.security, has_pin)
        }
        SettingsMessage::ShowSetPin => security::show_set_pin(target.security),
        SettingsMessage::ShowChangePin => {
            security::show_change_pin(target.security)
        }
        SettingsMessage::UpdatePinCurrent(value) => {
            security::update_pin_current(
                target.security,
                value,
                context.pin_policy,
            )
        }
        SettingsMessage::UpdatePinNew(value) => {
            security::update_pin_new(target.security, value, context.pin_policy)
        }
        SettingsMessage::UpdatePinConfirm(value) => {
            security::update_pin_confirm(
                target.security,
                value,
                context.pin_policy,
            )
        }
        SettingsMessage::SubmitPinChange => {
            security::submit_pin_change(target.security, context.pin_policy)
        }
        SettingsMessage::SubmitPinRemoval => {
            security::submit_pin_removal(target.security)
        }
        SettingsMessage::PinChangeResult(result) => {
            security::pin_change_result(target.security, result)
        }
        SettingsMessage::PinRemovalResult(result) => {
            security::pin_removal_result(target.security, result)
        }
        SettingsMessage::CancelPinChange => {
            security::cancel_pin_change(target.security)
        }
        SettingsMessage::ToggleAutoLogin(enabled) => {
            preferences::toggle_auto_login(target.preferences, enabled)
        }
        SettingsMessage::AutoLoginToggled(result) => {
            preferences::auto_login_toggled(target.preferences, result)
        }
        SettingsMessage::UpdateDisplayName(name) => {
            profile::update_display_name(target.profile, name)
        }
        SettingsMessage::UpdateEmail(email) => {
            profile::update_email(target.profile, email)
        }
        SettingsMessage::SubmitProfileChanges => {
            profile::submit_profile_changes(target.profile)
        }
        SettingsMessage::ProfileChangeResult(result) => {
            profile::profile_change_result(target.profile, result)
        }
        SettingsMessage::LoadDevices => {
            device_management::load_devices(target.device_management)
        }
        SettingsMessage::DevicesLoaded(result) => {
            device_management::devices_loaded(target.device_management, result)
        }
        SettingsMessage::RefreshDevices => {
            device_management::refresh_devices(target.device_management)
        }
        SettingsMessage::RevokeDevice(device_id) => {
            device_management::revoke_device(
                target.device_management,
                device_id,
            )
        }
        SettingsMessage::DeviceRevoked(result) => {
            device_management::device_revoked(target.device_management, result)
        }
    }
}

fn reduce_playback(
    playback: &mut PlaybackState,
    message: PlaybackMessage,
) -> SettingsUpdate {
    match message {
        PlaybackMessage::SetAutoPlayNext(enabled) => {
            playback.auto_play_next = enabled
        }
        PlaybackMessage::SetResumeBehavior(behavior) => {
            playback.resume_behavior = behavior
        }
        PlaybackMessage::SetPreferredQuality(quality) => {
            playback.preferred_quality = quality
        }
        PlaybackMessage::SetSeekForwardCoarse(value) => {
            if let Some(seconds) = parse_f64_range(&value, 0.0, 120.0) {
                playback.seek_forward_coarse = seconds;
            }
        }
        PlaybackMessage::SetSeekBackwardCoarse(value) => {
            if let Some(seconds) = parse_f64_range(&value, 0.0, 120.0) {
                playback.seek_backward_coarse = seconds;
            }
        }
        PlaybackMessage::SetSeekForwardFine(value) => {
            if let Some(seconds) = parse_f64_range(&value, 0.0, 60.0) {
                playback.seek_forward_fine = seconds;
            }
        }
        PlaybackMessage::SetSeekBackwardFine(value) => {
            if let Some(seconds) = parse_f64_range(&value, 0.0, 60.0) {
                playback.seek_backward_fine = seconds;
            }
        }
        PlaybackMessage::SetSkipIntroDuration(seconds) => {
            playback.skip_intro_duration = seconds;
        }
        PlaybackMessage::SetSkipCreditsDuration(seconds) => {
            playback.skip_credits_duration = seconds;
        }
        PlaybackMessage::SetSubtitlesEnabled(enabled) => {
            playback.subtitles_enabled = enabled;
        }
        PlaybackMessage::SetSubtitleLanguage(language) => {
            playback.subtitle_language = language;
        }
        PlaybackMessage::SetSubtitleFontScale(scale) => {
            playback.subtitle_font_scale = scale.clamp(0.5, 2.0);
        }
    }

    SettingsUpdate::none()
}

fn reduce_display(
    display: &mut DisplayState,
    message: DisplayMessage,
) -> SettingsUpdate {
    let mut update = SettingsUpdate::none();
    match message {
        DisplayMessage::SetTheme(theme) => display.theme = theme,
        DisplayMessage::SetGridSize(size) => display.grid_size = size,
        DisplayMessage::SetPosterTitlesOnHover(enabled) => {
            display.poster_titles_on_hover = enabled;
        }
        DisplayMessage::SetShowRecentlyWatched(enabled) => {
            display.show_recently_watched = enabled;
        }
        DisplayMessage::SetShowContinueWatching(enabled) => {
            display.show_continue_watching = enabled;
        }
        DisplayMessage::SetSidebarCollapsed(collapsed) => {
            display.sidebar_collapsed = collapsed;
        }
        DisplayMessage::SetPosterBaseWidth(value) => {
            if let Some(width) = parse_f32_range(&value, 100.0, 500.0) {
                display.poster_base_width = width;
            }
        }
        DisplayMessage::SetPosterBaseHeight(value) => {
            if let Some(height) = parse_f32_range(&value, 150.0, 750.0) {
                display.poster_base_height = height;
            }
        }
        DisplayMessage::SetPosterCornerRadius(value) => {
            if let Some(radius) = parse_f32_range(&value, 0.0, 50.0) {
                display.poster_corner_radius = radius;
            }
        }
        DisplayMessage::SetPosterTextAreaHeight(height) => {
            display.poster_text_area_height = height.clamp(0.0, 200.0);
        }
        DisplayMessage::SetGridPosterGap(value) => {
            if let Some(spacing) = parse_f32_range(&value, 0.0, 100.0) {
                display.grid_poster_gap = spacing;
                update.push(SettingsEffect::RefreshGridLayout);
            }
        }
        DisplayMessage::SetGridRowSpacing(value) => {
            if let Some(spacing) = parse_f32_range(&value, 0.0, 200.0) {
                display.grid_row_spacing = spacing;
            }
        }
        DisplayMessage::SetGridViewportPadding(padding) => {
            display.grid_viewport_padding = padding.clamp(0.0, 200.0);
        }
        DisplayMessage::SetGridTopPadding(padding) => {
            display.grid_top_padding = padding.clamp(0.0, 200.0);
        }
        DisplayMessage::SetGridBottomPadding(padding) => {
            display.grid_bottom_padding = padding.clamp(0.0, 300.0);
        }
        DisplayMessage::SetAnimationHoverScale(value) => {
            if let Some(scale) = parse_f32_range(&value, 1.0, 1.5) {
                display.animation_hover_scale = scale;
            }
        }
        DisplayMessage::SetAnimationDefaultDuration(value) => {
            if let Ok(ms) = value.parse::<u64>() {
                if (100..=2000).contains(&ms) {
                    display.animation_default_duration_ms = ms;
                }
            }
        }
        DisplayMessage::SetAnimationTextureFadeInitial(ms) => {
            display.animation_texture_fade_initial_ms = ms.clamp(0, 5000);
        }
        DisplayMessage::SetAnimationTextureFade(ms) => {
            display.animation_texture_fade_ms = ms.clamp(0, 5000);
        }
        DisplayMessage::SetLibraryPosterQuality(quality) => {
            display.library_poster_quality = quality;
        }
        DisplayMessage::SetDetailPosterQuality(quality) => {
            display.detail_poster_quality = quality;
        }
        DisplayMessage::SetScrollbarScrollerMinLength(value) => {
            if let Some(px) = parse_f32_range(&value, 2.0, 120.0) {
                display.scrollbar_scroller_min_length_px = px;
            }
        }
    }
    update
}

fn reduce_theme(
    theme: &mut ThemeState,
    message: ThemeMessage,
) -> SettingsUpdate {
    match message {
        ThemeMessage::SetAccentHueSat { hue, saturation } => {
            theme.accent_color.primary_hue = hue.rem_euclid(360.0);
            theme.accent_color.primary_saturation =
                saturation.clamp(0.0, 100.0);
            SettingsUpdate::effect(SettingsEffect::ApplyAccentColor(
                theme.accent_color.primary_color(),
            ))
        }
        ThemeMessage::SetAccentLightness(lightness) => {
            theme.accent_color.lightness = lightness.clamp(0.0, 100.0);
            SettingsUpdate::effect(SettingsEffect::ApplyAccentColor(
                theme.accent_color.primary_color(),
            ))
        }
        ThemeMessage::SetHarmonyMode(mode) => {
            theme.accent_color.harmony_mode = mode;
            SettingsUpdate::none()
        }
        ThemeMessage::ResetToDefault => {
            theme.accent_color = Default::default();
            theme.picker_active = false;
            SettingsUpdate::effect(SettingsEffect::ApplyAccentColor(
                theme.accent_color.primary_color(),
            ))
        }
        ThemeMessage::PickerDragStarted => {
            theme.picker_active = true;
            SettingsUpdate::none()
        }
        ThemeMessage::PickerDragEnded => {
            theme.picker_active = false;
            SettingsUpdate::none()
        }
    }
}

fn reduce_performance(
    performance: &mut PerformanceState,
    message: PerformanceMessage,
) -> SettingsUpdate {
    match message {
        PerformanceMessage::SetScrollDebounceMs(value) => {
            if let Ok(ms) = value.parse::<u64>() {
                if (5..=200).contains(&ms) {
                    performance.scroll_debounce_ms = ms;
                }
            }
            SettingsUpdate::none()
        }
        PerformanceMessage::SetScrollTickNs(ns) => {
            performance.scroll_tick_ns = ns.clamp(1_000_000, 33_333_333);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetScrollDecayTauMs(value) => {
            if let Ok(ms) = value.parse::<u64>() {
                if (50..=1000).contains(&ms) {
                    performance.scroll_decay_tau_ms = ms;
                }
            }
            SettingsUpdate::none()
        }
        PerformanceMessage::SetScrollBaseVelocity(value) => {
            performance.scroll_base_velocity = value.clamp(0.1, 20.0);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetScrollMaxVelocity(value) => {
            if let Some(velocity) = parse_f32_range(&value, 1.0, 20.0) {
                performance.scroll_max_velocity = velocity;
            }
            SettingsUpdate::none()
        }
        PerformanceMessage::SetScrollMinStopVelocity(value) => {
            performance.scroll_min_stop_velocity = value.clamp(0.0, 2.0);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetScrollRampMs(ms) => {
            performance.scroll_ramp_ms = ms.clamp(0, 5000);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetScrollBoostMultiplier(multiplier) => {
            performance.scroll_boost_multiplier = multiplier.clamp(1.0, 10.0);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetScrollEasing(easing) => {
            performance.scroll_easing = easing;
            SettingsUpdate::none()
        }
        PerformanceMessage::SetTextureMaxUploadsPerFrame(count) => {
            performance.texture_max_uploads_per_frame = count.clamp(1, 128);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetPrefetchRowsAbove(count) => {
            performance.prefetch_rows_above = count.min(10);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetPrefetchRowsBelow(count) => {
            performance.prefetch_rows_below = count.min(10);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetPrefetchKeepAliveMs(ms) => {
            performance.prefetch_keep_alive_ms = ms.clamp(1_000, 300_000);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselPrefetchItems(count) => {
            performance.carousel_prefetch_items = count.min(128);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselBackgroundItems(count) => {
            performance.carousel_background_items = count.min(256);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselBaseVelocity(value) => {
            performance.carousel_base_velocity = value.clamp(0.1, 20.0);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselMaxVelocity(value) => {
            performance.carousel_max_velocity = value.clamp(0.1, 40.0);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselBoostMultiplier(value) => {
            performance.carousel_boost_multiplier = value.clamp(1.0, 10.0);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselRampMs(ms) => {
            performance.carousel_ramp_ms = ms.clamp(0, 5000);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselDecayTauMs(ms) => {
            performance.carousel_decay_tau_ms = ms.clamp(50, 5000);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselItemSnapMs(ms) => {
            performance.carousel_item_snap_ms = ms.clamp(0, 5000);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselPageSnapMs(ms) => {
            performance.carousel_page_snap_ms = ms.clamp(0, 5000);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselHoldTapThresholdMs(ms) => {
            performance.carousel_hold_tap_threshold_ms = ms.clamp(0, 2000);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselSnapEpsilon(value) => {
            performance.carousel_snap_epsilon = value.clamp(0.0, 1.0);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetCarouselAnchorSettleMs(ms) => {
            performance.carousel_anchor_settle_ms = ms.clamp(0, 1000);
            SettingsUpdate::none()
        }
        PerformanceMessage::SetAnimationHoverScale(scale) => {
            let clamped = scale.clamp(1.0, 1.2);
            performance.animation_hover_scale = clamped;
            SettingsUpdate::effect(SettingsEffect::ApplyHoverAnimation {
                scale: Some(clamped),
                transition_ms: None,
                scale_down_delay_ms: None,
            })
        }
        PerformanceMessage::SetAnimationHoverTransitionMs(ms) => {
            let clamped = ms.clamp(50, 500);
            performance.animation_hover_transition_ms = clamped;
            SettingsUpdate::effect(SettingsEffect::ApplyHoverAnimation {
                scale: None,
                transition_ms: Some(clamped),
                scale_down_delay_ms: None,
            })
        }
        PerformanceMessage::SetAnimationHoverScaleDownDelayMs(ms) => {
            let clamped = ms.clamp(0, 500);
            performance.animation_hover_scale_down_delay_ms = clamped;
            SettingsUpdate::effect(SettingsEffect::ApplyHoverAnimation {
                scale: None,
                transition_ms: None,
                scale_down_delay_ms: Some(clamped),
            })
        }
    }
}

fn parse_f32_range(value: &str, min: f32, max: f32) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|parsed| parsed.is_finite() && *parsed >= min && *parsed <= max)
}

fn parse_f64_range(value: &str, min_exclusive: f64, max: f64) -> Option<f64> {
    value.parse::<f64>().ok().filter(|parsed| {
        parsed.is_finite() && *parsed > min_exclusive && *parsed <= max
    })
}
