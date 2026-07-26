//! Conservative capability gate for the macOS native-root presenter.
//!
//! mpv's modern macOS backend owns its `NSWindow` and video layer. Ferrex may
//! reparent its transparent Iced `NSView` into that native root's content
//! hierarchy only after the relationship has been proven across AppKit
//! lifetime, fullscreen, Spaces, scale, and teardown transitions. No controls
//! `NSWindow` participates in presentation, and the presenter never advertises
//! `wid` embedding.

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

    /// Whether this build may attach the AppKit in-root presenter.
    pub const fn enabled(self) -> bool {
        matches!(self, Self::Spike)
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
    /// mpv owns the root `NSWindow`; a transparent Iced view lives within it.
    NativeRootSubview,
}

/// Stable, non-sensitive reason an integrated presenter is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacOsPresenterBlocker {
    AppKitMainThreadUnavailable,
    MpvWindowUnavailable,
    MpvWindowLifetimeUnverified,
    InRootViewRelationshipUnverified,
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
            Self::InRootViewRelationshipUnverified => {
                "in_root_view_relationship"
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
    pub in_root_view_relationship_verified: bool,
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
            in_root_view_relationship_verified: true,
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
                self.in_root_view_relationship_verified,
                MacOsPresenterBlocker::InRootViewRelationshipUnverified,
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
            strategy: MacOsPresenterStrategy::NativeRootSubview,
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
/// The spike remains conservative and does not advertise HDR until the native
/// Apple Silicon and Intel matrix proves real HDR/EDR behavior.
pub fn macos_presenter_capabilities(
    build_mode: MacOsPresenterBuildMode,
) -> PresenterCapabilities {
    PresenterCapabilities {
        integrated_overlay: build_mode.enabled(),
        embedded_surface: false,
        native_hdr: false,
        fractional_scaling: true,
        native_window_fallback: true,
        fullscreen_owner: Some(FullscreenOwner::VideoOutput),
        compositor_requirement: Some(
            "macOS AppKit in-root NSView composition".to_owned(),
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

/// Opaque, non-null AppKit `NSView` identity.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacOsView(NonZeroUsize);

impl MacOsView {
    /// Wrap a non-null pointer obtained from an AppKit object lease.
    pub const fn from_non_zero(value: NonZeroUsize) -> Self {
        Self(value)
    }

    pub const fn get(self) -> usize {
        self.0.get()
    }
}

impl fmt::Debug for MacOsView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MacOsView(<redacted>)")
    }
}

/// Iced view and its original staging owner, borrowed for one attach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacOsPresenterHost {
    pub view: MacOsView,
    pub original_owner: MacOsWindow,
}

#[cfg(all(target_os = "macos", feature = "ui"))]
impl MacOsPresenterHost {
    /// Capture Iced's AppKit `NSView` and its original owner `NSWindow`.
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
        let view_raw = NonZeroUsize::new(Retained::as_ptr(&view) as usize)
            .ok_or_else(|| presenter_error("Iced AppKit NSView is null"))?;
        let owner_raw = NonZeroUsize::new(Retained::as_ptr(&window) as usize)
            .ok_or_else(|| {
            presenter_error("Iced AppKit NSWindow is null")
        })?;
        Ok(Self {
            view: MacOsView::from_non_zero(view_raw),
            original_owner: MacOsWindow::from_non_zero(owner_raw),
        })
    }
}

/// Logical rectangle in the mpv root content view's local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacOsViewRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl MacOsViewRect {
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

/// Pointer-free AppKit observations retained for integration diagnostics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacOsWindowSnapshot {
    pub content_bounds: MacOsViewRect,
    pub overlay_frame: MacOsViewRect,
    pub backing_scale_factor: f64,
    pub visible_on_active_space: bool,
    pub occluded: bool,
    pub miniaturized: bool,
    pub fullscreen: bool,
    pub overlay_in_root_content: bool,
    pub overlay_topmost: bool,
    pub child_window_count: usize,
}

/// Original state restored after the Iced view leaves mpv's hierarchy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacOsViewState {
    pub frame: MacOsViewRect,
    pub autoresizing_mask: u64,
    pub hidden: bool,
}

/// AppKit operations isolated behind a display-free fakeable interface.
///
/// Deliberately absent are child-window, screen-positioning, and overlay-window
/// visibility operations. Presentation is exclusively an in-root `NSView`.
pub trait MacOsWindowSystem {
    fn retain_window(
        &mut self,
        window: MacOsWindow,
    ) -> Result<(), MacOsPresenterError>;
    fn release_window(&mut self, window: MacOsWindow);
    fn retain_view(
        &mut self,
        view: MacOsView,
    ) -> Result<(), MacOsPresenterError>;
    fn release_view(&mut self, view: MacOsView);
    fn is_window(&self, window: MacOsWindow) -> bool;
    fn is_view(&self, view: MacOsView) -> bool;
    fn view_window(&self, view: MacOsView) -> Option<MacOsWindow>;
    fn is_window_content_view(
        &self,
        owner: MacOsWindow,
        view: MacOsView,
    ) -> bool;
    fn view_state(
        &self,
        view: MacOsView,
    ) -> Result<MacOsViewState, MacOsPresenterError>;
    fn set_view_frame(
        &mut self,
        view: MacOsView,
        frame: MacOsViewRect,
    ) -> Result<(), MacOsPresenterError>;
    fn set_view_autoresizing_mask(
        &mut self,
        view: MacOsView,
        mask: u64,
    ) -> Result<(), MacOsPresenterError>;
    fn set_view_hidden(
        &mut self,
        view: MacOsView,
        hidden: bool,
    ) -> Result<(), MacOsPresenterError>;
    fn reparent_view_above(
        &mut self,
        root: MacOsWindow,
        view: MacOsView,
    ) -> Result<(), MacOsPresenterError>;
    fn raise_view_above(
        &mut self,
        root: MacOsWindow,
        view: MacOsView,
    ) -> Result<(), MacOsPresenterError>;
    /// Remove `view` from mpv, then restore it if `owner` remains usable.
    fn restore_view_to_owner(&mut self, owner: MacOsWindow, view: MacOsView);
    fn snapshot(
        &self,
        root: MacOsWindow,
        view: MacOsView,
    ) -> Result<MacOsWindowSnapshot, MacOsPresenterError>;
    fn focus_view(
        &mut self,
        root: MacOsWindow,
        view: MacOsView,
    ) -> Result<(), MacOsPresenterError>;
    fn begin_window_drag(
        &mut self,
        root: MacOsWindow,
    ) -> Result<bool, MacOsPresenterError>;
}

const VIEW_WIDTH_SIZABLE: u64 = 1 << 1;
const VIEW_HEIGHT_SIZABLE: u64 = 1 << 4;

#[derive(Debug, Clone, Copy)]
struct MacOsAttachment {
    identity: PresenterIdentity,
    view: MacOsView,
    original_owner: MacOsWindow,
    original_state: MacOsViewState,
}

