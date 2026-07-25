//! Conservative capability gate for the macOS native-root presenter.
//!
//! mpv's modern macOS backend owns its `NSWindow` and video layer. Ferrex may
//! place a transparent Iced child window above that native root only after the
//! relationship has been proven across AppKit lifetime, fullscreen, Spaces,
//! scale, and teardown transitions. Until then this module deterministically
//! selects mpv's ordinary native window and never advertises `wid` embedding.

use std::{fmt, num::NonZeroUsize};

use crate::{
    contract::{
        FallbackReason, FallbackReasonCode, PlaybackError, PlaybackErrorKind,
        PlaybackTarget,
    },
    presenter::{
        FullscreenOwner, NativePresenter, PresenterCapabilities,
        PresenterIdentity, SurfaceGeometry,
    },
};

/// Build-time switch used to compile the developer AppKit presenter path.
pub const MACOS_PRESENTER_BUILD_ENV: &str = "FERREX_MPV_MACOS_PRESENTER";

/// Release-safe build mode for the native-root presenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MacOsPresenterBuildMode {
    /// Integrated presentation is unavailable and selection falls back.
    #[default]
    Disabled,
    /// Explicit developer/hardware-validation path; never selected by Auto.
    Spike,
}

impl MacOsPresenterBuildMode {
    /// Parse the build environment. Unknown values fail closed.
    pub fn parse(value: Option<&str>) -> Result<Self, MacOsPresenterError> {
        match value.map(str::trim) {
            None | Some("") | Some("disabled") => Ok(Self::Disabled),
            Some("spike") => Ok(Self::Spike),
            Some(value) => {
                Err(MacOsPresenterError::InvalidBuildMode(value.to_owned()))
            }
        }
    }

    /// Mode compiled into a macOS target.
    #[cfg(target_os = "macos")]
    pub fn compiled() -> Self {
        Self::parse(option_env!("FERREX_MPV_MACOS_PRESENTER"))
            .unwrap_or(Self::Disabled)
    }
}

/// Native relationship under evaluation for macOS integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacOsPresenterStrategy {
    /// mpv owns the root `NSWindow`; a transparent Iced child follows it.
    NativeRootChildWindow,
}

/// Stable, non-sensitive reason an integrated presenter is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacOsPresenterBlocker {
    AppKitMainThreadUnavailable,
    MpvWindowUnavailable,
    MpvWindowLifetimeUnverified,
    ChildWindowRelationshipUnverified,
    ContentLayoutUnverified,
    BackingScaleUnverified,
    FocusOcclusionUnverified,
    FullscreenUnverified,
    SpacesUnverified,
    HideUnhideUnverified,
    TeardownUnverified,
}

impl MacOsPresenterBlocker {
    /// Machine-stable label suitable for diagnostics and fallback logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AppKitMainThreadUnavailable => "appkit_main_thread",
            Self::MpvWindowUnavailable => "mpv_window",
            Self::MpvWindowLifetimeUnverified => "mpv_window_lifetime",
            Self::ChildWindowRelationshipUnverified => {
                "child_window_relationship"
            }
            Self::ContentLayoutUnverified => "content_layout",
            Self::BackingScaleUnverified => "backing_scale",
            Self::FocusOcclusionUnverified => "focus_occlusion",
            Self::FullscreenUnverified => "native_fullscreen",
            Self::SpacesUnverified => "spaces",
            Self::HideUnhideUnverified => "app_hide_unhide",
            Self::TeardownUnverified => "detach_before_teardown",
        }
    }
}

/// Evidence collected by the macOS AppKit spike/integration harness.
///
/// This deliberately stores no raw `NSWindow`, `NSView`, or `window-id` value;
/// diagnostics can report availability without leaking or retaining pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MacOsPresenterEvidence {
    pub appkit_main_thread: bool,
    pub mpv_window_available: bool,
    pub mpv_window_lifetime_verified: bool,
    pub child_window_relationship_verified: bool,
    pub content_layout_verified: bool,
    pub backing_scale_verified: bool,
    pub focus_occlusion_verified: bool,
    pub native_fullscreen_verified: bool,
    pub spaces_verified: bool,
    pub app_hide_unhide_verified: bool,
    pub detach_before_teardown_verified: bool,
    pub native_hdr_verified: bool,
}

impl MacOsPresenterEvidence {
    /// Evidence fixture representing a completed non-HDR integration spike.
    /// Production code must populate equivalent facts from the platform gate;
    /// this constructor does not itself enable any backend selector.
    #[cfg(test)]
    const fn verified() -> Self {
        Self {
            appkit_main_thread: true,
            mpv_window_available: true,
            mpv_window_lifetime_verified: true,
            child_window_relationship_verified: true,
            content_layout_verified: true,
            backing_scale_verified: true,
            focus_occlusion_verified: true,
            native_fullscreen_verified: true,
            spaces_verified: true,
            app_hide_unhide_verified: true,
            detach_before_teardown_verified: true,
            native_hdr_verified: false,
        }
    }

    fn blockers(self) -> Vec<MacOsPresenterBlocker> {
        let checks = [
            (
                self.appkit_main_thread,
                MacOsPresenterBlocker::AppKitMainThreadUnavailable,
            ),
            (
                self.mpv_window_available,
                MacOsPresenterBlocker::MpvWindowUnavailable,
            ),
            (
                self.mpv_window_lifetime_verified,
                MacOsPresenterBlocker::MpvWindowLifetimeUnverified,
            ),
            (
                self.child_window_relationship_verified,
                MacOsPresenterBlocker::ChildWindowRelationshipUnverified,
            ),
            (
                self.content_layout_verified,
                MacOsPresenterBlocker::ContentLayoutUnverified,
            ),
            (
                self.backing_scale_verified,
                MacOsPresenterBlocker::BackingScaleUnverified,
            ),
            (
                self.focus_occlusion_verified,
                MacOsPresenterBlocker::FocusOcclusionUnverified,
            ),
            (
                self.native_fullscreen_verified,
                MacOsPresenterBlocker::FullscreenUnverified,
            ),
            (
                self.spaces_verified,
                MacOsPresenterBlocker::SpacesUnverified,
            ),
            (
                self.app_hide_unhide_verified,
                MacOsPresenterBlocker::HideUnhideUnverified,
            ),
            (
                self.detach_before_teardown_verified,
                MacOsPresenterBlocker::TeardownUnverified,
            ),
        ];
        checks
            .into_iter()
            .filter_map(|(verified, blocker)| (!verified).then_some(blocker))
            .collect()
    }
}

