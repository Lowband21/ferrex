//! Windows native-root presenter spike and rollout capability model.
//!
//! mpv owns the video/root `HWND`; Iced owns a transparent, undecorated
//! overlay. The production gate is intentionally separate from the native
//! operations in this module: compiling the spike must not make Auto select an
//! unverified presenter.

use std::{fmt, num::NonZeroIsize};

use crate::{
    contract::{
        BackendCandidate, FallbackReason, FallbackReasonCode, PlaybackError,
        PlaybackErrorKind, PlaybackTarget,
    },
    presenter::{
        FullscreenOwner, NativePresenter, PresenterCapabilities,
        PresenterIdentity, SurfaceGeometry,
    },
};

/// Build-time switch used to compile developer-only Windows presenter work.
pub const WINDOWS_PRESENTER_BUILD_ENV: &str = "FERREX_MPV_WINDOWS_PRESENTER";

/// Whether the target contains the Windows presenter spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowsPresenterBuildMode {
    /// Release-safe default. Integrated mpv is reported unavailable.
    #[default]
    Disabled,
    /// Developer-only native relationship spike; never an Auto rollout gate.
    Spike,
}

impl WindowsPresenterBuildMode {
    /// Parse the build environment value. Unknown values fail closed.
    pub fn parse(value: Option<&str>) -> Result<Self, WindowsPresenterError> {
        match value.map(str::trim) {
            None | Some("") | Some("disabled") => Ok(Self::Disabled),
            Some("spike") => Ok(Self::Spike),
            Some(value) => {
                Err(WindowsPresenterError::InvalidBuildMode(value.to_owned()))
            }
        }
    }

    /// Mode compiled into this crate.
    #[cfg(target_os = "windows")]
    pub fn compiled() -> Self {
        // Invalid release configuration must not silently enable integration.
        Self::parse(option_env!("FERREX_MPV_WINDOWS_PRESENTER"))
            .unwrap_or(Self::Disabled)
    }
}

/// Evidence available to the Windows presenter capability decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsPresenterProbe {
    pub build_mode: WindowsPresenterBuildMode,
    pub mpv_window_id_observed: bool,
    pub iced_overlay_handle_observed: bool,
    pub dwm_composition_available: bool,
    /// Set only by a reviewed decision after the manual Windows matrix passes.
    pub production_gate_approved: bool,
    /// Set only after HDR with the overlay visible/hidden is validated.
    pub native_hdr_validated: bool,
}

impl Default for WindowsPresenterProbe {
    fn default() -> Self {
        Self {
            build_mode: WindowsPresenterBuildMode::Disabled,
            mpv_window_id_observed: false,
            iced_overlay_handle_observed: false,
            dwm_composition_available: false,
            production_gate_approved: false,
            native_hdr_validated: false,
        }
    }
}

/// Result of separating technical spike readiness from rollout approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsPresenterAvailability {
    Unavailable {
        code: FallbackReasonCode,
        detail: &'static str,
    },
    /// Native handles and DWM are available for an explicit developer spike.
    SpikeReady,
    /// All code and manual rollout gates have been approved.
    ProductionReady { native_hdr: bool },
}

impl WindowsPresenterAvailability {
    /// Evaluate prerequisites in a stable order for actionable diagnostics.
    pub const fn evaluate(probe: WindowsPresenterProbe) -> Self {
        if matches!(probe.build_mode, WindowsPresenterBuildMode::Disabled) {
            return Self::Unavailable {
                code: FallbackReasonCode::MissingCapability,
                detail: "the Windows integrated presenter is disabled in this build",
            };
        }
        if !probe.mpv_window_id_observed {
            return Self::Unavailable {
                code: FallbackReasonCode::MissingCapability,
                detail: "mpv did not expose a live Windows window-id",
            };
        }
        if !probe.iced_overlay_handle_observed {
            return Self::Unavailable {
                code: FallbackReasonCode::MissingCapability,
                detail: "Iced did not expose a live Win32 overlay handle",
            };
        }
        if !probe.dwm_composition_available {
            return Self::Unavailable {
                code: FallbackReasonCode::UnsupportedPlatform,
                detail: "Windows DWM composition is unavailable for the transparent overlay",
            };
        }
        if !probe.production_gate_approved {
            return Self::SpikeReady;
        }
        Self::ProductionReady {
            native_hdr: probe.native_hdr_validated,
        }
    }

