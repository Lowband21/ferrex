//! Renderer-neutral Iced slot for platform-native video presentation.
//!
//! [`crate::native_video_slot::NativeVideoSlot`] reserves layout but never turns decoded video into an
//! Iced primitive. On redraw it reports generation-scoped host geometry to a
//! UI-thread-local callback. Raw host handles are captured through
//! [`iced::window::run`] and retained in a thread-local registry, so Wayland,
//! AppKit, and other event-loop-local resources do not acquire a `Send` bound.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt,
    rc::{Rc, Weak},
};

use iced::window::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle, WindowHandle,
};
use iced::{
    Background, Color, Element, Event, Length, Rectangle, Size, Task,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
        widget::{Tree, tree},
    },
    window::{self, Window as IcedWindow},
};

use crate::{
    contract::{PlaybackError, PlaybackErrorKind},
    presenter::{
        GeometryRevision, LogicalRect, PresenterIdentity, PresenterInput,
        PresenterInputEnvelope, SurfaceGeometry,
    },
};

thread_local! {
    /// Raw handles are deliberately confined to the Iced event-loop thread.
    static ICED_NATIVE_HOSTS: RefCell<HashMap<window::Id, Rc<CapturedIcedHost>>> =
        RefCell::new(HashMap::new());
    /// Weak registrations let explicit window-close handling detach every slot
    /// before releasing the corresponding raw host lease. Cloning a slot
    /// handle does not create duplicate registrations or keep it alive.
    static ICED_NATIVE_VIDEO_SLOTS: RefCell<
        HashMap<window::Id, Vec<Weak<NativeVideoSlotHandleInner>>>,
    > = RefCell::new(HashMap::new());
}

/// Desktop window system represented by a captured Iced host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeHostPlatform {
    Wayland,
    X11,
    Windows,
    MacOs,
    Other,
}

/// Failure to copy a valid raw host handle from Iced's current window.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NativeHostCaptureError {
    #[error("could not acquire Iced {kind} handle: {detail}")]
    HandleUnavailable { kind: &'static str, detail: String },
    #[error(
        "Iced returned incompatible native handles (window={window}, display={display})"
    )]
    IncompatiblePair {
        window: &'static str,
        display: &'static str,
    },
}

/// Borrow-only native host captured from an Iced window.
///
/// Values of this type live exclusively in a thread-local registry. Callers can
/// only borrow one through [`with_captured_iced_host`], and must release it
/// before the corresponding native window is destroyed. The raw handle types
/// intentionally keep this value from becoming `Send`.
pub struct CapturedIcedHost {
    window_id: window::Id,
    platform: NativeHostPlatform,
    raw_window: RawWindowHandle,
    raw_display: RawDisplayHandle,
}

impl CapturedIcedHost {
    pub const fn window_id(&self) -> window::Id {
        self.window_id
    }

    pub const fn platform(&self) -> NativeHostPlatform {
        self.platform
    }
}

impl fmt::Debug for CapturedIcedHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedIcedHost")
            .field("window_id", &self.window_id)
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

impl HasWindowHandle for CapturedIcedHost {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: capture copies a handle while Iced owns a live window. The
        // registry only lends this wrapper and explicit close handling removes
        // it before platform teardown.
        Ok(unsafe { WindowHandle::borrow_raw(self.raw_window) })
    }
}

impl HasDisplayHandle for CapturedIcedHost {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: identical lifetime argument to `window_handle` above; the
        // display handle remains tied to the borrowed registry entry.
        Ok(unsafe { DisplayHandle::borrow_raw(self.raw_display) })
    }
}

/// Capture raw handles for `window_id` on Iced's native event-loop thread.
///
/// The task output is intentionally pointer-free and therefore can cross the
/// normal Iced task channel. The copied raw handles themselves never leave the
/// thread-local registry.
pub fn capture_iced_native_host(
    window_id: window::Id,
) -> Task<Result<NativeHostPlatform, NativeHostCaptureError>> {
    window::run(window_id, move |window| {
        capture_iced_native_host_from_window(window_id, window)
    })
}

/// Whether the current event-loop thread has a captured host for `window_id`.
pub fn has_captured_iced_host(window_id: window::Id) -> bool {
    ICED_NATIVE_HOSTS.with(|hosts| hosts.borrow().contains_key(&window_id))
}