/// Capability and fallback result for one macOS platform probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacOsPresenterDecision {
    pub strategy: MacOsPresenterStrategy,
    pub capabilities: PresenterCapabilities,
    pub blockers: Vec<MacOsPresenterBlocker>,
    pub fallback: Option<FallbackReason>,
}

impl MacOsPresenterDecision {
    /// Evaluate integrated presentation without depending on `wid`.
    pub fn evaluate(evidence: MacOsPresenterEvidence) -> Self {
        let blockers = evidence.blockers();
        let integrated = blockers.is_empty();
        let capabilities = PresenterCapabilities {
            integrated_overlay: integrated,
            embedded_surface: false,
            native_hdr: integrated && evidence.native_hdr_verified,
            fractional_scaling: integrated && evidence.backing_scale_verified,
            native_window_fallback: true,
            fullscreen_owner: integrated
                .then_some(FullscreenOwner::VideoOutput),
            compositor_requirement: None,
        };
        let fallback = (!integrated).then(|| FallbackReason {
            code: FallbackReasonCode::MissingCapability,
            from: Some(PlaybackTarget::MPV_INTEGRATED),
            to: PlaybackTarget::MPV_NATIVE_WINDOW,
            detail: format!(
                "macOS native-root presenter unavailable: {}",
                blockers
                    .iter()
                    .map(|blocker| blocker.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        });

        Self {
            strategy: MacOsPresenterStrategy::NativeRootChildWindow,
            capabilities,
            blockers,
            fallback,
        }
    }

    /// Modern mpv's macOS native-root strategy never requires `wid`.
    pub const fn requires_wid(&self) -> bool {
        false
    }
}

/// Capabilities of the developer AppKit presenter path.
///
/// The spike is intentionally explicit-only until the manual display, Spaces,
/// fullscreen, HDR, and teardown matrix has been recorded. In particular,
/// this function does not claim native HDR support.
pub fn macos_presenter_capabilities(
    build_mode: MacOsPresenterBuildMode,
) -> PresenterCapabilities {
    PresenterCapabilities {
        integrated_overlay: matches!(
            build_mode,
            MacOsPresenterBuildMode::Spike
        ),
        embedded_surface: false,
        native_hdr: false,
        fractional_scaling: true,
        native_window_fallback: true,
        fullscreen_owner: Some(FullscreenOwner::VideoOutput),
        compositor_requirement: Some(
            "macOS AppKit child-window composition".to_owned(),
        ),
    }
}

/// Opaque, non-null AppKit `NSWindow` identity.
///
/// For mpv v0.41's macOS VO, `VOCTRL_GET_WINDOW_ID` returns the `NSWindow`
/// pointer bit-cast to `i64`. This is an output observation; it is not mpv's
/// unsupported macOS `wid` input option.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacOsWindow(NonZeroUsize);

impl MacOsWindow {
    /// Convert mpv's read-only `window-id` property to an opaque window.
    pub fn from_mpv_window_id(value: i64) -> Result<Self, MacOsPresenterError> {
        let pointer_bits = value as u64;
        let raw = usize::try_from(pointer_bits)
            .ok()
            .and_then(NonZeroUsize::new)
            .ok_or(MacOsPresenterError::InvalidMpvWindowId)?;
        Ok(Self(raw))
    }

    /// Wrap a non-null pointer obtained from an AppKit object lease.
    pub const fn from_non_zero(value: NonZeroUsize) -> Self {
        Self(value)
    }

    pub const fn get(self) -> usize {
        self.0.get()
    }
}

impl fmt::Debug for MacOsWindow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MacOsWindow(<redacted>)")
    }
}

/// Iced overlay window borrowed for one AppKit attach operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacOsPresenterHost {
    pub overlay: MacOsWindow,
}

#[cfg(all(target_os = "macos", feature = "ui"))]
impl MacOsPresenterHost {
    /// Resolve Iced's AppKit `NSView` lease to its owning `NSWindow`.
    pub fn from_captured_iced_host(
        host: &crate::native_video_slot::CapturedIcedHost,
    ) -> Result<Self, PlaybackError> {
        use iced::window::raw_window_handle::{
            HasWindowHandle, RawWindowHandle,
        };
        use objc2::rc::Retained;
        use objc2_app_kit::NSView;

        let raw = host.window_handle().map_err(|error| {
            presenter_error(format!(
                "could not borrow Iced AppKit overlay handle: {error}"
            ))
        })?;
        let RawWindowHandle::AppKit(handle) = raw.as_raw() else {
            return Err(presenter_error(
                "captured Iced host is not an AppKit window",
            ));
        };

        // SAFETY: raw-window-handle guarantees that `ns_view` names a live
        // NSView for the duration of the host lease. Retaining it keeps the
        // object alive while AppKit resolves the owning top-level window.
        let view = unsafe {
            Retained::<NSView>::retain(handle.ns_view.as_ptr().cast())
        }
        .ok_or_else(|| {
            presenter_error("Iced AppKit NSView is no longer live")
        })?;
        let window = view.window().ok_or_else(|| {
            presenter_error("Iced AppKit NSView is not installed in a window")
        })?;
        let raw = NonZeroUsize::new(Retained::as_ptr(&window) as usize)
            .ok_or_else(|| presenter_error("Iced AppKit NSWindow is null"))?;
        Ok(Self {
            overlay: MacOsWindow::from_non_zero(raw),
        })
    }
}