    /// Candidate consumed by the neutral backend selector.
    ///
    /// Spike readiness deliberately remains unavailable to Auto. It can be
    /// exercised only by the dedicated spike harness until the production
    /// decision is recorded.
    pub const fn backend_candidate(&self) -> BackendCandidate {
        match self {
            Self::ProductionReady { native_hdr } => {
                BackendCandidate::available(
                    PlaybackTarget::MPV_INTEGRATED,
                    *native_hdr,
                )
            }
            Self::Unavailable { code, .. } => BackendCandidate::unavailable(
                PlaybackTarget::MPV_INTEGRATED,
                *code,
            ),
            Self::SpikeReady => BackendCandidate::unavailable(
                PlaybackTarget::MPV_INTEGRATED,
                FallbackReasonCode::Policy,
            ),
        }
    }

    /// Deterministic transition to mpv's ordinary native window.
    pub fn fallback_reason(&self) -> Option<FallbackReason> {
        let (code, detail) = match self {
            Self::Unavailable { code, detail } => (*code, *detail),
            Self::SpikeReady => (
                FallbackReasonCode::Policy,
                "the Windows presenter is spike-only until its production gate passes",
            ),
            Self::ProductionReady { .. } => return None,
        };
        Some(FallbackReason {
            code,
            from: Some(PlaybackTarget::MPV_INTEGRATED),
            to: PlaybackTarget::MPV_NATIVE_WINDOW,
            detail: detail.to_owned(),
        })
    }
}

/// Non-null Win32 window handle kept opaque outside the target adapter.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowsHwnd(NonZeroIsize);

impl WindowsHwnd {
    /// Convert mpv's `window-id` property to a Win32 handle.
    ///
    /// mpv returns the Win32 `HWND` through an `intptr_t`, so the value must be
    /// preserved at the target pointer width. Zero and values that do not fit
    /// the target `isize` are rejected before any Win32 call; `IsWindow` then
    /// validates the opaque handle immediately before presenter readiness.
    pub fn from_mpv_window_id(
        value: i64,
    ) -> Result<Self, WindowsPresenterError> {
        let pointer =
            isize::try_from(value)
                .ok()
                .and_then(NonZeroIsize::new)
                .ok_or(WindowsPresenterError::InvalidMpvWindowId(value))?;
        Ok(Self(pointer))
    }

    /// Wrap a non-null raw `HWND` obtained from Iced.
    pub const fn from_non_zero(value: NonZeroIsize) -> Self {
        Self(value)
    }

    pub const fn get(self) -> isize {
        self.0.get()
    }
}

impl fmt::Debug for WindowsHwnd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WindowsHwnd(<redacted>)")
    }
}

/// Owned-window overlay is the preferred full-player spike relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsWindowRelationship {
    OwnedOverlay,
}

/// Lifecycle capabilities available before mpv has created its root HWND.
///
/// This lets the host-side lifecycle begin in `AwaitingVideoOutput` without
/// duplicating target policy or manufacturing an invalid native handle.
pub fn windows_presenter_capabilities(
    build_mode: WindowsPresenterBuildMode,
) -> PresenterCapabilities {
    PresenterCapabilities {
        integrated_overlay: matches!(
            build_mode,
            WindowsPresenterBuildMode::Spike
        ),
        embedded_surface: false,
        // This remains false until the overlay-visible/hidden HDR hardware
        // gate is recorded. Native-window mpv keeps its independent output
        // capability in the fallback path.
        native_hdr: false,
        fractional_scaling: true,
        native_window_fallback: true,
        fullscreen_owner: Some(FullscreenOwner::VideoOutput),
        compositor_requirement: Some(
            "Windows 10+ with DWM composition".to_owned(),
        ),
    }
}

/// Iced overlay handle borrowed by one attach operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsPresenterHost {
    pub overlay: WindowsHwnd,
}

#[cfg(all(target_os = "windows", feature = "ui"))]
impl WindowsPresenterHost {
    /// Extract a Win32 `HWND` from the event-loop-local Iced host lease.
    pub fn from_captured_iced_host(
        host: &crate::native_video_slot::CapturedIcedHost,
    ) -> Result<Self, PlaybackError> {
        use iced::window::raw_window_handle::{
            HasWindowHandle, RawWindowHandle,
        };

        let raw = host.window_handle().map_err(|error| {
            presenter_error(format!(
                "could not borrow Iced Win32 overlay handle: {error}"
            ))
        })?;
        let RawWindowHandle::Win32(handle) = raw.as_raw() else {
            return Err(presenter_error(
                "captured Iced host is not a Win32 window",
            ));
        };
        Ok(Self {
            overlay: WindowsHwnd::from_non_zero(handle.hwnd),
        })
    }
}