/// UI-thread-local AppKit native-root/in-root-view presenter.
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
        MacOsPresenterStrategy::NativeRootSubview
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
            .focus_view(self.video_root, attachment.view)
            .map_err(PlaybackError::from)?;
        Ok(())
    }

    /// Hand the active mouse-down event to AppKit so the retained mpv root,
    /// rather than the hidden Iced staging owner, participates in the drag.
    ///
    /// Presses that race attachment, visibility, or teardown are intentionally
    /// ignored; they must not resurrect or move the donor window.
    pub fn begin_window_drag(&mut self) -> Result<bool, PlaybackError> {
        let Some(attachment) = self.attachment else {
            return Ok(false);
        };
        if self.applied_visible != Some(true)
            || self.suspended
            || !self.windows.is_window(self.video_root)
            || !self.windows.is_view(attachment.view)
            || self.windows.view_window(attachment.view)
                != Some(self.video_root)
        {
            return Ok(false);
        }
        let snapshot =
            self.windows.snapshot(self.video_root, attachment.view)?;
        if !snapshot.visible_on_active_space
            || snapshot.miniaturized
            || snapshot.fullscreen
            || !snapshot.overlay_in_root_content
        {
            return Ok(false);
        }
        self.windows
            .begin_window_drag(self.video_root)
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

    fn refresh_geometry_and_visibility(
        &mut self,
        attachment: MacOsAttachment,
    ) -> Result<(), PlaybackError> {
        if !self.windows.is_window(self.video_root)
            || !self.windows.is_window(attachment.original_owner)
            || !self.windows.is_view(attachment.view)
        {
            return Err(presenter_error(
                "macOS presenter AppKit lease was destroyed before synchronization",
            ));
        }
        let mut snapshot =
            self.windows.snapshot(self.video_root, attachment.view)?;
        let bounds = snapshot.content_bounds.validate()?;
        let overlay_frame = snapshot.overlay_frame.validate()?;
        if !snapshot.backing_scale_factor.is_finite()
            || snapshot.backing_scale_factor <= 0.0
        {
            return Err(presenter_error(
                "AppKit returned an invalid backing scale factor",
            ));
        }
        if !snapshot.overlay_in_root_content
            || self.windows.view_window(attachment.view)
                != Some(self.video_root)
        {
            return Err(presenter_error(
                "Iced AppKit view left the mpv content hierarchy",
            ));
        }
        let mut repaired = false;
        if !snapshot.overlay_topmost {
            self.windows
                .raise_view_above(self.video_root, attachment.view)?;
            repaired = true;
        }
        if overlay_frame != bounds {
            self.windows.set_view_frame(attachment.view, bounds)?;
            repaired = true;
        }
        if repaired {
            snapshot =
                self.windows.snapshot(self.video_root, attachment.view)?;
            if !snapshot.overlay_in_root_content
                || !snapshot.overlay_topmost
                || snapshot.overlay_frame.validate()? != bounds
                || self.windows.view_window(attachment.view)
                    != Some(self.video_root)
            {
                return Err(presenter_error(
                    "Iced AppKit view geometry or z-order repair did not stick",
                ));
            }
        }
        let visible = self.requested_visible
            && !self.suspended
            && self.geometry_visible
            && snapshot.visible_on_active_space
            && !snapshot.miniaturized;
        // AppKit naturally occludes the view with its owning mpv window. Do
        // not mutate visibility from the root's transient occlusion bit.
        if self.applied_visible != Some(visible) {
            self.windows.set_view_hidden(attachment.view, !visible)?;
            self.applied_visible = Some(visible);
        }
        self.last_snapshot = Some(snapshot);
        Ok(())
    }

    fn restore_attachment(&mut self, attachment: MacOsAttachment) {
        if self.windows.is_view(attachment.view) {
            // Prevent a one-frame flash while the Iced renderer returns to its
            // staging owner. The staging window itself is never used to
            // present controls and its visibility is not changed here.
            let _ = self.windows.set_view_hidden(attachment.view, true);
            // First remove the view from mpv unconditionally, then restore it
            // only if the retained donor is still usable.
            self.windows.restore_view_to_owner(
                attachment.original_owner,
                attachment.view,
            );
            let _ = self.windows.set_view_frame(
                attachment.view,
                attachment.original_state.frame,
            );
            let _ = self.windows.set_view_autoresizing_mask(
                attachment.view,
                attachment.original_state.autoresizing_mask,
            );
            let _ = self.windows.set_view_hidden(
                attachment.view,
                attachment.original_state.hidden,
            );
        }
        // Restoration must happen while both objects are still retained.
        self.windows.release_view(attachment.view);
        self.windows.release_window(attachment.original_owner);
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
        if !self.build_mode.enabled() {
            return Err(presenter_error(
                "macOS integrated presenter is disabled in this build",
            ));
        }
        if self.attachment.is_some() {
            return Err(presenter_error(
                "macOS presenter attach was requested more than once",
            ));
        }
        if host.original_owner == self.video_root {
            return Err(presenter_error(
                "Iced AppKit view is already owned by the mpv root window",
            ));
        }
        self.windows.retain_window(host.original_owner)?;
        if let Err(error) = self.windows.retain_view(host.view) {
            self.windows.release_window(host.original_owner);
            return Err(error.into());
        }
        if !self.windows.is_window(host.original_owner)
            || !self.windows.is_window(self.video_root)
            || !self.windows.is_view(host.view)
        {
            self.windows.release_view(host.view);
            self.windows.release_window(host.original_owner);
            return Err(presenter_error(
                "macOS presenter received a stale AppKit host",
            ));
        }
        if self.windows.view_window(host.view) != Some(host.original_owner)
            || !self
                .windows
                .is_window_content_view(host.original_owner, host.view)
        {
            self.windows.release_view(host.view);
            self.windows.release_window(host.original_owner);
            return Err(presenter_error(
                "Iced AppKit NSView is not its captured owner's content view",
            ));
        }

        let original_state = match self.windows.view_state(host.view) {
            Ok(state) => state,
            Err(error) => {
                self.windows.release_view(host.view);
                self.windows.release_window(host.original_owner);
                return Err(error.into());
            }
        };
        let attachment = MacOsAttachment {
            identity,
            view: host.view,
            original_owner: host.original_owner,
            original_state,
        };

        let setup = (|| {
            self.windows.set_view_hidden(host.view, true)?;
            self.applied_visible = Some(false);
            self.windows
                .reparent_view_above(self.video_root, host.view)?;
            self.windows.set_view_autoresizing_mask(
                host.view,
                VIEW_WIDTH_SIZABLE | VIEW_HEIGHT_SIZABLE,
            )
        })();
        if let Err(error) = setup {
            self.restore_attachment(attachment);
            return Err(error.into());
        }

        self.attachment = Some(attachment);
        if let Err(error) = self.refresh_geometry_and_visibility(attachment) {
            self.attachment = None;
            self.restore_attachment(attachment);
            return Err(error);
        }
        self.attachment = Some(attachment);
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
        self.refresh_geometry_and_visibility(attachment)
    }

    fn set_visible(
        &mut self,
        identity: PresenterIdentity,
        visible: bool,
    ) -> Result<(), PlaybackError> {
        let attachment = self.ensure_identity(identity)?;
        let was_applied = self.applied_visible;
        self.requested_visible = visible;
        self.refresh_geometry_and_visibility(attachment)?;
        if was_applied != Some(true) && self.applied_visible == Some(true) {
            // Winit's donor window is deliberately not made key. Keyboard
            // input follows the reparented view through mpv's root instead.
            self.windows.focus_view(self.video_root, attachment.view)?;
        }
        Ok(())
    }

    fn set_suspended(
        &mut self,
        identity: PresenterIdentity,
        suspended: bool,
    ) -> Result<(), PlaybackError> {
        let attachment = self.ensure_identity(identity)?;
        self.suspended = suspended;
        self.refresh_geometry_and_visibility(attachment)
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
        self.suspended = false;
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
    views: std::collections::HashMap<
        MacOsView,
        objc2::rc::Retained<objc2_app_kit::NSView>,
    >,
}

#[cfg(target_os = "macos")]
impl fmt::Debug for AppKitWindowSystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppKitWindowSystem")
            .field("retained_window_count", &self.windows.len())
            .field("retained_view_count", &self.views.len())
            .finish()
    }
}