/// Logical screen rectangle used to align the transparent overlay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacOsScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl MacOsScreenRect {
    fn validate(self) -> Result<Self, MacOsPresenterError> {
        if [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(f64::is_finite)
            && self.width >= 0.0
            && self.height >= 0.0
        {
            Ok(self)
        } else {
            Err(MacOsPresenterError::Operation(
                "AppKit returned invalid content geometry".to_owned(),
            ))
        }
    }
}

/// AppKit observations retained by the spike for scale/occlusion diagnostics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacOsWindowSnapshot {
    pub content_rect: MacOsScreenRect,
    pub backing_scale_factor: f64,
    pub visible_on_active_space: bool,
    pub occluded: bool,
    pub miniaturized: bool,
}

/// AppKit operations isolated behind a display-free fakeable interface.
pub trait MacOsWindowSystem {
    /// Retain a trusted AppKit window identity for subsequent operations.
    fn retain_window(
        &mut self,
        window: MacOsWindow,
    ) -> Result<(), MacOsPresenterError>;
    fn release_window(&mut self, window: MacOsWindow);
    fn is_window(&self, window: MacOsWindow) -> bool;
    fn parent_window(&self, window: MacOsWindow) -> Option<MacOsWindow>;
    fn collection_behavior(
        &self,
        window: MacOsWindow,
    ) -> Result<u64, MacOsPresenterError>;
    fn set_collection_behavior(
        &mut self,
        window: MacOsWindow,
        behavior: u64,
    ) -> Result<(), MacOsPresenterError>;
    fn ignores_mouse_events(
        &self,
        window: MacOsWindow,
    ) -> Result<bool, MacOsPresenterError>;
    fn set_ignores_mouse_events(
        &mut self,
        window: MacOsWindow,
        ignores: bool,
    ) -> Result<(), MacOsPresenterError>;
    fn add_child_above(
        &mut self,
        root: MacOsWindow,
        child: MacOsWindow,
    ) -> Result<(), MacOsPresenterError>;
    fn remove_child(&mut self, root: MacOsWindow, child: MacOsWindow);
    fn snapshot(
        &self,
        root: MacOsWindow,
    ) -> Result<MacOsWindowSnapshot, MacOsPresenterError>;
    fn position_overlay(
        &mut self,
        overlay: MacOsWindow,
        rect: MacOsScreenRect,
    ) -> Result<(), MacOsPresenterError>;
    fn set_visible_without_activation(
        &mut self,
        overlay: MacOsWindow,
        visible: bool,
    ) -> Result<(), MacOsPresenterError>;
    fn activate(
        &mut self,
        window: MacOsWindow,
    ) -> Result<(), MacOsPresenterError>;
}

const COLLECTION_TRANSIENT: u64 = 1 << 3;
const COLLECTION_FULLSCREEN_AUXILIARY: u64 = 1 << 8;

#[derive(Debug, Clone, Copy)]
struct MacOsAttachment {
    identity: PresenterIdentity,
    overlay: MacOsWindow,
    original_collection_behavior: u64,
    original_ignores_mouse_events: bool,
}

/// UI-thread-local AppKit native-root/child-overlay presenter.
///
/// mpv retains fullscreen ownership. The callback serializes a fullscreen
/// request through mpv; the presenter never independently toggles the root.
pub struct MacOsPresenter<W, F> {
    windows: W,
    fullscreen: F,
    video_root: MacOsWindow,
    build_mode: MacOsPresenterBuildMode,
    capabilities: PresenterCapabilities,
    attachment: Option<MacOsAttachment>,
    requested_visible: bool,
    suspended: bool,
    geometry_visible: bool,
    applied_visible: Option<bool>,
    last_snapshot: Option<MacOsWindowSnapshot>,
}

impl<W, F> fmt::Debug for MacOsPresenter<W, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacOsPresenter")
            .field("video_root", &self.video_root)
            .field("build_mode", &self.build_mode)
            .field("capabilities", &self.capabilities)
            .field("attachment", &self.attachment)
            .field("requested_visible", &self.requested_visible)
            .field("suspended", &self.suspended)
            .field("geometry_visible", &self.geometry_visible)
            .field("applied_visible", &self.applied_visible)
            .field("last_snapshot", &self.last_snapshot)
            .finish_non_exhaustive()
    }
}