/// Physical video-root client rectangle used to align the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsClientRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Target observations retained by the spike harness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowsWindowSnapshot {
    pub client_rect: WindowsClientRect,
    pub dpi: u32,
    pub minimized: bool,
}

impl WindowsWindowSnapshot {
    pub fn scale_factor(self) -> f64 {
        f64::from(self.dpi) / 96.0
    }
}

/// Win32 operations isolated behind a fakeable function table.
pub trait WindowsWindowSystem {
    fn is_window(&self, window: WindowsHwnd) -> bool;
    fn owner(&self, window: WindowsHwnd) -> Option<WindowsHwnd>;
    fn set_owner(
        &mut self,
        window: WindowsHwnd,
        owner: Option<WindowsHwnd>,
    ) -> Result<(), WindowsPresenterError>;
    fn extended_style(
        &self,
        window: WindowsHwnd,
    ) -> Result<u32, WindowsPresenterError>;
    fn set_extended_style(
        &mut self,
        window: WindowsHwnd,
        style: u32,
    ) -> Result<(), WindowsPresenterError>;
    fn snapshot(
        &self,
        video_root: WindowsHwnd,
    ) -> Result<WindowsWindowSnapshot, WindowsPresenterError>;
    fn position_overlay(
        &mut self,
        overlay: WindowsHwnd,
        rect: WindowsClientRect,
    ) -> Result<(), WindowsPresenterError>;
    fn set_visible_without_activation(
        &mut self,
        overlay: WindowsHwnd,
        visible: bool,
    ) -> Result<(), WindowsPresenterError>;
    fn activate(
        &mut self,
        window: WindowsHwnd,
    ) -> Result<(), WindowsPresenterError>;
}

const WS_EX_TOOLWINDOW_VALUE: u32 = 0x0000_0080;
const WS_EX_APPWINDOW_VALUE: u32 = 0x0004_0000;

#[derive(Debug, Clone, Copy)]
struct WindowsAttachment {
    identity: PresenterIdentity,
    overlay: WindowsHwnd,
    original_owner: Option<WindowsHwnd>,
    original_extended_style: u32,
}

/// UI-thread-local Windows owned-overlay presenter.
///
/// The fullscreen callback is the intentional seam to mpv's serialized
/// control plane. The target adapter does not fake fullscreen with maximize;
/// it asks mpv and waits for the normal confirmation path.
pub struct WindowsPresenter<W, F> {
    windows: W,
    fullscreen: F,
    video_root: WindowsHwnd,
    build_mode: WindowsPresenterBuildMode,
    capabilities: PresenterCapabilities,
    attachment: Option<WindowsAttachment>,
    requested_visible: bool,
    suspended: bool,
    geometry_visible: bool,
    last_snapshot: Option<WindowsWindowSnapshot>,
    applied_visible: Option<bool>,
}

impl<W, F> fmt::Debug for WindowsPresenter<W, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsPresenter")
            .field("video_root", &self.video_root)
            .field("build_mode", &self.build_mode)
            .field("capabilities", &self.capabilities)
            .field("attachment", &self.attachment)
            .field("requested_visible", &self.requested_visible)
            .field("suspended", &self.suspended)
            .field("geometry_visible", &self.geometry_visible)
            .field("last_snapshot", &self.last_snapshot)
            .field("applied_visible", &self.applied_visible)
            .finish_non_exhaustive()
    }
}