/// Borrow a captured native host without allowing it to escape the callback.
///
/// Platform presenter code should call this only from Iced's event-loop thread.
pub fn with_captured_iced_host<T>(
    window_id: window::Id,
    callback: impl FnOnce(&CapturedIcedHost) -> T,
) -> Option<T> {
    let host =
        ICED_NATIVE_HOSTS.with(|hosts| hosts.borrow().get(&window_id).cloned());
    host.map(|host| callback(&host))
}

/// Remove a captured host after presenter detach and before window destruction.
pub fn release_captured_iced_host(window_id: window::Id) -> bool {
    ICED_NATIVE_HOSTS
        .with(|hosts| hosts.borrow_mut().remove(&window_id).is_some())
}

/// Result of preparing one Iced host for deterministic native teardown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeHostCloseResult {
    /// Number of still-live slot generations explicitly detached.
    pub detached_slots: usize,
    /// Whether a captured raw host lease was released.
    pub released_host: bool,
}

/// Detach every native video slot registered for `window_id`.
///
/// This is the explicit pre-destruction path used by the daemon window
/// manager. Ordinary Iced widget-tree churn is not a host-lifetime signal and
/// deliberately leaves these registrations attached.
pub fn detach_native_video_slots(window_id: window::Id) -> usize {
    let slots = ICED_NATIVE_VIDEO_SLOTS.with(|slots| {
        slots.borrow_mut().remove(&window_id).unwrap_or_default()
    });

    slots
        .into_iter()
        .filter_map(|slot| slot.upgrade())
        .filter(|slot| {
            let handle = NativeVideoSlotHandle {
                inner: Rc::clone(slot),
            };
            if handle.is_detached() {
                false
            } else {
                let _ = handle.detach();
                true
            }
        })
        .count()
}

/// Detach native presentation and then release the raw Iced host lease.
///
/// Call this before issuing `window::close`; the ordering is intentional so no
/// presenter callback can retain or use a native handle after host teardown.
pub fn prepare_iced_native_host_close(
    window_id: window::Id,
) -> NativeHostCloseResult {
    let detached_slots = detach_native_video_slots(window_id);
    let released_host = release_captured_iced_host(window_id);
    NativeHostCloseResult {
        detached_slots,
        released_host,
    }
}

fn capture_iced_native_host_from_window(
    window_id: window::Id,
    window: &dyn IcedWindow,
) -> Result<NativeHostPlatform, NativeHostCaptureError> {
    let raw_window = window
        .window_handle()
        .map(|handle| handle.as_raw())
        .map_err(|error| NativeHostCaptureError::HandleUnavailable {
            kind: "window",
            detail: error.to_string(),
        })?;
    let raw_display = window
        .display_handle()
        .map(|handle| handle.as_raw())
        .map_err(|error| NativeHostCaptureError::HandleUnavailable {
            kind: "display",
            detail: error.to_string(),
        })?;
    let platform = native_host_platform(raw_window, raw_display)?;

    ICED_NATIVE_HOSTS.with(|hosts| {
        hosts.borrow_mut().insert(
            window_id,
            Rc::new(CapturedIcedHost {
                window_id,
                platform,
                raw_window,
                raw_display,
            }),
        );
    });
    log::debug!(
        "native presenter host capture completed: registered=true platform={platform:?}"
    );

    Ok(platform)
}

fn native_host_platform(
    window: RawWindowHandle,
    display: RawDisplayHandle,
) -> Result<NativeHostPlatform, NativeHostCaptureError> {
    match (window, display) {
        (RawWindowHandle::Wayland(_), RawDisplayHandle::Wayland(_)) => {
            Ok(NativeHostPlatform::Wayland)
        }
        (RawWindowHandle::Xlib(_), RawDisplayHandle::Xlib(_))
        | (RawWindowHandle::Xcb(_), RawDisplayHandle::Xcb(_)) => {
            Ok(NativeHostPlatform::X11)
        }
        (RawWindowHandle::Win32(_), RawDisplayHandle::Windows(_)) => {
            Ok(NativeHostPlatform::Windows)
        }
        (RawWindowHandle::AppKit(_), RawDisplayHandle::AppKit(_)) => {
            Ok(NativeHostPlatform::MacOs)
        }
        (window @ RawWindowHandle::Wayland(_), display)
        | (window @ RawWindowHandle::Xlib(_), display)
        | (window @ RawWindowHandle::Xcb(_), display)
        | (window @ RawWindowHandle::Win32(_), display)
        | (window @ RawWindowHandle::AppKit(_), display) => {
            Err(NativeHostCaptureError::IncompatiblePair {
                window: raw_window_label(window),
                display: raw_display_label(display),
            })
        }
        _ => Ok(NativeHostPlatform::Other),
    }
}