impl<W, F> MacOsPresenter<W, F>
where
    W: MacOsWindowSystem,
    F: FnMut(bool) -> Result<(), PlaybackError>,
{
    pub fn new(
        windows: W,
        fullscreen: F,
        video_root: MacOsWindow,
        build_mode: MacOsPresenterBuildMode,
    ) -> Self {
        Self {
            windows,
            fullscreen,
            video_root,
            build_mode,
            capabilities: macos_presenter_capabilities(build_mode),
            attachment: None,
            requested_visible: false,
            suspended: false,
            geometry_visible: false,
            applied_visible: None,
            last_snapshot: None,
        }
    }

    pub const fn relationship(&self) -> MacOsPresenterStrategy {
        MacOsPresenterStrategy::NativeRootChildWindow
    }

    pub const fn last_snapshot(&self) -> Option<MacOsWindowSnapshot> {
        self.last_snapshot
    }

    /// Explicit focus handoff for keyboard navigation/input testing.
    pub fn focus_overlay(&mut self) -> Result<(), PlaybackError> {
        let attachment = self.attachment.ok_or_else(|| {
            presenter_error("macOS presenter is not attached")
        })?;
        self.windows
            .activate(attachment.overlay)
            .map_err(Into::into)
    }

    fn ensure_identity(
        &self,
        identity: PresenterIdentity,
    ) -> Result<MacOsAttachment, PlaybackError> {
        match self.attachment {
            Some(attachment) if attachment.identity == identity => {
                Ok(attachment)
            }
            Some(_) => Err(presenter_error(
                "macOS presenter rejected a stale attachment generation",
            )),
            None => Err(presenter_error("macOS presenter is not attached")),
        }
    }

    fn refresh_position_and_visibility(
        &mut self,
        attachment: MacOsAttachment,
    ) -> Result<(), PlaybackError> {
        if !self.windows.is_window(self.video_root)
            || !self.windows.is_window(attachment.overlay)
        {
            return Err(presenter_error(
                "macOS presenter window was destroyed before synchronization",
            ));
        }
        let snapshot = self.windows.snapshot(self.video_root)?;
        let rect = snapshot.content_rect.validate()?;
        if !snapshot.backing_scale_factor.is_finite()
            || snapshot.backing_scale_factor <= 0.0
        {
            return Err(presenter_error(
                "AppKit returned an invalid backing scale factor",
            ));
        }
        if self.last_snapshot.map(|snapshot| snapshot.content_rect)
            != Some(rect)
        {
            self.windows.position_overlay(attachment.overlay, rect)?;
        }
        let visible = self.requested_visible
            && !self.suspended
            && self.geometry_visible
            && snapshot.visible_on_active_space
            && !snapshot.miniaturized;
        // Do not hide solely from the root's occlusion bit. AppKit may count
        // this presenter's own transparent child as occluding the mpv root,
        // which would otherwise create a show/hide feedback loop. Parent/child
        // ordering already follows other-app occlusion.
        if self.applied_visible != Some(visible) {
            self.windows
                .set_visible_without_activation(attachment.overlay, visible)?;
            self.applied_visible = Some(visible);
        }
        self.last_snapshot = Some(snapshot);
        Ok(())
    }

    fn restore_attachment(&mut self, attachment: MacOsAttachment) {
        if self.windows.is_window(attachment.overlay) {
            let _ = self
                .windows
                .set_visible_without_activation(attachment.overlay, false);
            if self.windows.parent_window(attachment.overlay)
                == Some(self.video_root)
            {
                self.windows
                    .remove_child(self.video_root, attachment.overlay);
            }
            let _ = self.windows.set_collection_behavior(
                attachment.overlay,
                attachment.original_collection_behavior,
            );
            let _ = self.windows.set_ignores_mouse_events(
                attachment.overlay,
                attachment.original_ignores_mouse_events,
            );
        }
        self.windows.release_window(attachment.overlay);
        self.applied_visible = None;
    }
}

impl<W, F> NativePresenter for MacOsPresenter<W, F>
where
    W: MacOsWindowSystem + 'static,
    F: FnMut(bool) -> Result<(), PlaybackError> + 'static,
{
    type Host<'host>
        = MacOsPresenterHost
    where
        Self: 'host;

    fn attach(
        &mut self,
        identity: PresenterIdentity,
        host: Self::Host<'_>,
    ) -> Result<(), PlaybackError> {
        if !matches!(self.build_mode, MacOsPresenterBuildMode::Spike) {
            return Err(presenter_error(
                "macOS integrated presenter is disabled in this build",
            ));
        }
        if self.attachment.is_some() {
            return Err(presenter_error(
                "macOS presenter attach was requested more than once",
            ));
        }
        if host.overlay == self.video_root {
            return Err(presenter_error(
                "macOS presenter received identical root and overlay windows",
            ));
        }
        self.windows.retain_window(host.overlay)?;
        if !self.windows.is_window(host.overlay)
            || !self.windows.is_window(self.video_root)
        {
            self.windows.release_window(host.overlay);
            return Err(presenter_error(
                "macOS presenter received a stale AppKit window",
            ));
        }
        if self.windows.parent_window(host.overlay).is_some() {
            self.windows.release_window(host.overlay);
            return Err(presenter_error(
                "Iced AppKit overlay already has a parent window",
            ));
        }

        let original_collection_behavior =
            match self.windows.collection_behavior(host.overlay) {
                Ok(behavior) => behavior,
                Err(error) => {
                    self.windows.release_window(host.overlay);
                    return Err(error.into());
                }
            };
        let original_ignores_mouse_events =
            match self.windows.ignores_mouse_events(host.overlay) {
                Ok(ignores) => ignores,
                Err(error) => {
                    self.windows.release_window(host.overlay);
                    return Err(error.into());
                }
            };
        let attachment = MacOsAttachment {
            identity,
            overlay: host.overlay,
            original_collection_behavior,
            original_ignores_mouse_events,
        };

        let behavior = original_collection_behavior
            | COLLECTION_TRANSIENT
            | COLLECTION_FULLSCREEN_AUXILIARY;
        let setup = (|| {
            self.windows
                .set_visible_without_activation(host.overlay, false)?;
            self.applied_visible = Some(false);
            self.windows.set_ignores_mouse_events(host.overlay, false)?;
            self.windows
                .set_collection_behavior(host.overlay, behavior)?;
            self.windows.add_child_above(self.video_root, host.overlay)
        })();
        if let Err(error) = setup {
            self.restore_attachment(attachment);
            return Err(error.into());
        }

        self.attachment = Some(attachment);
        if let Err(error) = self.refresh_position_and_visibility(attachment) {
            self.attachment = None;
            self.restore_attachment(attachment);
            return Err(error);
        }
        Ok(())
    }

    fn synchronize(
        &mut self,
        identity: PresenterIdentity,
        geometry: SurfaceGeometry,
    ) -> Result<(), PlaybackError> {
        geometry.validate().map_err(|error| {
            presenter_error(format!(
                "macOS presenter rejected host geometry: {error}"
            ))
        })?;
        let attachment = self.ensure_identity(identity)?;
        self.geometry_visible = geometry.is_visible();
        self.refresh_position_and_visibility(attachment)
    }

    fn set_visible(
        &mut self,
        identity: PresenterIdentity,
        visible: bool,
    ) -> Result<(), PlaybackError> {
        let attachment = self.ensure_identity(identity)?;
        self.requested_visible = visible;
        self.refresh_position_and_visibility(attachment)
    }

    fn set_suspended(
        &mut self,
        identity: PresenterIdentity,
        suspended: bool,
    ) -> Result<(), PlaybackError> {
        let attachment = self.ensure_identity(identity)?;
        self.suspended = suspended;
        self.refresh_position_and_visibility(attachment)
    }

    fn set_fullscreen(
        &mut self,
        identity: PresenterIdentity,
        owner: FullscreenOwner,
        fullscreen: bool,
    ) -> Result<(), PlaybackError> {
        let _ = self.ensure_identity(identity)?;
        if owner != FullscreenOwner::VideoOutput {
            return Err(presenter_error(
                "macOS native-root mode requires mpv to own fullscreen",
            ));
        }
        (self.fullscreen)(fullscreen)
    }

    fn detach(&mut self, identity: PresenterIdentity) {
        let Some(attachment) = self.attachment else {
            return;
        };
        if attachment.identity != identity {
            return;
        }
        self.attachment = None;
        self.requested_visible = false;
        self.geometry_visible = false;
        self.applied_visible = None;
        self.last_snapshot = None;
        self.restore_attachment(attachment);
    }

    fn capabilities(&self) -> &PresenterCapabilities {
        &self.capabilities
    }
}