#[cfg(target_os = "macos")]
impl AppKitWindowSystem {
    /// Whether the caller is the AppKit main thread.
    pub fn main_thread_available() -> bool {
        objc2::MainThreadMarker::new().is_some()
    }

    /// Resolve and retain mpv's live `NSWindow` on the AppKit main thread.
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
            views: std::collections::HashMap::new(),
        })
    }

    pub fn is_live(&self, window: MacOsWindow) -> bool {
        self.windows.contains_key(&window)
    }

    fn retain_native(
        window: MacOsWindow,
    ) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>, MacOsPresenterError>
    {
        let main_thread = objc2::MainThreadMarker::new()
            .ok_or(MacOsPresenterError::AppKitMainThreadRequired)?;
        let app = objc2_app_kit::NSApplication::sharedApplication(main_thread);
        app.windows()
            .into_iter()
            .find(|candidate| Self::window_identity(candidate) == window)
            .ok_or_else(|| {
                MacOsPresenterError::Operation(
                    "AppKit NSWindow identity is no longer live".to_owned(),
                )
            })
    }

    fn retain_native_view(
        view: MacOsView,
    ) -> Result<objc2::rc::Retained<objc2_app_kit::NSView>, MacOsPresenterError>
    {
        // SAFETY: the identity comes from raw-window-handle's live `ns_view`
        // lease and is retained on the AppKit main thread before that host
        // lease can end.
        unsafe {
            objc2::rc::Retained::retain(
                view.get() as *mut objc2_app_kit::NSView
            )
        }
        .ok_or_else(|| {
            MacOsPresenterError::Operation(
                "could not retain AppKit NSView".to_owned(),
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

    fn view(
        &self,
        view: MacOsView,
    ) -> Result<&objc2_app_kit::NSView, MacOsPresenterError> {
        self.views.get(&view).map(AsRef::as_ref).ok_or_else(|| {
            MacOsPresenterError::Operation(
                "AppKit view lease is unavailable".to_owned(),
            )
        })
    }

    fn window_identity(window: &objc2_app_kit::NSWindow) -> MacOsWindow {
        let raw = NonZeroUsize::new(window as *const _ as usize)
            .expect("Objective-C object references are non-null");
        MacOsWindow::from_non_zero(raw)
    }

    fn view_identity(view: &objc2_app_kit::NSView) -> MacOsView {
        let raw = NonZeroUsize::new(view as *const _ as usize)
            .expect("Objective-C object references are non-null");
        MacOsView::from_non_zero(raw)
    }
}

/// Synchronously remove mpv's live native root from the visible AppKit window
/// set without destroying it.
///
/// Shutdown remains asynchronous because libmpv may dispatch teardown work
/// back to AppKit. `orderOut:` is the non-destructive visibility barrier that
/// lets the shell restore or a replacement root open without a two-window
/// interval. Pointer identity is matched only against AppKit's retained live
/// application windows; the observed mpv value is never blindly retained at
/// teardown.
#[cfg(target_os = "macos")]
pub(crate) fn withdraw_mpv_root_window(
    native_window_id: i64,
) -> Result<(), PlaybackError> {
    let target = MacOsWindow::from_mpv_window_id(native_window_id)
        .map_err(PlaybackError::from)?;
    let main_thread = objc2::MainThreadMarker::new().ok_or_else(|| {
        presenter_error(
            "mpv native-root withdrawal requires the AppKit main thread",
        )
    })?;
    let app = objc2_app_kit::NSApplication::sharedApplication(main_thread);
    for window in app.windows() {
        let Some(raw) =
            NonZeroUsize::new(objc2::rc::Retained::as_ptr(&window) as usize)
        else {
            continue;
        };
        if MacOsWindow::from_non_zero(raw) != target {
            continue;
        }
        window.orderOut(None);
        if window.isVisible() {
            return Err(presenter_error(
                "AppKit kept mpv's native root visible after orderOut",
            ));
        }
        log::debug!("mpv native root withdrawn before asynchronous teardown");
        return Ok(());
    }

    // Absence from NSApplication.windows means the observed root has already
    // left the application's live top-level window set.
    log::debug!(
        "mpv native root was already absent from AppKit's live window set"
    );
    Ok(())
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

    fn retain_view(
        &mut self,
        view: MacOsView,
    ) -> Result<(), MacOsPresenterError> {
        if !self.views.contains_key(&view) {
            self.views.insert(view, Self::retain_native_view(view)?);
        }
        Ok(())
    }

    fn release_view(&mut self, view: MacOsView) {
        self.views.remove(&view);
    }

    fn is_window(&self, window: MacOsWindow) -> bool {
        self.is_live(window)
    }

    fn is_view(&self, view: MacOsView) -> bool {
        self.views.contains_key(&view)
    }

    fn view_window(&self, view: MacOsView) -> Option<MacOsWindow> {
        self.view(view)
            .ok()
            .and_then(objc2_app_kit::NSView::window)
            .as_deref()
            .map(Self::window_identity)
    }

    fn is_window_content_view(
        &self,
        owner: MacOsWindow,
        view: MacOsView,
    ) -> bool {
        self.window(owner)
            .ok()
            .and_then(objc2_app_kit::NSWindow::contentView)
            .as_deref()
            .map(Self::view_identity)
            == Some(view)
    }

    fn view_state(
        &self,
        view: MacOsView,
    ) -> Result<MacOsViewState, MacOsPresenterError> {
        let view = self.view(view)?;
        let frame = view.frame();
        Ok(MacOsViewState {
            frame: MacOsViewRect {
                x: frame.origin.x,
                y: frame.origin.y,
                width: frame.size.width,
                height: frame.size.height,
            },
            autoresizing_mask: view.autoresizingMask().0 as u64,
            hidden: view.isHidden(),
        })
    }

    fn set_view_frame(
        &mut self,
        view: MacOsView,
        frame: MacOsViewRect,
    ) -> Result<(), MacOsPresenterError> {
        let frame = frame.validate()?;
        self.view(view)?.setFrame(objc2_foundation::NSRect::new(
            objc2_foundation::NSPoint::new(frame.x, frame.y),
            objc2_foundation::NSSize::new(frame.width, frame.height),
        ));
        Ok(())
    }

    fn set_view_autoresizing_mask(
        &mut self,
        view: MacOsView,
        mask: u64,
    ) -> Result<(), MacOsPresenterError> {
        let mask = usize::try_from(mask).map_err(|_| {
            MacOsPresenterError::Operation(
                "AppKit autoresizing mask is out of range".to_owned(),
            )
        })?;
        self.view(view)?.setAutoresizingMask(
            objc2_app_kit::NSAutoresizingMaskOptions::from_bits_retain(mask),
        );
        Ok(())
    }

    fn raise_view_above(
        &mut self,
        root: MacOsWindow,
        view: MacOsView,
    ) -> Result<(), MacOsPresenterError> {
        let root_content =
            self.window(root)?.contentView().ok_or_else(|| {
                MacOsPresenterError::Operation(
                    "mpv AppKit window has no content view".to_owned(),
                )
            })?;
        let view = self.view(view)?;
        root_content.addSubview_positioned_relativeTo(
            view,
            objc2_app_kit::NSWindowOrderingMode::Above,
            None,
        );
        Ok(())
    }

    fn set_view_hidden(
        &mut self,
        view: MacOsView,
        hidden: bool,
    ) -> Result<(), MacOsPresenterError> {
        self.view(view)?.setHidden(hidden);
        Ok(())
    }

    fn reparent_view_above(
        &mut self,
        root: MacOsWindow,
        view: MacOsView,
    ) -> Result<(), MacOsPresenterError> {
        let root_content =
            self.window(root)?.contentView().ok_or_else(|| {
                MacOsPresenterError::Operation(
                    "mpv AppKit window has no content view".to_owned(),
                )
            })?;
        let view = self.view(view)?;
        view.removeFromSuperview();
        root_content.addSubview_positioned_relativeTo(
            view,
            objc2_app_kit::NSWindowOrderingMode::Above,
            None,
        );
        Ok(())
    }

    fn restore_view_to_owner(&mut self, owner: MacOsWindow, view: MacOsView) {
        let Ok(view) = self.view(view) else {
            return;
        };
        view.removeFromSuperview();
        if let Ok(owner) = self.window(owner) {
            owner.setContentView(Some(view));
        }
    }

    fn snapshot(
        &self,
        root: MacOsWindow,
        view: MacOsView,
    ) -> Result<MacOsWindowSnapshot, MacOsPresenterError> {
        let root = self.window(root)?;
        let view = self.view(view)?;
        let content = root.contentView().ok_or_else(|| {
            MacOsPresenterError::Operation(
                "mpv AppKit window has no content view".to_owned(),
            )
        })?;
        let bounds = content.bounds();
        let overlay_frame = view.frame();
        let occluded = !root
            .occlusionState()
            .contains(objc2_app_kit::NSWindowOcclusionState::Visible);
        let overlay_in_root_content =
            view.window().as_deref().map(Self::window_identity)
                == Some(Self::window_identity(root))
                && view.isDescendantOf(&content);
        let overlay_topmost = content
            .subviews()
            .into_iter()
            .last()
            .is_some_and(|candidate| {
                Self::view_identity(&candidate) == Self::view_identity(view)
            });
        let child_window_count = root
            .childWindows()
            .map(|children| children.count())
            .unwrap_or(0);
        Ok(MacOsWindowSnapshot {
            content_bounds: MacOsViewRect {
                x: bounds.origin.x,
                y: bounds.origin.y,
                width: bounds.size.width,
                height: bounds.size.height,
            },
            overlay_frame: MacOsViewRect {
                x: overlay_frame.origin.x,
                y: overlay_frame.origin.y,
                width: overlay_frame.size.width,
                height: overlay_frame.size.height,
            },
            backing_scale_factor: root.backingScaleFactor(),
            visible_on_active_space: root.isVisible() && root.isOnActiveSpace(),
            occluded,
            miniaturized: root.isMiniaturized(),
            fullscreen: root
                .styleMask()
                .contains(objc2_app_kit::NSWindowStyleMask::FullScreen),
            overlay_in_root_content,
            overlay_topmost,
            child_window_count,
        })
    }

    fn focus_view(
        &mut self,
        root: MacOsWindow,
        view: MacOsView,
    ) -> Result<(), MacOsPresenterError> {
        if self
            .window(root)?
            .makeFirstResponder(Some(self.view(view)?))
        {
            Ok(())
        } else {
            Err(MacOsPresenterError::Operation(
                "mpv AppKit window rejected the Iced first responder"
                    .to_owned(),
            ))
        }
    }

    fn begin_window_drag(
        &mut self,
        root: MacOsWindow,
    ) -> Result<bool, MacOsPresenterError> {
        let Some(event) =
            objc2_app_kit::NSApplication::sharedApplication(self._main_thread)
                .currentEvent()
        else {
            return Ok(false);
        };
        if event.r#type() != objc2_app_kit::NSEventType::LeftMouseDown {
            return Ok(false);
        }
        let root = self.window(root)?;
        let event_targets_root = event
            .window(self._main_thread)
            .as_deref()
            .map(Self::window_identity)
            == Some(Self::window_identity(root));
        if !event_targets_root {
            return Ok(false);
        }
        root.performWindowDragWithEvent(&event);
        Ok(true)
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

/// Pointer-free facts required before a presenter failure can be called a
/// completed native-window fallback.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeFallbackWindowObservation {
    visible: bool,
    on_active_space: bool,
    miniaturized: bool,
    has_content_view: bool,
    can_become_key: bool,
    movable: bool,
    titled: bool,
    resizable: bool,
    child_window_count: usize,
}

#[cfg(any(target_os = "macos", test))]
impl NativeFallbackWindowObservation {
    const fn qualifies(self) -> bool {
        self.visible
            && self.on_active_space
            && !self.miniaturized
            && self.has_content_view
            && self.can_become_key
            && self.movable
            && self.titled
            && self.resizable
            && self.child_window_count == 0
    }
}

/// Confirm that mpv's post-detach AppKit root is a live visible, movable,
/// resizable native window. The raw identity is matched only inside AppKit and
/// is never retained in the evidence projection.
#[cfg(target_os = "macos")]
pub(crate) fn verify_mpv_native_fallback_window(
    native_window_id: i64,
) -> Result<bool, PlaybackError> {
    let target = MacOsWindow::from_mpv_window_id(native_window_id)
        .map_err(PlaybackError::from)?;
    let main_thread = objc2::MainThreadMarker::new().ok_or_else(|| {
        presenter_error(
            "mpv native fallback verification requires the AppKit main thread",
        )
    })?;
    let app = objc2_app_kit::NSApplication::sharedApplication(main_thread);
    let Some(window) = app.windows().into_iter().find(|candidate| {
        AppKitWindowSystem::window_identity(candidate) == target
    }) else {
        return Ok(false);
    };
    let style = window.styleMask();
    Ok(NativeFallbackWindowObservation {
        visible: window.isVisible(),
        on_active_space: window.isOnActiveSpace(),
        miniaturized: window.isMiniaturized(),
        has_content_view: window.contentView().is_some(),
        can_become_key: window.canBecomeKeyWindow(),
        movable: window.isMovable(),
        titled: style.contains(objc2_app_kit::NSWindowStyleMask::Titled),
        resizable: style.contains(objc2_app_kit::NSWindowStyleMask::Resizable),
        child_window_count: window
            .childWindows()
            .map(|children| children.count())
            .unwrap_or(0),
    }
    .qualifies())
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

    fn view(value: usize) -> MacOsView {
        MacOsView::from_non_zero(NonZeroUsize::new(value).unwrap())
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
        content_view: MacOsView,
        visible: bool,
        active_space: bool,
        occluded: bool,
        miniaturized: bool,
        fullscreen: bool,
        backing_scale_factor: f64,
        child_window_count: usize,
    }

    #[derive(Debug, Clone, Copy)]
    struct ViewState {
        live: bool,
        window: Option<MacOsWindow>,
        superview: Option<MacOsView>,
        topmost_in_superview: bool,
        frame: MacOsViewRect,
        autoresizing_mask: u64,
        hidden: bool,
    }

    #[derive(Debug, Default)]
    struct FakeState {
        windows: HashMap<MacOsWindow, WindowState>,
        views: HashMap<MacOsView, ViewState>,
        retained_windows: HashMap<MacOsWindow, usize>,
        retained_views: HashMap<MacOsView, usize>,
        dragged_roots: Vec<MacOsWindow>,
        drag_event_available: bool,
    }

    #[derive(Debug, Clone)]
    struct FakeAppKit {
        state: Rc<RefCell<FakeState>>,
        operations: Rc<RefCell<Vec<String>>>,
        fail_next: Rc<RefCell<Option<&'static str>>>,
    }

    impl FakeAppKit {
        fn new(
            video: MacOsWindow,
            owner: MacOsWindow,
            overlay: MacOsView,
        ) -> Self {
            let root_content = view(9_000_000 + video.get());
            let content_bounds = MacOsViewRect {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 720.0,
            };
            Self {
                state: Rc::new(RefCell::new(FakeState {
                    windows: HashMap::from([
                        (
                            video,
                            WindowState {
                                live: true,
                                content_view: root_content,
                                visible: true,
                                active_space: true,
                                occluded: false,
                                miniaturized: false,
                                fullscreen: false,
                                backing_scale_factor: 2.0,
                                child_window_count: 0,
                            },
                        ),
                        (
                            owner,
                            WindowState {
                                live: true,
                                content_view: overlay,
                                // The shell hides the retained donor before it
                                // asks the presenter to expose the in-root view.
                                visible: false,
                                active_space: true,
                                occluded: false,
                                miniaturized: false,
                                fullscreen: false,
                                backing_scale_factor: 2.0,
                                child_window_count: 0,
                            },
                        ),
                    ]),
                    views: HashMap::from([
                        (
                            root_content,
                            ViewState {
                                live: true,
                                window: Some(video),
                                superview: None,
                                topmost_in_superview: false,
                                frame: content_bounds,
                                autoresizing_mask: 0,
                                hidden: false,
                            },
                        ),
                        (
                            overlay,
                            ViewState {
                                live: true,
                                window: Some(owner),
                                superview: None,
                                topmost_in_superview: false,
                                frame: MacOsViewRect {
                                    x: 5.0,
                                    y: 10.0,
                                    width: 640.0,
                                    height: 360.0,
                                },
                                autoresizing_mask: 0x20,
                                hidden: false,
                            },
                        ),
                    ]),
                    drag_event_available: true,
                    ..FakeState::default()
                })),
                operations: Rc::new(RefCell::new(Vec::new())),
                fail_next: Rc::new(RefCell::new(None)),
            }
        }

        fn operation(&self, value: impl Into<String>) {
            self.operations.borrow_mut().push(value.into());
        }

        fn maybe_fail(
            &self,
            operation: &'static str,
        ) -> Result<(), MacOsPresenterError> {
            if self.fail_next.borrow().as_ref() == Some(&operation) {
                self.fail_next.borrow_mut().take();
                self.operation(format!("fail:{operation}"));
                Err(MacOsPresenterError::Operation(format!(
                    "injected {operation} failure"
                )))
            } else {
                Ok(())
            }
        }

        fn fail_on(&self, operation: &'static str) {
            *self.fail_next.borrow_mut() = Some(operation);
        }

        fn original_view_state(&self, overlay: MacOsView) -> MacOsViewState {
            let state = self.state.borrow().views[&overlay];
            MacOsViewState {
                frame: state.frame,
                autoresizing_mask: state.autoresizing_mask,
                hidden: state.hidden,
            }
        }

        fn visible_top_level_count(&self) -> usize {
            self.state
                .borrow()
                .windows
                .values()
                .filter(|window| window.visible)
                .count()
        }

        fn lease_count(&self) -> usize {
            let state = self.state.borrow();
            state.retained_windows.values().sum::<usize>()
                + state.retained_views.values().sum::<usize>()
        }

        fn assert_host_restored(
            &self,
            owner: MacOsWindow,
            overlay: MacOsView,
            original: MacOsViewState,
        ) {
            let state = self.state.borrow();
            let view = state.views[&overlay];
            assert_eq!(view.window, Some(owner));
            assert_eq!(view.superview, None);
            assert_eq!(state.windows[&owner].content_view, overlay);
            assert_eq!(view.frame, original.frame);
            assert_eq!(view.autoresizing_mask, original.autoresizing_mask);
            assert_eq!(view.hidden, original.hidden);
        }
    }

    impl MacOsWindowSystem for FakeAppKit {
        fn retain_window(
            &mut self,
            window: MacOsWindow,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("retain_window");
            if !self.is_window(window) {
                return Err(MacOsPresenterError::Operation("stale".into()));
            }
            *self
                .state
                .borrow_mut()
                .retained_windows
                .entry(window)
                .or_default() += 1;
            Ok(())
        }

        fn release_window(&mut self, window: MacOsWindow) {
            self.operation("release_window");
            let mut state = self.state.borrow_mut();
            let count = state.retained_windows.get_mut(&window).unwrap();
            *count -= 1;
            if *count == 0 {
                state.retained_windows.remove(&window);
            }
        }

        fn retain_view(
            &mut self,
            view: MacOsView,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("retain_view");
            if !self.is_view(view) {
                return Err(MacOsPresenterError::Operation("stale".into()));
            }
            *self
                .state
                .borrow_mut()
                .retained_views
                .entry(view)
                .or_default() += 1;
            Ok(())
        }

        fn release_view(&mut self, view: MacOsView) {
            self.operation("release_view");
            let mut state = self.state.borrow_mut();
            let count = state.retained_views.get_mut(&view).unwrap();
            *count -= 1;
            if *count == 0 {
                state.retained_views.remove(&view);
            }
        }

        fn is_window(&self, window: MacOsWindow) -> bool {
            self.state
                .borrow()
                .windows
                .get(&window)
                .is_some_and(|state| state.live)
        }

        fn is_view(&self, view: MacOsView) -> bool {
            self.state
                .borrow()
                .views
                .get(&view)
                .is_some_and(|state| state.live)
        }

        fn view_window(&self, view: MacOsView) -> Option<MacOsWindow> {
            self.state
                .borrow()
                .views
                .get(&view)
                .and_then(|state| state.window)
        }

        fn is_window_content_view(
            &self,
            owner: MacOsWindow,
            view: MacOsView,
        ) -> bool {
            let state = self.state.borrow();
            state.windows.get(&owner).is_some_and(|window| {
                window.content_view == view
                    && state.views[&view].window == Some(owner)
            })
        }

        fn view_state(
            &self,
            view: MacOsView,
        ) -> Result<MacOsViewState, MacOsPresenterError> {
            let state = self.state.borrow().views[&view];
            Ok(MacOsViewState {
                frame: state.frame,
                autoresizing_mask: state.autoresizing_mask,
                hidden: state.hidden,
            })
        }

        fn set_view_frame(
            &mut self,
            view: MacOsView,
            frame: MacOsViewRect,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("set_frame");
            self.maybe_fail("set_frame")?;
            self.state.borrow_mut().views.get_mut(&view).unwrap().frame = frame;
            Ok(())
        }

        fn set_view_autoresizing_mask(
            &mut self,
            view: MacOsView,
            mask: u64,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("set_autoresizing");
            self.maybe_fail("set_autoresizing")?;
            self.state
                .borrow_mut()
                .views
                .get_mut(&view)
                .unwrap()
                .autoresizing_mask = mask;
            Ok(())
        }

        fn set_view_hidden(
            &mut self,
            view: MacOsView,
            hidden: bool,
        ) -> Result<(), MacOsPresenterError> {
            self.operation(format!("set_hidden:{hidden}"));
            self.maybe_fail("set_hidden")?;
            self.state.borrow_mut().views.get_mut(&view).unwrap().hidden =
                hidden;
            Ok(())
        }

        fn reparent_view_above(
            &mut self,
            root: MacOsWindow,
            view: MacOsView,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("reparent_view");
            self.maybe_fail("reparent_view")?;
            let mut state = self.state.borrow_mut();
            let root_content = state.windows[&root].content_view;
            let view = state.views.get_mut(&view).unwrap();
            view.window = Some(root);
            view.superview = Some(root_content);
            view.topmost_in_superview = true;
            Ok(())
        }

        fn raise_view_above(
            &mut self,
            root: MacOsWindow,
            view: MacOsView,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("raise_view");
            self.maybe_fail("raise_view")?;
            let mut state = self.state.borrow_mut();
            let root_content = state.windows[&root].content_view;
            let view = state.views.get_mut(&view).unwrap();
            if view.window != Some(root) || view.superview != Some(root_content)
            {
                return Err(MacOsPresenterError::Operation(
                    "view is outside root".into(),
                ));
            }
            view.topmost_in_superview = true;
            Ok(())
        }

        fn restore_view_to_owner(
            &mut self,
            owner: MacOsWindow,
            view: MacOsView,
        ) {
            self.operation("restore_owner");
            let mut state = self.state.borrow_mut();
            let owner_live =
                state.windows.get(&owner).is_some_and(|window| window.live);
            if owner_live {
                state.windows.get_mut(&owner).unwrap().content_view = view;
            }
            let view_state = state.views.get_mut(&view).unwrap();
            view_state.window = owner_live.then_some(owner);
            view_state.superview = None;
            view_state.topmost_in_superview = false;
        }

        fn snapshot(
            &self,
            root: MacOsWindow,
            view: MacOsView,
        ) -> Result<MacOsWindowSnapshot, MacOsPresenterError> {
            let state = self.state.borrow();
            let root_state = &state.windows[&root];
            let root_content = root_state.content_view;
            let view_state = &state.views[&view];
            Ok(MacOsWindowSnapshot {
                content_bounds: state.views[&root_content].frame,
                overlay_frame: view_state.frame,
                backing_scale_factor: root_state.backing_scale_factor,
                visible_on_active_space: root_state.visible
                    && root_state.active_space,
                occluded: root_state.occluded,
                miniaturized: root_state.miniaturized,
                fullscreen: root_state.fullscreen,
                overlay_in_root_content: view_state.window == Some(root)
                    && view_state.superview == Some(root_content),
                overlay_topmost: view_state.topmost_in_superview,
                child_window_count: root_state.child_window_count,
            })
        }

        fn focus_view(
            &mut self,
            root: MacOsWindow,
            view: MacOsView,
        ) -> Result<(), MacOsPresenterError> {
            self.operation("focus_root_view");
            (self.view_window(view) == Some(root))
                .then_some(())
                .ok_or_else(|| {
                    MacOsPresenterError::Operation(
                        "view is outside root".into(),
                    )
                })
        }

        fn begin_window_drag(
            &mut self,
            root: MacOsWindow,
        ) -> Result<bool, MacOsPresenterError> {
            self.operation("begin_window_drag");
            self.maybe_fail("begin_window_drag")?;
            if !self.state.borrow().drag_event_available {
                return Ok(false);
            }
            if !self.is_window(root) {
                return Err(MacOsPresenterError::Operation(
                    "drag root is stale".into(),
                ));
            }
            self.state.borrow_mut().dragged_roots.push(root);
            Ok(true)
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
    fn native_fallback_requires_one_manageable_live_mpv_root() {
        let verified = NativeFallbackWindowObservation {
            visible: true,
            on_active_space: true,
            miniaturized: false,
            has_content_view: true,
            can_become_key: true,
            movable: true,
            titled: true,
            resizable: true,
            child_window_count: 0,
        };
        assert!(verified.qualifies());

        for unqualified in [
            NativeFallbackWindowObservation {
                visible: false,
                ..verified
            },
            NativeFallbackWindowObservation {
                on_active_space: false,
                ..verified
            },
            NativeFallbackWindowObservation {
                miniaturized: true,
                ..verified
            },
            NativeFallbackWindowObservation {
                has_content_view: false,
                ..verified
            },
            NativeFallbackWindowObservation {
                can_become_key: false,
                ..verified
            },
            NativeFallbackWindowObservation {
                movable: false,
                ..verified
            },
            NativeFallbackWindowObservation {
                titled: false,
                ..verified
            },
            NativeFallbackWindowObservation {
                resizable: false,
                ..verified
            },
            NativeFallbackWindowObservation {
                child_window_count: 1,
                ..verified
            },
        ] {
            assert!(!unqualified.qualifies());
        }
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
        assert!(!MacOsPresenterBuildMode::Disabled.enabled());
        assert!(MacOsPresenterBuildMode::Spike.enabled());
        assert!(MacOsPresenterBuildMode::parse(Some("production")).is_err());
        assert!(MacOsWindow::from_mpv_window_id(0).is_err());
        assert_eq!(MacOsWindow::from_mpv_window_id(42).unwrap().get(), 42);
        let high_bit = MacOsWindow::from_mpv_window_id(i64::MIN).unwrap();
        assert_eq!(high_bit.get() as u64, i64::MIN as u64);
        assert!(!format!("{:?}", window(42)).contains("42"));
        assert!(!format!("{:?}", view(43)).contains("43"));
    }

    #[test]
    fn in_root_view_tracks_visibility_fullscreen_focus_and_detach() {
        let video = window(10);
        let donor = window(20);
        let overlay = view(30);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let original = observed.original_view_state(overlay);
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
            .attach(
                id,
                MacOsPresenterHost {
                    view: overlay,
                    original_owner: donor,
                },
            )
            .unwrap();
        {
            let state = observed.state.borrow();
            let overlay_state = state.views[&overlay];
            let root_content = state.windows[&video].content_view;
            assert_eq!(overlay_state.window, Some(video));
            assert_eq!(overlay_state.superview, Some(root_content));
            assert_eq!(
                overlay_state.autoresizing_mask,
                VIEW_WIDTH_SIZABLE | VIEW_HEIGHT_SIZABLE
            );
            assert!(overlay_state.hidden);
            assert_eq!(state.windows[&video].child_window_count, 0);
            assert!(!state.windows[&donor].visible);
        }
        assert_eq!(observed.visible_top_level_count(), 1);

        presenter.synchronize(id, geometry(true)).unwrap();
        presenter.set_visible(id, true).unwrap();
        let state = observed.state.borrow();
        let root_content = state.windows[&video].content_view;
        assert!(!state.views[&overlay].hidden);
        assert_eq!(
            state.views[&overlay].frame,
            state.views[&root_content].frame
        );
        assert!(!state.windows[&donor].visible);
        drop(state);
        assert_eq!(
            presenter.last_snapshot().unwrap().backing_scale_factor,
            2.0
        );
        assert!(presenter.last_snapshot().unwrap().overlay_in_root_content);
        assert!(presenter.last_snapshot().unwrap().overlay_topmost);
        assert_eq!(
            presenter.last_snapshot().unwrap().overlay_frame,
            presenter.last_snapshot().unwrap().content_bounds
        );
        assert_eq!(presenter.last_snapshot().unwrap().child_window_count, 0);

        presenter.set_suspended(id, true).unwrap();
        assert!(observed.state.borrow().views[&overlay].hidden);
        presenter.set_suspended(id, false).unwrap();
        assert!(!observed.state.borrow().views[&overlay].hidden);
        presenter
            .set_fullscreen(id, FullscreenOwner::VideoOutput, true)
            .unwrap();
        assert_eq!(&*fullscreen_values.borrow(), &[true]);
        assert_eq!(
            observed
                .operations
                .borrow()
                .iter()
                .filter(|operation| operation.as_str() == "focus_root_view")
                .count(),
            1
        );

        presenter.detach(id);
        observed.assert_host_restored(donor, overlay, original);
        assert_eq!(observed.lease_count(), 0);
        assert_eq!(observed.visible_top_level_count(), 1);
        let operations = observed.operations.borrow();
        let hide = operations
            .iter()
            .position(|operation| operation == "set_hidden:true")
            .unwrap();
        let attach = operations
            .iter()
            .position(|operation| operation == "reparent_view")
            .unwrap();
        assert!(hide < attach);
        let restore = operations
            .iter()
            .rposition(|operation| operation == "restore_owner")
            .unwrap();
        let release_view = operations
            .iter()
            .rposition(|operation| operation == "release_view")
            .unwrap();
        let release_window = operations
            .iter()
            .rposition(|operation| operation == "release_window")
            .unwrap();
        assert!(restore < release_view);
        assert!(release_view < release_window);
        assert!(
            operations
                .iter()
                .all(|operation| !operation.contains("window_visible"))
        );
    }

    #[test]
    fn synchronize_repairs_stale_overlay_frame_when_root_bounds_are_unchanged()
    {
        let video = window(11);
        let donor = window(12);
        let overlay = view(13);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Spike,
        );
        let id = identity(11);
        presenter
            .attach(
                id,
                MacOsPresenterHost {
                    view: overlay,
                    original_owner: donor,
                },
            )
            .unwrap();

        let expected = {
            let state = observed.state.borrow();
            let root_content = state.windows[&video].content_view;
            state.views[&root_content].frame
        };
        observed
            .state
            .borrow_mut()
            .views
            .get_mut(&overlay)
            .unwrap()
            .frame = MacOsViewRect {
            x: 17.0,
            y: 23.0,
            width: 400.0,
            height: 300.0,
        };
        observed.operations.borrow_mut().clear();

        presenter.synchronize(id, geometry(true)).unwrap();

        assert_eq!(observed.state.borrow().views[&overlay].frame, expected);
        assert_eq!(presenter.last_snapshot().unwrap().overlay_frame, expected);
        let operations = observed.operations.borrow();
        assert_eq!(
            operations
                .iter()
                .filter(|operation| operation.as_str() == "set_frame")
                .count(),
            1
        );
        assert!(!operations.iter().any(|operation| operation == "raise_view"));
    }

    #[test]
    fn synchronize_repairs_lost_topmost_order_without_reparenting_host() {
        let video = window(14);
        let donor = window(15);
        let overlay = view(16);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Spike,
        );
        let id = identity(12);
        presenter
            .attach(
                id,
                MacOsPresenterHost {
                    view: overlay,
                    original_owner: donor,
                },
            )
            .unwrap();

        observed
            .state
            .borrow_mut()
            .views
            .get_mut(&overlay)
            .unwrap()
            .topmost_in_superview = false;
        observed.operations.borrow_mut().clear();

        presenter.synchronize(id, geometry(true)).unwrap();

        assert!(observed.state.borrow().views[&overlay].topmost_in_superview);
        assert!(presenter.last_snapshot().unwrap().overlay_topmost);
        let operations = observed.operations.borrow();
        assert_eq!(
            operations
                .iter()
                .filter(|operation| operation.as_str() == "raise_view")
                .count(),
            1
        );
        assert!(
            !operations
                .iter()
                .any(|operation| operation == "reparent_view"),
            "z-order repair must not detach the live foreign-hosted view"
        );
        assert!(!operations.iter().any(|operation| operation == "set_frame"));
    }

    #[test]
    fn native_drag_targets_visible_attached_mpv_root_and_other_states_no_op() {
        let video = window(21);
        let donor = window(22);
        let overlay = view(23);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Spike,
        );
        let id = identity(21);

        assert!(!presenter.begin_window_drag().unwrap());
        presenter
            .attach(
                id,
                MacOsPresenterHost {
                    view: overlay,
                    original_owner: donor,
                },
            )
            .unwrap();
        assert!(!presenter.begin_window_drag().unwrap());

        presenter.synchronize(id, geometry(true)).unwrap();
        presenter.set_visible(id, true).unwrap();
        assert!(presenter.begin_window_drag().unwrap());
        assert_eq!(observed.state.borrow().dragged_roots, vec![video]);
        assert!(!observed.state.borrow().dragged_roots.contains(&donor));

        observed.state.borrow_mut().drag_event_available = false;
        assert!(!presenter.begin_window_drag().unwrap());
        observed.state.borrow_mut().drag_event_available = true;

        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&video)
            .unwrap()
            .active_space = false;
        assert!(!presenter.begin_window_drag().unwrap());
        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&video)
            .unwrap()
            .active_space = true;

        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&video)
            .unwrap()
            .miniaturized = true;
        assert!(!presenter.begin_window_drag().unwrap());
        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&video)
            .unwrap()
            .miniaturized = false;

        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&video)
            .unwrap()
            .fullscreen = true;
        assert!(!presenter.begin_window_drag().unwrap());
        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&video)
            .unwrap()
            .fullscreen = false;

        presenter.set_suspended(id, true).unwrap();
        assert!(!presenter.begin_window_drag().unwrap());
        presenter.set_suspended(id, false).unwrap();
        observed
            .state
            .borrow_mut()
            .views
            .get_mut(&overlay)
            .unwrap()
            .window = Some(donor);
        assert!(!presenter.begin_window_drag().unwrap());
        assert_eq!(observed.state.borrow().dragged_roots, vec![video]);

        presenter.detach(id);
        assert!(!presenter.begin_window_drag().unwrap());
    }

    #[test]
    fn occlusion_does_not_oscillate_overlay_and_stale_generations_fail_safe() {
        let video = window(30);
        let donor = window(40);
        let overlay = view(50);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Spike,
        );
        let id = identity(2);
        presenter
            .attach(
                id,
                MacOsPresenterHost {
                    view: overlay,
                    original_owner: donor,
                },
            )
            .unwrap();
        presenter.synchronize(id, geometry(true)).unwrap();
        presenter.set_visible(id, true).unwrap();
        assert!(!observed.state.borrow().views[&overlay].hidden);

        let operation_count = observed.operations.borrow().len();
        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&video)
            .unwrap()
            .occluded = true;
        presenter.synchronize(id, geometry(true)).unwrap();
        assert!(!observed.state.borrow().views[&overlay].hidden);
        assert_eq!(observed.operations.borrow().len(), operation_count);

        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&video)
            .unwrap()
            .miniaturized = true;
        presenter.synchronize(id, geometry(true)).unwrap();
        assert!(observed.state.borrow().views[&overlay].hidden);

        let stale = identity(3);
        assert!(presenter.synchronize(stale, geometry(true)).is_err());
        presenter.detach(stale);
        assert_eq!(observed.state.borrow().views[&overlay].window, Some(video));
        presenter.detach(id);
        assert_eq!(observed.lease_count(), 0);
    }

    #[test]
    fn partial_attach_failure_restores_donor_before_releasing_leases() {
        let video = window(60);
        let donor = window(70);
        let overlay = view(80);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let original = observed.original_view_state(overlay);
        // Initial refresh sets local root bounds after reparenting.
        observed.fail_on("set_frame");
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Spike,
        );

        let error = presenter
            .attach(
                identity(4),
                MacOsPresenterHost {
                    view: overlay,
                    original_owner: donor,
                },
            )
            .unwrap_err();

        assert!(error.message.contains("injected set_frame failure"));
        observed.assert_host_restored(donor, overlay, original);
        assert_eq!(observed.lease_count(), 0);
        assert_eq!(observed.visible_top_level_count(), 1);
        let operations = observed.operations.borrow();
        let reparent = operations
            .iter()
            .position(|operation| operation == "reparent_view")
            .unwrap();
        let failure = operations
            .iter()
            .position(|operation| operation == "fail:set_frame")
            .unwrap();
        let restore = operations
            .iter()
            .position(|operation| operation == "restore_owner")
            .unwrap();
        let release = operations
            .iter()
            .position(|operation| operation == "release_view")
            .unwrap();
        assert!(reparent < failure);
        assert!(failure < restore);
        assert!(restore < release);
    }

    #[test]
    fn donor_invalidation_still_removes_view_from_mpv_before_release() {
        let video = window(81);
        let donor = window(82);
        let overlay = view(83);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Spike,
        );
        let id = identity(5);
        presenter
            .attach(
                id,
                MacOsPresenterHost {
                    view: overlay,
                    original_owner: donor,
                },
            )
            .unwrap();
        observed
            .state
            .borrow_mut()
            .windows
            .get_mut(&donor)
            .unwrap()
            .live = false;

        presenter.detach(id);

        let state = observed.state.borrow();
        assert_eq!(state.views[&overlay].window, None);
        assert_eq!(state.views[&overlay].superview, None);
        drop(state);
        assert_eq!(observed.lease_count(), 0);
        let operations = observed.operations.borrow();
        let restore = operations
            .iter()
            .rposition(|operation| operation == "restore_owner")
            .unwrap();
        let release = operations
            .iter()
            .rposition(|operation| operation == "release_view")
            .unwrap();
        assert!(restore < release);
    }

    #[test]
    fn one_hundred_attach_sync_fullscreen_detach_cycles_have_zero_growth() {
        let video = window(90);
        let donor = window(100);
        let overlay = view(110);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let original = observed.original_view_state(overlay);
        let fullscreen_values = Rc::new(RefCell::new(Vec::new()));
        let callback_values = Rc::clone(&fullscreen_values);
        let mut presenter = MacOsPresenter::new(
            appkit,
            move |fullscreen| {
                callback_values.borrow_mut().push(fullscreen);
                Ok(())
            },
            video,
            MacOsPresenterBuildMode::Spike,
        );

        for cycle in 1..=100 {
            let id = identity(cycle);
            let host = MacOsPresenterHost {
                view: overlay,
                original_owner: donor,
            };
            presenter.attach(id, host).unwrap();
            presenter.synchronize(id, geometry(true)).unwrap();
            presenter.set_visible(id, true).unwrap();
            presenter
                .set_fullscreen(id, FullscreenOwner::VideoOutput, true)
                .unwrap();
            presenter
                .set_fullscreen(id, FullscreenOwner::VideoOutput, false)
                .unwrap();
            assert!(presenter.last_snapshot().unwrap().overlay_in_root_content);
            assert!(presenter.last_snapshot().unwrap().overlay_topmost);
            assert_eq!(
                presenter.last_snapshot().unwrap().overlay_frame,
                presenter.last_snapshot().unwrap().content_bounds
            );
            assert_eq!(
                presenter.last_snapshot().unwrap().child_window_count,
                0
            );
            assert_eq!(observed.visible_top_level_count(), 1);
            presenter.detach(id);
            observed.assert_host_restored(donor, overlay, original);
            assert_eq!(observed.lease_count(), 0, "cycle {cycle}");
        }

        assert_eq!(fullscreen_values.borrow().len(), 200);
        assert_eq!(observed.visible_top_level_count(), 1);
    }

    #[test]
    fn disabled_presenter_rejects_attach_without_mutating_appkit() {
        let video = window(120);
        let donor = window(130);
        let overlay = view(140);
        let appkit = FakeAppKit::new(video, donor, overlay);
        let observed = appkit.clone();
        let mut presenter = MacOsPresenter::new(
            appkit,
            |_| Ok(()),
            video,
            MacOsPresenterBuildMode::Disabled,
        );

        assert!(
            presenter
                .attach(
                    identity(5),
                    MacOsPresenterHost {
                        view: overlay,
                        original_owner: donor,
                    },
                )
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