fn raw_window_label(handle: RawWindowHandle) -> &'static str {
    match handle {
        RawWindowHandle::Wayland(_) => "wayland",
        RawWindowHandle::Xlib(_) => "xlib",
        RawWindowHandle::Xcb(_) => "xcb",
        RawWindowHandle::Win32(_) => "win32",
        RawWindowHandle::AppKit(_) => "appkit",
        _ => "other",
    }
}

fn raw_display_label(handle: RawDisplayHandle) -> &'static str {
    match handle {
        RawDisplayHandle::Wayland(_) => "wayland",
        RawDisplayHandle::Xlib(_) => "xlib",
        RawDisplayHandle::Xcb(_) => "xcb",
        RawDisplayHandle::Windows(_) => "windows",
        RawDisplayHandle::AppKit(_) => "appkit",
        _ => "other",
    }
}

/// Host work requested after one slot lifecycle input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeVideoSlotDirective {
    request_redraw: bool,
    request_host_capture: bool,
    request_snapshot_sync: bool,
}

impl NativeVideoSlotDirective {
    pub const IDLE: Self = Self {
        request_redraw: false,
        request_host_capture: false,
        request_snapshot_sync: false,
    };
    pub const REDRAW: Self = Self {
        request_redraw: true,
        request_host_capture: false,
        request_snapshot_sync: false,
    };
    pub const CAPTURE_HOST: Self = Self {
        request_redraw: false,
        request_host_capture: true,
        request_snapshot_sync: false,
    };

    pub const fn new(request_redraw: bool, request_host_capture: bool) -> Self {
        Self {
            request_redraw,
            request_host_capture,
            request_snapshot_sync: false,
        }
    }

    /// Request an application update that drains presenter events into the
    /// backend-neutral playback snapshot.
    pub const fn with_snapshot_sync(mut self) -> Self {
        self.request_snapshot_sync = true;
        self
    }

    pub const fn requests_redraw(self) -> bool {
        self.request_redraw
    }

    pub const fn requests_host_capture(self) -> bool {
        self.request_host_capture
    }

    pub const fn requests_snapshot_sync(self) -> bool {
        self.request_snapshot_sync
    }

    pub const fn merge(self, other: Self) -> Self {
        Self {
            request_redraw: self.request_redraw || other.request_redraw,
            request_host_capture: self.request_host_capture
                || other.request_host_capture,
            request_snapshot_sync: self.request_snapshot_sync
                || other.request_snapshot_sync,
        }
    }
}

type SlotCallback = dyn for<'host> Fn(
    Option<&'host CapturedIcedHost>,
    PresenterInputEnvelope,
) -> NativeVideoSlotDirective;

struct NativeVideoSlotHandleInner {
    window_id: window::Id,
    identity: PresenterIdentity,
    callback: Box<SlotCallback>,
    detached: Cell<bool>,
}

impl Drop for NativeVideoSlotHandleInner {
    fn drop(&mut self) {
        // Do not retain one dead weak registration per playback generation.
        // `try_with` also makes event-loop thread shutdown harmless.
        let _ = ICED_NATIVE_VIDEO_SLOTS.try_with(|slots| {
            let mut slots = slots.borrow_mut();
            let remove_window =
                slots.get_mut(&self.window_id).is_some_and(|registered| {
                    registered.retain(|slot| slot.strong_count() > 0);
                    registered.is_empty()
                });
            if remove_window {
                slots.remove(&self.window_id);
            }
        });
    }
}

/// Cloneable UI-thread-local bridge between a slot and presenter lifecycle.
///
/// The callback receives a borrowed host when one has been captured. Inputs
/// such as `VideoOutputReady` may legitimately arrive before that capture and
/// therefore receive `None`; `HostReady` emitted by the widget always carries a
/// host. A handle is single-generation and cannot be reused after detach.
#[derive(Clone)]
pub struct NativeVideoSlotHandle {
    inner: Rc<NativeVideoSlotHandleInner>,
}