/// Target-native AppKit implementation. The retained objects and
/// `MainThreadMarker` make this value event-loop-local and non-`Send`.
#[cfg(target_os = "macos")]
pub struct AppKitWindowSystem {
    _main_thread: objc2::MainThreadMarker,
    windows: std::collections::HashMap<
        MacOsWindow,
        objc2::rc::Retained<objc2_app_kit::NSWindow>,
    >,
}

#[cfg(target_os = "macos")]
impl fmt::Debug for AppKitWindowSystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppKitWindowSystem")
            .field("retained_window_count", &self.windows.len())
            .finish()
    }
}

#[cfg(target_os = "macos")]
impl AppKitWindowSystem {
    /// Whether the caller is the AppKit main thread.
    pub fn main_thread_available() -> bool {
        objc2::MainThreadMarker::new().is_some()
    }

    /// Retain mpv's live `NSWindow` on the AppKit main thread.
    ///
    /// Call this only after `vo-configured=true` and a non-zero macOS
    /// `window-id` observation. mpv owns the source pointer contract.
    pub fn new(video_root: MacOsWindow) -> Result<Self, MacOsPresenterError> {
        let main_thread = objc2::MainThreadMarker::new()
            .ok_or(MacOsPresenterError::AppKitMainThreadRequired)?;
        let root = Self::retain_native(video_root)?;
        Ok(Self {
            _main_thread: main_thread,
            windows: std::collections::HashMap::from([(video_root, root)]),
        })
    }

    pub fn is_live(&self, window: MacOsWindow) -> bool {
        self.windows.contains_key(&window)
    }

    fn retain_native(
        window: MacOsWindow,
    ) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>, MacOsPresenterError>
    {
        // SAFETY: callers obtain identities only from mpv's macOS
        // VOCTRL_GET_WINDOW_ID or NSView.window. Both are NSWindow pointers,
        // and construction is restricted to the AppKit main thread.
        unsafe {
            objc2::rc::Retained::retain(
                window.get() as *mut objc2_app_kit::NSWindow
            )
        }
        .ok_or_else(|| {
            MacOsPresenterError::Operation(
                "could not retain AppKit NSWindow".to_owned(),
            )
        })
    }

    fn window(
        &self,
        window: MacOsWindow,
    ) -> Result<&objc2_app_kit::NSWindow, MacOsPresenterError> {
        self.windows.get(&window).map(AsRef::as_ref).ok_or_else(|| {
            MacOsPresenterError::Operation(
                "AppKit window lease is unavailable".to_owned(),
            )
        })
    }

    fn identity(window: &objc2_app_kit::NSWindow) -> MacOsWindow {
        let raw = NonZeroUsize::new(window as *const _ as usize)
            .expect("Objective-C object references are non-null");
        MacOsWindow::from_non_zero(raw)
    }
}

#[cfg(target_os = "macos")]
impl MacOsWindowSystem for AppKitWindowSystem {
    fn retain_window(
        &mut self,
        window: MacOsWindow,
    ) -> Result<(), MacOsPresenterError> {
        if !self.windows.contains_key(&window) {
            self.windows.insert(window, Self::retain_native(window)?);
        }
        Ok(())
    }

    fn release_window(&mut self, window: MacOsWindow) {
        self.windows.remove(&window);
    }

    fn is_window(&self, window: MacOsWindow) -> bool {
        self.is_live(window)
    }

    fn parent_window(&self, window: MacOsWindow) -> Option<MacOsWindow> {
        self.window(window)
            .ok()
            .and_then(objc2_app_kit::NSWindow::parentWindow)
            .as_deref()
            .map(Self::identity)
    }

    fn collection_behavior(
        &self,
        window: MacOsWindow,
    ) -> Result<u64, MacOsPresenterError> {
        Ok(self.window(window)?.collectionBehavior().0 as u64)
    }

    fn set_collection_behavior(
        &mut self,
        window: MacOsWindow,
        behavior: u64,
    ) -> Result<(), MacOsPresenterError> {
        let behavior = usize::try_from(behavior).map_err(|_| {
            MacOsPresenterError::Operation(
                "AppKit collection behavior is out of range".to_owned(),
            )
        })?;
        self.window(window)?.setCollectionBehavior(
            objc2_app_kit::NSWindowCollectionBehavior::from_bits_retain(
                behavior,
            ),
        );
        Ok(())
    }

    fn ignores_mouse_events(
        &self,
        window: MacOsWindow,
    ) -> Result<bool, MacOsPresenterError> {
        Ok(self.window(window)?.ignoresMouseEvents())
    }

    fn set_ignores_mouse_events(
        &mut self,
        window: MacOsWindow,
        ignores: bool,
    ) -> Result<(), MacOsPresenterError> {
        self.window(window)?.setIgnoresMouseEvents(ignores);
        Ok(())
    }

    fn add_child_above(
        &mut self,
        root: MacOsWindow,
        child: MacOsWindow,
    ) -> Result<(), MacOsPresenterError> {
        // SAFETY: both retained objects are NSWindows, have no existing child
        // relationship, and this is executed on the AppKit main thread.
        unsafe {
            self.window(root)?.addChildWindow_ordered(
                self.window(child)?,
                objc2_app_kit::NSWindowOrderingMode::Above,
            );
        }
        Ok(())
    }