impl<W, F> WindowsPresenter<W, F>
where
    W: WindowsWindowSystem,
    F: FnMut(bool) -> Result<(), PlaybackError>,
{
    pub fn new(
        windows: W,
        fullscreen: F,
        video_root: WindowsHwnd,
        build_mode: WindowsPresenterBuildMode,
    ) -> Self {
        Self {
            windows,
            fullscreen,
            video_root,
            build_mode,
            capabilities: windows_presenter_capabilities(build_mode),
            attachment: None,
            requested_visible: false,
            suspended: false,
            geometry_visible: false,
            last_snapshot: None,
            applied_visible: None,
        }
    }

    pub const fn relationship(&self) -> WindowsWindowRelationship {
        WindowsWindowRelationship::OwnedOverlay
    }

    pub const fn last_snapshot(&self) -> Option<WindowsWindowSnapshot> {
        self.last_snapshot
    }

    /// Explicit focus handoff used by the overlay input policy spike.
    pub fn focus_video_root(&mut self) -> Result<(), PlaybackError> {
        self.windows
            .activate(self.video_root)
            .map_err(PlaybackError::from)
    }

    fn ensure_identity(
        &self,
        identity: PresenterIdentity,
    ) -> Result<WindowsAttachment, PlaybackError> {
        match self.attachment {
            Some(attachment) if attachment.identity == identity => {
                Ok(attachment)
            }
            Some(_) => Err(presenter_error(
                "Windows presenter rejected a stale attachment generation",
            )),
            None => Err(presenter_error("Windows presenter is not attached")),
        }
    }

    fn refresh_position_and_visibility(
        &mut self,
        attachment: WindowsAttachment,
    ) -> Result<(), PlaybackError> {
        if !self.windows.is_window(self.video_root)
            || !self.windows.is_window(attachment.overlay)
        {
            return Err(presenter_error(
                "Windows presenter HWND was destroyed before synchronization",
            ));
        }
        let snapshot = self.windows.snapshot(self.video_root)?;
        if self.last_snapshot.map(|last| last.client_rect)
            != Some(snapshot.client_rect)
        {
            self.windows
                .position_overlay(attachment.overlay, snapshot.client_rect)?;
        }
        let visible = self.requested_visible
            && !self.suspended
            && self.geometry_visible
            && !snapshot.minimized;
        if self.applied_visible != Some(visible) {
            self.windows
                .set_visible_without_activation(attachment.overlay, visible)?;
            self.applied_visible = Some(visible);
        }
        self.last_snapshot = Some(snapshot);
        Ok(())
    }

    fn restore_attachment(&mut self, attachment: WindowsAttachment) {
        if !self.windows.is_window(attachment.overlay) {
            return;
        }
        let _ = self
            .windows
            .set_visible_without_activation(attachment.overlay, false);
        self.applied_visible = Some(false);
        let _ = self
            .windows
            .set_owner(attachment.overlay, attachment.original_owner);
        let _ = self.windows.set_extended_style(
            attachment.overlay,
            attachment.original_extended_style,
        );
    }
}

impl<W, F> NativePresenter for WindowsPresenter<W, F>
where
    W: WindowsWindowSystem + 'static,
    F: FnMut(bool) -> Result<(), PlaybackError> + 'static,
{
    type Host<'host>
        = WindowsPresenterHost
    where
        Self: 'host;

    fn attach(
        &mut self,
        identity: PresenterIdentity,
        host: Self::Host<'_>,
    ) -> Result<(), PlaybackError> {
        if !matches!(self.build_mode, WindowsPresenterBuildMode::Spike) {
            return Err(presenter_error(
                "Windows integrated presenter is disabled in this build",
            ));
        }
        if self.attachment.is_some() {
            return Err(presenter_error(
                "Windows presenter attach was requested more than once",
            ));
        }
        if host.overlay == self.video_root
            || !self.windows.is_window(host.overlay)
            || !self.windows.is_window(self.video_root)
        {
            return Err(presenter_error(
                "Windows presenter received invalid or identical HWNDs",
            ));
        }

        let attachment = WindowsAttachment {
            identity,
            overlay: host.overlay,
            original_owner: self.windows.owner(host.overlay),
            original_extended_style: self
                .windows
                .extended_style(host.overlay)?,
        };

        // Hide first, then establish ownership and task-switcher identity.
        self.windows
            .set_visible_without_activation(host.overlay, false)?;
        self.applied_visible = Some(false);
        self.windows
            .set_owner(host.overlay, Some(self.video_root))?;
        let overlay_style = (attachment.original_extended_style
            | WS_EX_TOOLWINDOW_VALUE)
            & !WS_EX_APPWINDOW_VALUE;
        if let Err(error) =
            self.windows.set_extended_style(host.overlay, overlay_style)
        {
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
                "Windows presenter rejected host geometry: {error}"
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
                "Windows native-root mode requires mpv to own fullscreen",
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
        self.last_snapshot = None;
        self.restore_attachment(attachment);
    }

    fn capabilities(&self) -> &PresenterCapabilities {
        &self.capabilities
    }
}

/// Target-native Win32 implementation. All calls remain on the Iced event-loop
/// thread through the presenter's non-`Send` lifecycle.
#[cfg(target_os = "windows")]
#[derive(Debug, Default)]
pub struct Win32WindowSystem;

#[cfg(target_os = "windows")]
impl Win32WindowSystem {
    /// Query the DWM composition prerequisite before advertising spike
    /// readiness. The probe is intentionally separate from construction so a
    /// failed platform check can fall back without touching either HWND.
    pub fn composition_available() -> Result<bool, WindowsPresenterError> {
        let mut enabled = 0;
        // SAFETY: DwmIsCompositionEnabled only initializes the stack BOOL.
        let result = unsafe {
            windows_sys::Win32::Graphics::Dwm::DwmIsCompositionEnabled(
                &mut enabled,
            )
        };
        if result < 0 {
            return Err(WindowsPresenterError::Win32 {
                operation: "DwmIsCompositionEnabled",
                code: result as u32,
            });
        }
        Ok(enabled != 0)
    }

    /// Validate an observed mpv HWND immediately before announcing video
    /// output readiness to the lifecycle.
    pub fn is_live(&self, window: WindowsHwnd) -> bool {
        // SAFETY: IsWindow validates the opaque value without dereferencing it.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(Self::raw(
                window,
            )) != 0
        }
    }

    fn raw(window: WindowsHwnd) -> windows_sys::Win32::Foundation::HWND {
        window.get() as windows_sys::Win32::Foundation::HWND
    }

    fn last_error(operation: &'static str) -> WindowsPresenterError {
        // SAFETY: GetLastError has no preconditions.
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        WindowsPresenterError::Win32 { operation, code }
    }
}