impl NativeVideoSlotHandle {
    pub fn new<F>(
        window_id: window::Id,
        identity: PresenterIdentity,
        callback: F,
    ) -> Self
    where
        F: for<'host> Fn(
                Option<&'host CapturedIcedHost>,
                PresenterInputEnvelope,
            ) -> NativeVideoSlotDirective
            + 'static,
    {
        let inner = Rc::new(NativeVideoSlotHandleInner {
            window_id,
            identity,
            callback: Box::new(callback),
            detached: Cell::new(false),
        });
        ICED_NATIVE_VIDEO_SLOTS.with(|slots| {
            slots
                .borrow_mut()
                .entry(window_id)
                .or_default()
                .push(Rc::downgrade(&inner));
        });
        Self { inner }
    }

    pub fn window_id(&self) -> window::Id {
        self.inner.window_id
    }

    pub fn identity(&self) -> PresenterIdentity {
        self.inner.identity
    }

    pub fn host_is_captured(&self) -> bool {
        has_captured_iced_host(self.window_id())
    }

    pub fn is_detached(&self) -> bool {
        self.inner.detached.get()
    }

    /// Deliver an input on the UI thread, borrowing the captured host if it is
    /// already available.
    pub fn notify(&self, input: PresenterInput) -> NativeVideoSlotDirective {
        if self.is_detached() {
            return NativeVideoSlotDirective::IDLE;
        }
        self.dispatch(input)
    }

    /// Explicitly detach before the native host window closes.
    pub fn detach(&self) -> NativeVideoSlotDirective {
        if self.inner.detached.replace(true) {
            return NativeVideoSlotDirective::IDLE;
        }
        self.dispatch(PresenterInput::Detach)
    }

    fn dispatch(&self, input: PresenterInput) -> NativeVideoSlotDirective {
        let mut envelope =
            Some(PresenterInputEnvelope::new(self.identity(), input));
        let with_host = with_captured_iced_host(self.window_id(), |host| {
            (self.inner.callback)(
                Some(host),
                envelope.take().expect("envelope"),
            )
        });

        with_host.unwrap_or_else(|| {
            (self.inner.callback)(
                None,
                envelope.take().expect("envelope not consumed without host"),
            )
        })
    }

    fn same_instance(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for NativeVideoSlotHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeVideoSlotHandle")
            .field("window_id", &self.window_id())
            .field("identity", &self.identity())
            .field("detached", &self.is_detached())
            .finish_non_exhaustive()
    }
}

/// Non-video fallback painted by the slot's generic renderer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum NativeVideoSlotAppearance {
    /// Preserve alpha once native presentation is attached.
    Transparent,
    /// Opaque neutral loading plate; controls may be stacked above it.
    #[default]
    Loading,
    /// Distinct failure plate; error text may be stacked above it.
    Failed,
    /// Caller-selected solid fallback color.
    Solid(Color),
}

impl NativeVideoSlotAppearance {
    fn background(self) -> Option<Background> {
        match self {
            Self::Transparent => None,
            Self::Loading => Some(Color::BLACK.into()),
            Self::Failed => Some(Color::from_rgb(0.18, 0.03, 0.03).into()),
            Self::Solid(color) => Some(color.into()),
        }
    }
}

/// An axis-aligned Iced layout slot for independently rendered native video.
///
/// The widget is renderer-generic and records no decoded image or custom wgpu
/// primitive. `on_host_capture` should map to an application message that runs
/// [`capture_iced_native_host`]. Processing that task causes a normal Iced
/// update/redraw, at which point the first `HostReady` input is delivered.
pub struct NativeVideoSlot<'a, Message> {
    handle: NativeVideoSlotHandle,
    width: Length,
    height: Length,
    scale_factor: Option<f64>,
    appearance: NativeVideoSlotAppearance,
    on_host_capture: Box<dyn Fn(window::Id) -> Message + 'a>,
    on_presenter_update: Option<Box<dyn Fn() -> Message + 'a>>,
}

impl<'a, Message> NativeVideoSlot<'a, Message> {
    pub fn new(
        handle: NativeVideoSlotHandle,
        on_host_capture: impl Fn(window::Id) -> Message + 'a,
    ) -> Self {
        Self {
            handle,
            width: Length::Fill,
            height: Length::Fill,
            scale_factor: None,
            appearance: NativeVideoSlotAppearance::default(),
            on_host_capture: Box::new(on_host_capture),
            on_presenter_update: None,
        }
    }