    fn remove_child(&mut self, root: MacOsWindow, child: MacOsWindow) {
        if let (Ok(root), Ok(child)) = (self.window(root), self.window(child)) {
            root.removeChildWindow(child);
        }
    }

    fn snapshot(
        &self,
        root: MacOsWindow,
    ) -> Result<MacOsWindowSnapshot, MacOsPresenterError> {
        let root = self.window(root)?;
        let content = root.contentView().ok_or_else(|| {
            MacOsPresenterError::Operation(
                "mpv AppKit window has no content view".to_owned(),
            )
        })?;
        let window_rect = content.convertRect_toView(content.bounds(), None);
        let screen_rect = root.convertRectToScreen(window_rect);
        let occluded = !root
            .occlusionState()
            .contains(objc2_app_kit::NSWindowOcclusionState::Visible);
        Ok(MacOsWindowSnapshot {
            content_rect: MacOsScreenRect {
                x: screen_rect.origin.x,
                y: screen_rect.origin.y,
                width: screen_rect.size.width,
                height: screen_rect.size.height,
            },
            backing_scale_factor: root.backingScaleFactor(),
            visible_on_active_space: root.isVisible() && root.isOnActiveSpace(),
            occluded,
            miniaturized: root.isMiniaturized(),
        })
    }

    fn position_overlay(
        &mut self,
        overlay: MacOsWindow,
        rect: MacOsScreenRect,
    ) -> Result<(), MacOsPresenterError> {
        let rect = rect.validate()?;
        self.window(overlay)?.setFrame_display(
            objc2_foundation::NSRect::new(
                objc2_foundation::NSPoint::new(rect.x, rect.y),
                objc2_foundation::NSSize::new(rect.width, rect.height),
            ),
            true,
        );
        Ok(())
    }

    fn set_visible_without_activation(
        &mut self,
        overlay: MacOsWindow,
        visible: bool,
    ) -> Result<(), MacOsPresenterError> {
        let overlay = self.window(overlay)?;
        if visible {
            overlay.orderFront(None);
        } else {
            overlay.orderOut(None);
        }
        Ok(())
    }