#[cfg(target_os = "windows")]
impl WindowsWindowSystem for Win32WindowSystem {
    fn is_window(&self, window: WindowsHwnd) -> bool {
        self.is_live(window)
    }

    fn owner(&self, window: WindowsHwnd) -> Option<WindowsHwnd> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GW_OWNER, GetWindow,
        };
        // SAFETY: the caller already validated the HWND and GetWindow borrows it.
        let owner = unsafe { GetWindow(Self::raw(window), GW_OWNER) } as isize;
        NonZeroIsize::new(owner).map(WindowsHwnd::from_non_zero)
    }

    fn set_owner(
        &mut self,
        window: WindowsHwnd,
        owner: Option<WindowsHwnd>,
    ) -> Result<(), WindowsPresenterError> {
        use windows_sys::Win32::{
            Foundation::{ERROR_SUCCESS, GetLastError, SetLastError},
            UI::WindowsAndMessaging::{GWLP_HWNDPARENT, SetWindowLongPtrW},
        };
        // SAFETY: SetWindowLongPtrW updates only the owner slot of a live HWND.
        unsafe {
            SetLastError(ERROR_SUCCESS);
            let previous = SetWindowLongPtrW(
                Self::raw(window),
                GWLP_HWNDPARENT,
                owner.map_or(0, WindowsHwnd::get),
            );
            if previous == 0 && GetLastError() != ERROR_SUCCESS {
                return Err(Self::last_error("SetWindowLongPtrW(owner)"));
            }
        }
        Ok(())
    }

    fn extended_style(
        &self,
        window: WindowsHwnd,
    ) -> Result<u32, WindowsPresenterError> {
        use windows_sys::Win32::{
            Foundation::{ERROR_SUCCESS, GetLastError, SetLastError},
            UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW},
        };
        // SAFETY: reads the style word of a validated HWND.
        let style = unsafe {
            SetLastError(ERROR_SUCCESS);
            let style = GetWindowLongPtrW(Self::raw(window), GWL_EXSTYLE);
            if style == 0 && GetLastError() != ERROR_SUCCESS {
                return Err(Self::last_error("GetWindowLongPtrW(exstyle)"));
            }
            style
        };
        Ok(style as u32)
    }

    fn set_extended_style(
        &mut self,
        window: WindowsHwnd,
        style: u32,
    ) -> Result<(), WindowsPresenterError> {
        use windows_sys::Win32::{
            Foundation::{ERROR_SUCCESS, GetLastError, SetLastError},
            UI::WindowsAndMessaging::{GWL_EXSTYLE, SetWindowLongPtrW},
        };
        // SAFETY: updates the style word of a validated HWND.
        unsafe {
            SetLastError(ERROR_SUCCESS);
            let previous = SetWindowLongPtrW(
                Self::raw(window),
                GWL_EXSTYLE,
                style as isize,
            );
            if previous == 0 && GetLastError() != ERROR_SUCCESS {
                return Err(Self::last_error("SetWindowLongPtrW(exstyle)"));
            }
        }
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SetWindowPos,
        };
        // Extended style data may be cached. Force the non-client/task-switcher
        // state to observe the new APPWINDOW/TOOLWINDOW bits without moving or
        // activating the overlay.
        // SAFETY: updates only cached frame state for the same validated HWND.
        if unsafe {
            SetWindowPos(
                Self::raw(window),
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED
                    | SWP_NOACTIVATE
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER,
            )
        } == 0
        {
            return Err(Self::last_error("SetWindowPos(frame change)"));
        }
        Ok(())
    }

    fn snapshot(
        &self,
        video_root: WindowsHwnd,
    ) -> Result<WindowsWindowSnapshot, WindowsPresenterError> {
        use windows_sys::Win32::{
            Foundation::{POINT, RECT},
            Graphics::Gdi::ClientToScreen,
            UI::{
                HiDpi::GetDpiForWindow,
                WindowsAndMessaging::{GetClientRect, IsIconic},
            },
        };
        let hwnd = Self::raw(video_root);
        let mut rect = RECT::default();
        // SAFETY: all pointers refer to initialized stack values for the call.
        if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
            return Err(Self::last_error("GetClientRect"));
        }
        let mut top_left = POINT {
            x: rect.left,
            y: rect.top,
        };
        let mut bottom_right = POINT {
            x: rect.right,
            y: rect.bottom,
        };
        // SAFETY: same live HWND and stack values as above.
        if unsafe { ClientToScreen(hwnd, &mut top_left) } == 0
            || unsafe { ClientToScreen(hwnd, &mut bottom_right) } == 0
        {
            return Err(Self::last_error("ClientToScreen"));
        }
        // SAFETY: GetDpiForWindow and IsIconic only inspect the live HWND.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            return Err(Self::last_error("GetDpiForWindow"));
        }
        Ok(WindowsWindowSnapshot {
            client_rect: WindowsClientRect {
                x: top_left.x,
                y: top_left.y,
                width: bottom_right.x.saturating_sub(top_left.x),
                height: bottom_right.y.saturating_sub(top_left.y),
            },
            dpi,
            // SAFETY: same inspection-only contract.
            minimized: unsafe { IsIconic(hwnd) != 0 },
        })
    }

    fn position_overlay(
        &mut self,
        overlay: WindowsHwnd,
        rect: WindowsClientRect,
    ) -> Result<(), WindowsPresenterError> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            HWND_TOP, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SetWindowPos,
        };
        // SAFETY: moves a validated overlay using value-only coordinates.
        if unsafe {
            SetWindowPos(
                Self::raw(overlay),
                HWND_TOP,
                rect.x,
                rect.y,
                rect.width.max(0),
                rect.height.max(0),
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        } == 0
        {
            return Err(Self::last_error("SetWindowPos"));
        }
        Ok(())
    }

    fn set_visible_without_activation(
        &mut self,
        overlay: WindowsHwnd,
        visible: bool,
    ) -> Result<(), WindowsPresenterError> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow,
        };
        // ShowWindow's return value is previous visibility, not success.
        // SAFETY: the caller validates and owns the overlay lifecycle.
        unsafe {
            ShowWindow(
                Self::raw(overlay),
                if visible { SW_SHOWNOACTIVATE } else { SW_HIDE },
            );
        }
        Ok(())
    }

    fn activate(
        &mut self,
        window: WindowsHwnd,
    ) -> Result<(), WindowsPresenterError> {
        use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        // SAFETY: activates a validated top-level HWND.
        if unsafe { SetForegroundWindow(Self::raw(window)) } == 0 {
            return Err(Self::last_error("SetForegroundWindow"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WindowsPresenterError {
    #[error("invalid {WINDOWS_PRESENTER_BUILD_ENV} value: {0}")]
    InvalidBuildMode(String),
    #[error("mpv returned an invalid Windows window-id: {0}")]
    InvalidMpvWindowId(i64),
    #[error("Win32 {operation} failed with error {code}")]
    Win32 { operation: &'static str, code: u32 },
    #[error("Windows presenter operation failed: {0}")]
    Operation(String),
}

impl From<WindowsPresenterError> for PlaybackError {
    fn from(error: WindowsPresenterError) -> Self {
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

    use crate::{
        contract::{GeometryRevision, LogicalRect, SessionGeneration},
        presenter::{FullscreenOwner, NativePresenter, PresenterGeneration},
    };

    use super::*;

    fn hwnd(value: isize) -> WindowsHwnd {
        WindowsHwnd::from_non_zero(NonZeroIsize::new(value).unwrap())
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
            LogicalRect::new(0.0, 0.0, 1920.0, 1080.0),
            visible.then(|| LogicalRect::new(0.0, 0.0, 1920.0, 1080.0)),
            1.5,
        )
    }

    #[derive(Debug, Clone)]
    struct WindowState {
        live: bool,
        owner: Option<WindowsHwnd>,
        style: u32,
        visible: bool,
    }

    #[derive(Debug, Clone)]
    struct FakeWindows {
        state: Rc<RefCell<HashMap<WindowsHwnd, WindowState>>>,
        operations: Rc<RefCell<Vec<String>>>,
        snapshot: Rc<RefCell<WindowsWindowSnapshot>>,
    }

    impl FakeWindows {
        fn new(video: WindowsHwnd, overlay: WindowsHwnd) -> Self {
            let state = HashMap::from([
                (
                    video,
                    WindowState {
                        live: true,
                        owner: None,
                        style: 0,
                        visible: true,
                    },
                ),
                (
                    overlay,
                    WindowState {
                        live: true,
                        owner: None,
                        style: WS_EX_APPWINDOW_VALUE,
                        visible: false,
                    },
                ),
            ]);
            Self {
                state: Rc::new(RefCell::new(state)),
                operations: Rc::new(RefCell::new(Vec::new())),
                snapshot: Rc::new(RefCell::new(WindowsWindowSnapshot {
                    client_rect: WindowsClientRect {
                        x: 10,
                        y: 20,
                        width: 1280,
                        height: 720,
                    },
                    dpi: 144,
                    minimized: false,
                })),
            }
        }

        fn operation(&self, value: impl Into<String>) {
            self.operations.borrow_mut().push(value.into());
        }
    }

    impl WindowsWindowSystem for FakeWindows {
        fn is_window(&self, window: WindowsHwnd) -> bool {
            self.state
                .borrow()
                .get(&window)
                .is_some_and(|state| state.live)
        }

        fn owner(&self, window: WindowsHwnd) -> Option<WindowsHwnd> {
            self.state
                .borrow()
                .get(&window)
                .and_then(|state| state.owner)
        }

        fn set_owner(
            &mut self,
            window: WindowsHwnd,
            owner: Option<WindowsHwnd>,
        ) -> Result<(), WindowsPresenterError> {
            self.operation(format!("owner:{:?}", owner.map(WindowsHwnd::get)));
            self.state.borrow_mut().get_mut(&window).unwrap().owner = owner;
            Ok(())
        }

        fn extended_style(
            &self,
            window: WindowsHwnd,
        ) -> Result<u32, WindowsPresenterError> {
            Ok(self.state.borrow()[&window].style)
        }

        fn set_extended_style(
            &mut self,
            window: WindowsHwnd,
            style: u32,
        ) -> Result<(), WindowsPresenterError> {
            self.operation(format!("style:{style:#x}"));
            self.state.borrow_mut().get_mut(&window).unwrap().style = style;
            Ok(())
        }

        fn snapshot(
            &self,
            _video_root: WindowsHwnd,
        ) -> Result<WindowsWindowSnapshot, WindowsPresenterError> {
            Ok(*self.snapshot.borrow())
        }

        fn position_overlay(
            &mut self,
            _overlay: WindowsHwnd,
            rect: WindowsClientRect,
        ) -> Result<(), WindowsPresenterError> {
            self.operation(format!(
                "position:{},{},{},{}",
                rect.x, rect.y, rect.width, rect.height
            ));
            Ok(())
        }

        fn set_visible_without_activation(
            &mut self,
            overlay: WindowsHwnd,
            visible: bool,
        ) -> Result<(), WindowsPresenterError> {
            self.operation(format!("visible:{visible}"));
            self.state.borrow_mut().get_mut(&overlay).unwrap().visible =
                visible;
            Ok(())
        }

        fn activate(
            &mut self,
            window: WindowsHwnd,
        ) -> Result<(), WindowsPresenterError> {
            self.operation(format!("activate:{}", window.get()));
            Ok(())
        }
    }

    #[test]
    fn build_mode_and_mpv_window_id_fail_closed() {
        assert_eq!(
            WindowsPresenterBuildMode::parse(None).unwrap(),
            WindowsPresenterBuildMode::Disabled
        );
        assert_eq!(
            WindowsPresenterBuildMode::parse(Some("spike")).unwrap(),
            WindowsPresenterBuildMode::Spike
        );
        assert!(WindowsPresenterBuildMode::parse(Some("production")).is_err());
        assert!(WindowsHwnd::from_mpv_window_id(0).is_err());
        assert_eq!(WindowsHwnd::from_mpv_window_id(42).unwrap().get(), 42);
        #[cfg(target_pointer_width = "64")]
        {
            let wide = i64::from(u32::MAX) + 1;
            assert_eq!(
                WindowsHwnd::from_mpv_window_id(wide).unwrap().get(),
                wide as isize
            );
        }
    }

    #[test]
    fn capability_requires_each_probe_and_keeps_spike_out_of_auto() {
        let probe = WindowsPresenterProbe {
            build_mode: WindowsPresenterBuildMode::Spike,
            mpv_window_id_observed: true,
            iced_overlay_handle_observed: true,
            dwm_composition_available: true,
            production_gate_approved: false,
            native_hdr_validated: false,
        };
        let availability = WindowsPresenterAvailability::evaluate(probe);
        assert_eq!(availability, WindowsPresenterAvailability::SpikeReady);
        assert!(!availability.backend_candidate().available);
        let fallback = availability.fallback_reason().unwrap();
        assert_eq!(fallback.code, FallbackReasonCode::Policy);
        assert_eq!(fallback.to, PlaybackTarget::MPV_NATIVE_WINDOW);

        let production =
            WindowsPresenterAvailability::evaluate(WindowsPresenterProbe {
                production_gate_approved: true,
                native_hdr_validated: true,
                ..probe
            });
        assert_eq!(
            production.backend_candidate(),
            BackendCandidate::available(PlaybackTarget::MPV_INTEGRATED, true)
        );
        assert!(production.fallback_reason().is_none());
    }

    #[test]
    fn owned_overlay_attaches_hidden_positions_and_restores_on_detach() {
        let video = hwnd(10);
        let overlay = hwnd(20);
        let windows = FakeWindows::new(video, overlay);
        let observed = windows.clone();
        let fullscreen_values = Rc::new(RefCell::new(Vec::new()));
        let fullscreen_values_for_callback = Rc::clone(&fullscreen_values);
        let mut presenter = WindowsPresenter::new(
            windows,
            move |fullscreen| {
                fullscreen_values_for_callback.borrow_mut().push(fullscreen);
                Ok(())
            },
            video,
            WindowsPresenterBuildMode::Spike,
        );
        let id = identity(1);
        presenter
            .attach(id, WindowsPresenterHost { overlay })
            .unwrap();

        let state = observed.state.borrow();
        assert_eq!(state[&overlay].owner, Some(video));
        assert_eq!(state[&overlay].style, WS_EX_TOOLWINDOW_VALUE);
        assert!(!state[&overlay].visible);
        drop(state);

        presenter.synchronize(id, geometry(true)).unwrap();
        presenter.set_visible(id, true).unwrap();
        assert!(observed.state.borrow()[&overlay].visible);
        assert_eq!(presenter.last_snapshot().unwrap().dpi, 144);
        assert_eq!(presenter.last_snapshot().unwrap().scale_factor(), 1.5);
        let operation_count = observed.operations.borrow().len();
        presenter.synchronize(id, geometry(true)).unwrap();
        assert_eq!(observed.operations.borrow().len(), operation_count);
        presenter
            .set_fullscreen(id, FullscreenOwner::VideoOutput, true)
            .unwrap();
        assert_eq!(&*fullscreen_values.borrow(), &[true]);

        presenter.detach(id);
        let state = observed.state.borrow();
        assert_eq!(state[&overlay].owner, None);
        assert_eq!(state[&overlay].style, WS_EX_APPWINDOW_VALUE);
        assert!(!state[&overlay].visible);
        let operations = observed.operations.borrow();
        let owner_index = operations
            .iter()
            .position(|operation| operation == "owner:Some(10)")
            .unwrap();
        let first_show_index = operations
            .iter()
            .position(|operation| operation == "visible:true")
            .unwrap();
        assert!(owner_index < first_show_index);
    }

    #[test]
    fn minimize_and_stale_detach_do_not_break_live_attachment() {
        let video = hwnd(30);
        let overlay = hwnd(40);
        let windows = FakeWindows::new(video, overlay);
        let observed = windows.clone();
        let mut presenter = WindowsPresenter::new(
            windows,
            |_| Ok(()),
            video,
            WindowsPresenterBuildMode::Spike,
        );
        let id = identity(2);
        presenter
            .attach(id, WindowsPresenterHost { overlay })
            .unwrap();
        presenter.synchronize(id, geometry(true)).unwrap();
        presenter.set_visible(id, true).unwrap();
        let previous_snapshot = *observed.snapshot.borrow();
        *observed.snapshot.borrow_mut() = WindowsWindowSnapshot {
            minimized: true,
            ..previous_snapshot
        };
        presenter.synchronize(id, geometry(true)).unwrap();
        assert!(!observed.state.borrow()[&overlay].visible);

        presenter.detach(identity(99));
        assert_eq!(observed.state.borrow()[&overlay].owner, Some(video));
        presenter.detach(id);
        assert_eq!(observed.state.borrow()[&overlay].owner, None);
    }
}