    /// Publish an application message whenever the presenter callback queues
    /// snapshot-visible state, geometry, fullscreen, or fallback events.
    pub fn on_presenter_update(
        mut self,
        message: impl Fn() -> Message + 'a,
    ) -> Self {
        self.on_presenter_update = Some(Box::new(message));
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Override the scale observed from `Window::Rescaled` (useful when the
    /// host applies an additional application-level logical scale).
    pub fn scale_factor(mut self, scale_factor: f64) -> Self {
        self.scale_factor = Some(scale_factor);
        self
    }

    pub fn appearance(mut self, appearance: NativeVideoSlotAppearance) -> Self {
        self.appearance = appearance;
        self
    }
}

impl<Message> fmt::Debug for NativeVideoSlot<'_, Message> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeVideoSlot")
            .field("handle", &self.handle)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("scale_factor", &self.scale_factor)
            .field("appearance", &self.appearance)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GeometryFingerprint {
    logical_bounds: LogicalRect,
    visible_bounds: Option<LogicalRect>,
    scale_factor: f64,
}

#[derive(Debug)]
struct NativeVideoSlotState {
    handle: NativeVideoSlotHandle,
    host_announced: bool,
    capture_requested: bool,
    last_fingerprint: Option<GeometryFingerprint>,
    last_revision: Option<GeometryRevision>,
    observed_scale_factor: f64,
}

impl NativeVideoSlotState {
    fn new(handle: NativeVideoSlotHandle) -> Self {
        Self {
            handle,
            host_announced: false,
            capture_requested: false,
            last_fingerprint: None,
            last_revision: None,
            observed_scale_factor: 1.0,
        }
    }

    fn redraw(
        &mut self,
        bounds: Rectangle,
        viewport: Rectangle,
        configured_scale_factor: Option<f64>,
    ) -> NativeVideoSlotDirective {
        if self.handle.is_detached() {
            return NativeVideoSlotDirective::IDLE;
        }
        if !self.handle.host_is_captured() {
            return NativeVideoSlotDirective::CAPTURE_HOST;
        }

        if self.capture_requested {
            log::debug!(
                "native presenter post-capture redraw observed: host_captured=true"
            );
        }
        self.capture_requested = false;
        let fingerprint = geometry_fingerprint(
            bounds,
            viewport,
            configured_scale_factor.unwrap_or(self.observed_scale_factor),
        );
        if self.last_fingerprint == Some(fingerprint) {
            return NativeVideoSlotDirective::IDLE;
        }

        let revision = match self.last_revision {
            None => GeometryRevision::INITIAL,
            Some(revision) => match revision.next() {
                Some(revision) => revision,
                None => {
                    let error = PlaybackError::new(
                        PlaybackErrorKind::Presenter,
                        "native video slot geometry revision exhausted",
                    );
                    return self.handle.notify(PresenterInput::Failed(error));
                }
            },
        };
        self.last_fingerprint = Some(fingerprint);
        self.last_revision = Some(revision);

        let geometry = SurfaceGeometry::new(
            revision,
            fingerprint.logical_bounds,
            fingerprint.visible_bounds,
            fingerprint.scale_factor,
        );
        let input = if self.host_announced {
            PresenterInput::GeometryChanged(geometry)
        } else {
            self.host_announced = true;
            log::debug!(
                "native presenter host readiness transition: host_ready=true"
            );
            PresenterInput::HostReady { geometry }
        };
        self.handle.notify(input)
    }

    fn update_scale_factor(&mut self, scale_factor: f32) -> bool {
        let scale_factor = f64::from(scale_factor);
        if !scale_factor.is_finite()
            || scale_factor <= 0.0
            || self.observed_scale_factor == scale_factor
        {
            return false;
        }
        self.observed_scale_factor = scale_factor;
        true
    }

    fn detach(&mut self) {
        let _ = self.handle.detach();
    }
}

// Do not detach from `Drop`: Iced may discard and rebuild widget state while
// the same native host and presenter generation remain live. Real replacement
// and native-window close paths call `detach` explicitly.
fn geometry_fingerprint(
    bounds: Rectangle,
    viewport: Rectangle,
    scale_factor: f64,
) -> GeometryFingerprint {
    let logical_bounds = logical_rect(bounds);
    let visible_bounds = (bounds.width > 0.0 && bounds.height > 0.0)
        .then(|| bounds.intersection(&viewport))
        .flatten()
        .filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
        .map(logical_rect);

    GeometryFingerprint {
        logical_bounds,
        visible_bounds,
        scale_factor,
    }
}

fn logical_rect(bounds: Rectangle) -> LogicalRect {
    LogicalRect::new(
        f64::from(bounds.x),
        f64::from(bounds.y),
        f64::from(bounds.width),
        f64::from(bounds.height),
    )
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for NativeVideoSlot<'_, Message>
where
    Renderer: iced::advanced::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(background) = self.appearance.background() else {
            return;
        };
        let Some(bounds) = layout.bounds().intersection(viewport) else {
            return;
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            background,
        );
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<NativeVideoSlotState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(NativeVideoSlotState::new(self.handle.clone()))
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<NativeVideoSlotState>();
        if !state.handle.same_instance(&self.handle) {
            state.detach();
            *state = NativeVideoSlotState::new(self.handle.clone());
        }
        tree.children.clear();
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<NativeVideoSlotState>();

        match event {
            Event::Window(window::Event::RedrawRequested(_)) => {
                let directive =
                    state.redraw(layout.bounds(), *viewport, self.scale_factor);
                if directive.requests_host_capture() && !state.capture_requested
                {
                    state.capture_requested = true;
                    log::debug!(
                        "native presenter host capture requested: host_captured=false"
                    );
                    shell.publish((self.on_host_capture)(
                        self.handle.window_id(),
                    ));
                }
                if directive.requests_redraw() {
                    shell.request_redraw();
                }
                if directive.requests_snapshot_sync()
                    && let Some(message) = self.on_presenter_update.as_ref()
                {
                    shell.publish(message());
                }
            }
            Event::Window(window::Event::Rescaled(scale_factor)) => {
                if self.scale_factor.is_none()
                    && state.update_scale_factor(*scale_factor)
                {
                    shell.request_redraw();
                }
            }
            Event::Window(
                window::Event::CloseRequested | window::Event::Closed,
            ) => {
                // The close request is the deterministic pre-destruction path;
                // `Closed` is a defensive backup for hosts that bypass it.
                let _ = prepare_iced_native_host_close(self.handle.window_id());
                state.detach();
            }
            _ => {}
        }
    }
}

impl<'a, Message, Theme, Renderer> From<NativeVideoSlot<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(slot: NativeVideoSlot<'a, Message>) -> Self {
        Element::new(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use iced::window::raw_window_handle::{
        XlibDisplayHandle, XlibWindowHandle,
    };
    use iced::{Point, advanced::Widget};

    use crate::{contract::SessionGeneration, presenter::PresenterGeneration};

    struct FakeXlibWindow;

    impl HasWindowHandle for FakeXlibWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(7));
            // SAFETY: tests only compare the copied numeric XID and never call
            // a native API with it.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    impl HasDisplayHandle for FakeXlibWindow {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            let raw = RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, 0));
            // SAFETY: tests only compare the copied inert handle.
            Ok(unsafe { DisplayHandle::borrow_raw(raw) })
        }
    }

    fn identity() -> PresenterIdentity {
        PresenterIdentity::new(
            SessionGeneration::new(4),
            PresenterGeneration::new(2),
        )
    }

    fn capture(window_id: window::Id) {
        release_captured_iced_host(window_id);
        assert_eq!(
            capture_iced_native_host_from_window(window_id, &FakeXlibWindow,),
            Ok(NativeHostPlatform::X11)
        );
    }

    #[test]
    fn generic_iced_host_capture_stays_borrowed_in_local_registry() {
        let window_id = window::Id::unique();
        capture(window_id);

        let observed = with_captured_iced_host(window_id, |host| {
            assert_eq!(host.window_id(), window_id);
            assert_eq!(host.platform(), NativeHostPlatform::X11);
            (
                host.window_handle().unwrap().as_raw(),
                host.display_handle().unwrap().as_raw(),
            )
        });

        assert!(matches!(
            observed,
            Some((RawWindowHandle::Xlib(_), RawDisplayHandle::Xlib(_)))
        ));
        assert!(release_captured_iced_host(window_id));
        assert!(!has_captured_iced_host(window_id));
    }

    #[test]
    fn redraw_emits_only_monotonic_changed_geometry() {
        let window_id = window::Id::unique();
        capture(window_id);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_for_callback = Rc::clone(&events);
        let handle = NativeVideoSlotHandle::new(
            window_id,
            identity(),
            move |host, envelope| {
                assert!(host.is_some());
                events_for_callback.borrow_mut().push(envelope);
                NativeVideoSlotDirective::IDLE
            },
        );
        let mut state = NativeVideoSlotState::new(handle);
        let bounds =
            Rectangle::new(Point::new(10.0, 20.0), Size::new(1280.0, 720.0));
        let viewport = Rectangle::new(Point::ORIGIN, Size::new(1920.0, 1080.0));

        assert_eq!(
            state.redraw(bounds, viewport, None),
            NativeVideoSlotDirective::IDLE
        );
        assert_eq!(
            state.redraw(bounds, viewport, None),
            NativeVideoSlotDirective::IDLE
        );
        state.observed_scale_factor = 2.0;
        assert_eq!(
            state.redraw(bounds, viewport, None),
            NativeVideoSlotDirective::IDLE
        );

        let events = events.borrow();
        assert_eq!(events.len(), 2);
        let PresenterInput::HostReady { geometry: first } = events[0].input
        else {
            panic!("first redraw must announce the host");
        };
        let PresenterInput::GeometryChanged(second) = events[1].input else {
            panic!("scale change must produce one geometry revision");
        };
        assert_eq!(first.revision, GeometryRevision::new(1));
        assert_eq!(second.revision, GeometryRevision::new(2));
        assert_eq!(first.scale_factor, 1.0);
        assert_eq!(second.scale_factor, 2.0);
        drop(events);
        drop(state);
        release_captured_iced_host(window_id);
    }

    #[test]
    fn clipping_and_zero_size_are_reported_as_hidden_geometry() {
        let window_id = window::Id::unique();
        capture(window_id);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_for_callback = Rc::clone(&events);
        let handle = NativeVideoSlotHandle::new(
            window_id,
            identity(),
            move |_host, envelope| {
                events_for_callback.borrow_mut().push(envelope);
                NativeVideoSlotDirective::IDLE
            },
        );
        let mut state = NativeVideoSlotState::new(handle);
        let bounds =
            Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 80.0));
        let clipped =
            Rectangle::new(Point::new(50.0, 0.0), Size::new(30.0, 50.0));

        state.redraw(bounds, clipped, Some(1.5));
        state.redraw(
            Rectangle::new(Point::new(10.0, 20.0), Size::ZERO),
            clipped,
            Some(1.5),
        );

        let events = events.borrow();
        let PresenterInput::HostReady { geometry: first } = events[0].input
        else {
            panic!("expected host geometry");
        };
        assert_eq!(
            first.visible_bounds,
            Some(LogicalRect::new(50.0, 20.0, 30.0, 30.0))
        );
        assert!(first.is_visible());

        let PresenterInput::GeometryChanged(zero) = events[1].input else {
            panic!("expected changed zero-size geometry");
        };
        assert_eq!(zero.visible_bounds, None);
        assert!(!zero.is_visible());
        drop(events);
        drop(state);
        release_captured_iced_host(window_id);
    }

    #[test]
    fn missing_host_requests_one_capture_without_consuming_revision() {
        let window_id = window::Id::unique();
        release_captured_iced_host(window_id);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_for_callback = Rc::clone(&events);
        let handle = NativeVideoSlotHandle::new(
            window_id,
            identity(),
            move |_host, envelope| {
                events_for_callback.borrow_mut().push(envelope);
                NativeVideoSlotDirective::IDLE
            },
        );
        let mut state = NativeVideoSlotState::new(handle);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(640.0, 360.0));

        assert_eq!(
            state.redraw(bounds, bounds, None),
            NativeVideoSlotDirective::CAPTURE_HOST
        );
        assert!(events.borrow().is_empty());
        assert_eq!(state.last_revision, None);

        capture(window_id);
        state.redraw(bounds, bounds, None);
        assert!(matches!(
            events.borrow()[0].input,
            PresenterInput::HostReady { .. }
        ));
        drop(state);
        release_captured_iced_host(window_id);
    }

    #[test]
    fn transient_tree_state_drop_preserves_slot_generation_and_readiness() {
        let window_id = window::Id::unique();
        capture(window_id);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_for_callback = Rc::clone(&events);
        let handle = NativeVideoSlotHandle::new(
            window_id,
            identity(),
            move |_host, envelope| {
                events_for_callback.borrow_mut().push(envelope.input);
                NativeVideoSlotDirective::IDLE
            },
        );
        let slot = NativeVideoSlot::new(handle.clone(), |_| ());
        let mut tree = Tree::new(&slot as &dyn Widget<(), (), iced::Renderer>);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(1280.0, 720.0));

        assert_eq!(
            tree.state
                .downcast_mut::<NativeVideoSlotState>()
                .redraw(bounds, bounds, None),
            NativeVideoSlotDirective::IDLE
        );
        drop(tree);
        assert!(!handle.is_detached());

        assert_eq!(
            handle.notify(PresenterInput::VideoOutputReady),
            NativeVideoSlotDirective::IDLE
        );
        assert!(matches!(
            events.borrow().as_slice(),
            [
                PresenterInput::HostReady { .. },
                PresenterInput::VideoOutputReady
            ]
        ));

        assert_eq!(handle.detach(), NativeVideoSlotDirective::IDLE);
        assert!(handle.is_detached());
        assert!(matches!(
            events.borrow().as_slice(),
            [
                PresenterInput::HostReady { .. },
                PresenterInput::VideoOutputReady,
                PresenterInput::Detach
            ]
        ));
        release_captured_iced_host(window_id);
    }

    #[test]
    fn diff_to_a_real_replacement_explicitly_detaches_old_generation() {
        let window_id = window::Id::unique();
        let events = Rc::new(RefCell::new(Vec::new()));
        let old_events = Rc::clone(&events);
        let old_handle = NativeVideoSlotHandle::new(
            window_id,
            identity(),
            move |_host, envelope| {
                old_events.borrow_mut().push(envelope.input);
                NativeVideoSlotDirective::IDLE
            },
        );
        let old_slot = NativeVideoSlot::new(old_handle.clone(), |_| ());
        let mut tree =
            Tree::new(&old_slot as &dyn Widget<(), (), iced::Renderer>);

        let replacement = NativeVideoSlotHandle::new(
            window_id,
            PresenterIdentity::new(
                SessionGeneration::new(4),
                PresenterGeneration::new(3),
            ),
            |_host, _envelope| NativeVideoSlotDirective::IDLE,
        );
        let replacement_slot =
            NativeVideoSlot::new(replacement.clone(), |_| ());
        <NativeVideoSlot<'_, ()> as Widget<(), (), iced::Renderer>>::diff(
            &replacement_slot,
            &mut tree,
        );

        assert!(old_handle.is_detached());
        assert!(!replacement.is_detached());
        assert_eq!(events.borrow().as_slice(), &[PresenterInput::Detach]);
    }

    #[test]
    fn dropped_generations_do_not_accumulate_slot_registrations() {
        let window_id = window::Id::unique();
        let handle = NativeVideoSlotHandle::new(
            window_id,
            identity(),
            |_host, _envelope| NativeVideoSlotDirective::IDLE,
        );
        assert_eq!(
            ICED_NATIVE_VIDEO_SLOTS.with(|slots| slots
                .borrow()
                .get(&window_id)
                .map_or(0, Vec::len)),
            1
        );

        drop(handle);
        assert_eq!(
            ICED_NATIVE_VIDEO_SLOTS.with(|slots| slots
                .borrow()
                .get(&window_id)
                .map_or(0, Vec::len)),
            0
        );
    }

    #[test]
    fn explicit_host_close_detaches_all_slots_before_releasing_host() {
        let window_id = window::Id::unique();
        capture(window_id);
        let events = Rc::new(RefCell::new(Vec::new()));

        let first_events = Rc::clone(&events);
        let first = NativeVideoSlotHandle::new(
            window_id,
            identity(),
            move |host, envelope| {
                first_events
                    .borrow_mut()
                    .push((host.is_some(), envelope.input));
                NativeVideoSlotDirective::IDLE
            },
        );
        let second_events = Rc::clone(&events);
        let second = NativeVideoSlotHandle::new(
            window_id,
            PresenterIdentity::new(
                SessionGeneration::new(5),
                PresenterGeneration::new(3),
            ),
            move |host, envelope| {
                second_events
                    .borrow_mut()
                    .push((host.is_some(), envelope.input));
                NativeVideoSlotDirective::IDLE
            },
        );

        let result = prepare_iced_native_host_close(window_id);
        assert_eq!(
            result,
            NativeHostCloseResult {
                detached_slots: 2,
                released_host: true,
            }
        );
        assert!(first.is_detached());
        assert!(second.is_detached());
        assert!(!has_captured_iced_host(window_id));
        assert_eq!(
            events.borrow().as_slice(),
            &[
                (true, PresenterInput::Detach),
                (true, PresenterInput::Detach),
            ]
        );

        assert_eq!(
            prepare_iced_native_host_close(window_id),
            NativeHostCloseResult {
                detached_slots: 0,
                released_host: false,
            }
        );
    }
}