    fn activate(
        &mut self,
        window: MacOsWindow,
    ) -> Result<(), MacOsPresenterError> {
        self.window(window)?.makeKeyWindow();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MacOsPresenterError {
    #[error("invalid {MACOS_PRESENTER_BUILD_ENV} value: {0}")]
    InvalidBuildMode(String),
    #[error("mpv returned an invalid macOS window-id")]
    InvalidMpvWindowId,
    #[error("the macOS presenter must run on the AppKit main thread")]
    AppKitMainThreadRequired,
    #[error("macOS presenter operation failed: {0}")]
    Operation(String),
}

impl From<MacOsPresenterError> for PlaybackError {
    fn from(error: MacOsPresenterError) -> Self {
        presenter_error(error.to_string())
    }
}

fn presenter_error(message: impl Into<String>) -> PlaybackError {
    let mut error = PlaybackError::new(PlaybackErrorKind::Presenter, message);
    error.backend = Some(crate::contract::BackendKind::Mpv);
    error.recoverable = true;
    error
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    use crate::contract::{
        BackendCandidate, BackendRequest, FallbackPolicy, GeometryRevision,
        LogicalRect, PlaybackRequirements, SessionGeneration, select_backend,
    };
    use crate::presenter::{NativePresenter, PresenterGeneration};

    use super::*;

    fn window(value: usize) -> MacOsWindow {
        MacOsWindow::from_non_zero(NonZeroUsize::new(value).unwrap())
    }

    fn identity(value: u64) -> PresenterIdentity {
        PresenterIdentity::new(
            SessionGeneration::new(value),
            PresenterGeneration::new(value),
        )
    }

    fn geometry(visible: bool) -> SurfaceGeometry {
        SurfaceGeometry::new(
            GeometryRevision::INITIAL,
            LogicalRect::new(0.0, 0.0, 1280.0, 720.0),
            visible.then(|| LogicalRect::new(0.0, 0.0, 1280.0, 720.0)),
            2.0,
        )
    }

    #[derive(Debug, Clone)]
    struct WindowState {
        live: bool,
        parent: Option<MacOsWindow>,
        behavior: u64,
        ignores_mouse: bool,
        visible: bool,
        frame: Option<MacOsScreenRect>,
    }

    #[derive(Debug, Clone)]
    struct FakeAppKit {
        state: Rc<RefCell<HashMap<MacOsWindow, WindowState>>>,
        snapshot: Rc<RefCell<MacOsWindowSnapshot>>,
        operations: Rc<RefCell<Vec<String>>>,
    }

    impl FakeAppKit {
        fn new(video: MacOsWindow, overlay: MacOsWindow) -> Self {
            Self {
                state: Rc::new(RefCell::new(HashMap::from([
                    (
                        video,
                        WindowState {
                            live: true,
                            parent: None,
                            behavior: 0,
                            ignores_mouse: false,
                            visible: true,
                            frame: None,
                        },
                    ),
                    (
                        overlay,
                        WindowState {
                            live: true,
                            parent: None,
                            behavior: 0x20,
                            ignores_mouse: true,
                            visible: false,
                            frame: None,
                        },
                    ),
                ]))),
                snapshot: Rc::new(RefCell::new(MacOsWindowSnapshot {
                    content_rect: MacOsScreenRect {
                        x: 40.0,
                        y: 80.0,
                        width: 1280.0,
                        height: 720.0,
                    },
                    backing_scale_factor: 2.0,
                    visible_on_active_space: true,
                    occluded: false,
                    miniaturized: false,
                })),
                operations: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn operation(&self, value: impl Into<String>) {
            self.operations.borrow_mut().push(value.into());
        }
    }

    impl MacOsWindowSystem for FakeAppKit {
        fn retain_window(
            &mut self,
            window: MacOsWindow,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("retain");
            self.is_window(window)
                .then_some(())
                .ok_or_else(|| MacOsPresenterError::Operation("stale".into()))
        }

        fn release_window(&mut self, _window: MacOsWindow) {
            self.operation("release");
        }

        fn is_window(&self, window: MacOsWindow) -> bool {
            self.state
                .borrow()
                .get(&window)
                .is_some_and(|state| state.live)
        }

        fn parent_window(&self, window: MacOsWindow) -> Option<MacOsWindow> {
            self.state
                .borrow()
                .get(&window)
                .and_then(|state| state.parent)
        }

        fn collection_behavior(
            &self,
            window: MacOsWindow,
        ) -> Result<u64, MacOsPresenterError> {
            Ok(self.state.borrow()[&window].behavior)
        }

        fn set_collection_behavior(
            &mut self,
            window: MacOsWindow,
            behavior: u64,
        ) -> Result<(), MacOsPresenterError> {
            self.operation(format!("behavior:{behavior:#x}"));
            self.state.borrow_mut().get_mut(&window).unwrap().behavior =
                behavior;
            Ok(())
        }

        fn ignores_mouse_events(
            &self,
            window: MacOsWindow,
        ) -> Result<bool, MacOsPresenterError> {
            Ok(self.state.borrow()[&window].ignores_mouse)
        }

        fn set_ignores_mouse_events(
            &mut self,
            window: MacOsWindow,
            ignores: bool,
        ) -> Result<(), MacOsPresenterError> {
            self.operation(format!("ignores:{ignores}"));
            self.state
                .borrow_mut()
                .get_mut(&window)
                .unwrap()
                .ignores_mouse = ignores;
            Ok(())
        }

        fn add_child_above(
            &mut self,
            root: MacOsWindow,
            child: MacOsWindow,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("add_child");
            self.state.borrow_mut().get_mut(&child).unwrap().parent =
                Some(root);
            Ok(())
        }

        fn remove_child(&mut self, root: MacOsWindow, child: MacOsWindow) {
            self.operation("remove_child");
            let mut state = self.state.borrow_mut();
            let child = state.get_mut(&child).unwrap();
            if child.parent == Some(root) {
                child.parent = None;
            }
        }

        fn snapshot(
            &self,
            _root: MacOsWindow,
        ) -> Result<MacOsWindowSnapshot, MacOsPresenterError> {
            Ok(*self.snapshot.borrow())
        }

        fn position_overlay(
            &mut self,
            overlay: MacOsWindow,
            rect: MacOsScreenRect,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("position");
            self.state.borrow_mut().get_mut(&overlay).unwrap().frame =
                Some(rect);
            Ok(())
        }

        fn set_visible_without_activation(
            &mut self,
            overlay: MacOsWindow,
            visible: bool,
        ) -> Result<(), MacOsPresenterError> {
            self.operation(format!("visible:{visible}"));
            self.state.borrow_mut().get_mut(&overlay).unwrap().visible =
                visible;
            Ok(())
        }

        fn activate(
            &mut self,
            _window: MacOsWindow,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("activate");
            Ok(())
        }
    }

    #[test]
    fn unverified_appkit_path_falls_back_to_mpv_native_window() {
        let decision =
            MacOsPresenterDecision::evaluate(MacOsPresenterEvidence::default());

        assert!(!decision.capabilities.integrated_overlay);
        assert!(!decision.capabilities.embedded_surface);
        assert!(decision.capabilities.native_window_fallback);
        assert!(!decision.requires_wid());
        let fallback = decision.fallback.as_ref().unwrap();
        assert_eq!(fallback.code, FallbackReasonCode::MissingCapability);
        assert_eq!(fallback.from, Some(PlaybackTarget::MPV_INTEGRATED));
        assert_eq!(fallback.to, PlaybackTarget::MPV_NATIVE_WINDOW);
        assert!(fallback.detail.contains("appkit_main_thread"));
        assert!(fallback.detail.contains("spaces"));
    }

    #[test]
    fn presenter_decision_feeds_the_backend_fallback_policy() {
        let presenter =
            MacOsPresenterDecision::evaluate(MacOsPresenterEvidence::default());
        let candidates = [
            BackendCandidate::unavailable(
                PlaybackTarget::MPV_INTEGRATED,
                presenter.fallback.as_ref().unwrap().code,
            ),
            BackendCandidate::available(
                PlaybackTarget::MPV_NATIVE_WINDOW,
                true,
            ),
        ];

        let selected = select_backend(
            BackendRequest::Exact(PlaybackTarget::MPV_INTEGRATED),
            PlaybackRequirements::default(),
            &FallbackPolicy::migration_default(),
            &candidates,
        )
        .unwrap();

        assert_eq!(selected.selected, PlaybackTarget::MPV_NATIVE_WINDOW);
        assert_eq!(
            selected.fallback.unwrap().code,
            FallbackReasonCode::MissingCapability
        );
    }

    #[test]
    fn every_appkit_gate_is_required_before_integration_is_advertised() {
        let verified = MacOsPresenterEvidence::verified();
        let decision = MacOsPresenterDecision::evaluate(verified);
        assert!(decision.capabilities.integrated_overlay);
        assert!(decision.capabilities.fractional_scaling);
        assert_eq!(
            decision.capabilities.fullscreen_owner,
            Some(FullscreenOwner::VideoOutput)
        );
        assert!(decision.fallback.is_none());

        let missing_spaces =
            MacOsPresenterDecision::evaluate(MacOsPresenterEvidence {
                spaces_verified: false,
                ..verified
            });
        assert!(!missing_spaces.capabilities.integrated_overlay);
        assert_eq!(
            missing_spaces.blockers,
            vec![MacOsPresenterBlocker::SpacesUnverified]
        );
    }

    #[test]
    fn hdr_capability_requires_separate_native_output_evidence() {
        let without_hdr = MacOsPresenterDecision::evaluate(
            MacOsPresenterEvidence::verified(),
        );
        assert!(without_hdr.capabilities.integrated_overlay);
        assert!(!without_hdr.capabilities.native_hdr);

        let with_hdr =
            MacOsPresenterDecision::evaluate(MacOsPresenterEvidence {
                native_hdr_verified: true,
                ..MacOsPresenterEvidence::verified()
            });
        assert!(with_hdr.capabilities.native_hdr);
    }

    #[test]
    fn build_mode_and_mpv_window_id_fail_closed() {
        assert_eq!(
            MacOsPresenterBuildMode::parse(None).unwrap(),
            MacOsPresenterBuildMode::Disabled
        );
        assert_eq!(
            MacOsPresenterBuildMode::parse(Some("spike")).unwrap(),
            MacOsPresenterBuildMode::Spike
        );
        assert!(MacOsPresenterBuildMode::parse(Some("production")).is_err());
        assert!(MacOsWindow::from_mpv_window_id(0).is_err());
        assert_eq!(MacOsWindow::from_mpv_window_id(42).unwrap().get(), 42);
        let high_bit = MacOsWindow::from_mpv_window_id(i64::MIN).unwrap();
        assert_eq!(high_bit.get() as u64, i64::MIN as u64);
        assert!(!format!("{:?}", window(42)).contains("42"));
    }

    #[test]
    fn child_overlay_tracks_geometry_visibility_fullscreen_and_detach() {
        let video = window(10);
        let overlay = window(20);
        let appkit = FakeAppKit::new(video, overlay);
        let observed = appkit.clone();
        let fullscreen_values = Rc::new(RefCell::new(Vec::new()));
        let fullscreen_values_for_callback = Rc::clone(&fullscreen_values);
        let mut presenter = MacOsPresenter::new(
            appkit,
            move |fullscreen| {
                fullscreen_values_for_callback.borrow_mut().push(fullscreen);
                Ok(())
            },
            video,
            MacOsPresenterBuildMode::Spike,
        );
        let id = identity(1);

        presenter
            .attach(id, MacOsPresenterHost { overlay })
            .unwrap();
        {
            let state = observed.state.borrow();
            assert_eq!(state[&overlay].parent, Some(video));
            assert_eq!(
                state[&overlay].behavior,
                0x20 | COLLECTION_TRANSIENT | COLLECTION_FULLSCREEN_AUXILIARY
            );
            assert!(!state[&overlay].ignores_mouse);
            assert!(!state[&overlay].visible);
        }

        presenter.synchronize(id, geometry(true)).unwrap();
        presenter.set_visible(id, true).unwrap();
        assert!(observed.state.borrow()[&overlay].visible);
        assert_eq!(
            observed.state.borrow()[&overlay].frame,
            Some(observed.snapshot.borrow().content_rect)
        );
        assert_eq!(
            presenter.last_snapshot().unwrap().backing_scale_factor,
            2.0
        );

        presenter.set_suspended(id, true).unwrap();
        assert!(!observed.state.borrow()[&overlay].visible);
        presenter.set_suspended(id, false).unwrap();
        assert!(observed.state.borrow()[&overlay].visible);
        presenter
            .set_fullscreen(id, FullscreenOwner::VideoOutput, true)
            .unwrap();
        assert_eq!(&*fullscreen_values.borrow(), &[true]);
        presenter.focus_overlay().unwrap();

        presenter.detach(id);
        let state = observed.state.borrow();
        assert_eq!(state[&overlay].parent, None);
        assert_eq!(state[&overlay].behavior, 0x20);
        assert!(state[&overlay].ignores_mouse);
        assert!(!state[&overlay].visible);
        drop(state);
        let operations = observed.operations.borrow();
        let hide = operations
            .iter()
            .position(|operation| operation == "visible:false")
            .unwrap();
        let attach = operations
            .iter()
            .position(|operation| operation == "add_child")
            .unwrap();
        assert!(hide < attach);
        assert!(operations.iter().any(|operation| operation == "release"));
    }

    #[test]
    fn occlusion_does_not_oscillate_overlay_and_stale_generations_fail_safe() {
        let video = window(30);
        let overlay = window(40);
        let appkit = FakeAppKit::new(video, overlay);
        let observed = appkit.clone();
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Spike,
        );
        let id = identity(2);
        presenter
            .attach(id, MacOsPresenterHost { overlay })
            .unwrap();
        presenter.synchronize(id, geometry(true)).unwrap();
        presenter.set_visible(id, true).unwrap();
        assert!(observed.state.borrow()[&overlay].visible);

        let operation_count = observed.operations.borrow().len();
        observed.snapshot.borrow_mut().occluded = true;
        presenter.synchronize(id, geometry(true)).unwrap();
        assert!(observed.state.borrow()[&overlay].visible);
        assert_eq!(observed.operations.borrow().len(), operation_count);

        observed.snapshot.borrow_mut().miniaturized = true;
        presenter.synchronize(id, geometry(true)).unwrap();
        assert!(!observed.state.borrow()[&overlay].visible);

        let stale = identity(3);
        assert!(presenter.synchronize(stale, geometry(true)).is_err());
        presenter.detach(stale);
        assert_eq!(observed.state.borrow()[&overlay].parent, Some(video));
        presenter.detach(id);
    }

    #[test]
    fn disabled_presenter_rejects_attach_without_mutating_appkit() {
        let video = window(50);
        let overlay = window(60);
        let appkit = FakeAppKit::new(video, overlay);
        let observed = appkit.clone();
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Disabled,
        );

        assert!(
            presenter
                .attach(identity(4), MacOsPresenterHost { overlay })
                .is_err()
        );
        assert!(observed.operations.borrow().is_empty());
    }

    #[test]
    fn diagnostic_decision_never_contains_a_raw_window_value() {
        let decision =
            MacOsPresenterDecision::evaluate(MacOsPresenterEvidence::default());
        let debug = format!("{decision:?}");
        assert!(!debug.contains("0x"));
        assert!(!debug.contains("window-id"));
    }
}
