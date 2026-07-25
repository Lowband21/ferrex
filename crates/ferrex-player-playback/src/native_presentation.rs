//! UI-thread-local bridge from the neutral presenter lifecycle to a platform
//! native presenter.
//!
//! The bridge owns no decoded frames. It keeps native window objects on the
//! Iced event-loop thread, queues pointer-free presenter events for the normal
//! playback reducer, and exposes only a renderer-neutral video slot to views.

#![cfg_attr(
    all(test, not(any(target_os = "windows", target_os = "macos"))),
    allow(dead_code)
)]

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fmt,
    rc::Rc,
    time::{Duration, Instant},
};

use iced::window;

use crate::{
    contract::{
        PlaybackError, PlaybackErrorKind, PlaybackTarget, PresenterEvent,
        PresenterState, SessionGeneration,
    },
    native_video_slot::{
        CapturedIcedHost, NativeVideoSlotDirective, NativeVideoSlotHandle,
    },
    presenter::{
        PresenterCapabilities, PresenterCommand, PresenterEffect,
        PresenterGeneration, PresenterIdentity, PresenterInput,
        PresenterInputEnvelope, PresenterLifecycle,
    },
};

const PRESENTER_READINESS_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::{
    contract::{FallbackReason, FallbackReasonCode},
    presenter::NativePresenter,
};

trait PlatformPresenterDriver {
    fn execute(
        &mut self,
        command: PresenterCommand,
        host: Option<&CapturedIcedHost>,
    ) -> Result<(), PlaybackError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadinessMissing {
    Host,
    VideoOutput,
    HostAndVideoOutput,
}

impl ReadinessMissing {
    const fn label(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::VideoOutput => "video_output",
            Self::HostAndVideoOutput => "host,video_output",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ReadinessWait {
    missing: ReadinessMissing,
    started_at: Instant,
}

struct BridgeInner {
    lifecycle: PresenterLifecycle,
    driver: Option<Box<dyn PlatformPresenterDriver>>,
    pending_events: VecDeque<PresenterEvent>,
    video_output_started: bool,
    readiness_wait: Option<ReadinessWait>,
}

impl fmt::Debug for BridgeInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BridgeInner")
            .field("identity", &self.lifecycle.identity())
            .field("state", &self.lifecycle.state())
            .field("geometry", &self.lifecycle.geometry())
            .field("driver_ready", &self.driver.is_some())
            .field("video_output_started", &self.video_output_started)
            .field("readiness_wait", &self.readiness_wait)
            .field("pending_events", &self.pending_events.len())
            .finish()
    }
}

impl BridgeInner {
    fn handle(
        &mut self,
        host: Option<&CapturedIcedHost>,
        envelope: PresenterInputEnvelope,
    ) -> NativeVideoSlotDirective {
        let input_kind = presenter_input_label(&envelope.input);
        let previous_state = self.lifecycle.state();
        let previous_readiness = self.lifecycle.readiness();
        let refresh_after_fullscreen =
            matches!(&envelope.input, PresenterInput::FullscreenConfirmed(_));
        let transition = self.lifecycle.handle(envelope);
        let changed = !transition.effects.is_empty();
        self.apply_effects(host, transition.effects);
        let state = self.lifecycle.state();
        let readiness = self.lifecycle.readiness();
        if previous_state != state || previous_readiness != readiness {
            log::debug!(
                "native presenter lifecycle transition: input={input_kind} state={previous_state:?}->{state:?} host_ready={} video_output_ready={} attached={}",
                readiness.0,
                readiness.1,
                readiness.2,
            );
        }
        if self.readiness_missing().is_none() {
            self.readiness_wait = None;
        }

        // Native fullscreen can move or resize a backend-owned root without
        // changing the Iced slot's logical rectangle. Refresh from the native
        // root after mpv confirms the property instead of waiting for a
        // coincidental Iced layout revision.
        if refresh_after_fullscreen && self.refresh_platform_window(host) {
            return NativeVideoSlotDirective::REDRAW.with_snapshot_sync();
        }

        if changed || !self.pending_events.is_empty() {
            NativeVideoSlotDirective::REDRAW.with_snapshot_sync()
        } else {
            NativeVideoSlotDirective::IDLE
        }
    }

    /// Re-query the platform-owned video root even when Iced's logical slot
    /// geometry has not changed. Win32 owned windows do not follow their owner
    /// and AppKit child windows still need occlusion/Space refreshes, so this
    /// deliberately bypasses lifecycle geometry-revision deduplication.
    fn refresh_platform_window(
        &mut self,
        host: Option<&CapturedIcedHost>,
    ) -> bool {
        if self.check_readiness_timeout(host, Instant::now()) {
            return true;
        }
        let Some(geometry) = self.lifecycle.geometry() else {
            return false;
        };
        let Some(driver) = self.driver.as_mut() else {
            return false;
        };
        let result = driver.execute(
            PresenterCommand::Synchronize {
                identity: self.lifecycle.identity(),
                geometry,
            },
            host,
        );
        if let Err(error) = result {
            self.fail(host, error);
            return true;
        }
        false
    }

    fn set_video_output_started(&mut self, started: bool) {
        if self.video_output_started == started {
            return;
        }
        self.video_output_started = started;
        if !started {
            self.readiness_wait = None;
        }
        log::debug!(
            "native presenter video-output activity transition: started={started}"
        );
    }

    fn readiness_missing(&self) -> Option<ReadinessMissing> {
        if !self.video_output_started {
            return None;
        }
        match self.lifecycle.state() {
            PresenterState::AwaitingHost => Some(ReadinessMissing::Host),
            PresenterState::AwaitingVideoOutput => {
                Some(ReadinessMissing::VideoOutput)
            }
            PresenterState::Detached => {
                Some(ReadinessMissing::HostAndVideoOutput)
            }
            PresenterState::Attached
            | PresenterState::Hidden
            | PresenterState::Suspended
            | PresenterState::Failed => None,
        }
    }

    fn check_readiness_timeout(
        &mut self,
        host: Option<&CapturedIcedHost>,
        now: Instant,
    ) -> bool {
        let Some(missing) = self.readiness_missing() else {
            self.readiness_wait = None;
            return false;
        };
        let Some(wait) =
            self.readiness_wait.filter(|wait| wait.missing == missing)
        else {
            self.readiness_wait = Some(ReadinessWait {
                missing,
                started_at: now,
            });
            log::debug!(
                "native presenter readiness watchdog armed: missing={} timeout_ms={}",
                missing.label(),
                PRESENTER_READINESS_TIMEOUT.as_millis(),
            );
            return false;
        };
        if now.saturating_duration_since(wait.started_at)
            < PRESENTER_READINESS_TIMEOUT
        {
            return false;
        }

        self.readiness_wait = None;
        log::warn!(
            "native presenter readiness timed out: missing={} timeout_ms={}",
            missing.label(),
            PRESENTER_READINESS_TIMEOUT.as_millis(),
        );
        self.fail(
            host,
            presenter_error(format!(
                "native presenter readiness timed out after {}ms (missing={})",
                PRESENTER_READINESS_TIMEOUT.as_millis(),
                missing.label(),
            )),
        );
        true
    }

    fn apply_effects(
        &mut self,
        host: Option<&CapturedIcedHost>,
        effects: Vec<PresenterEffect>,
    ) {
        for effect in effects {
            match effect {
                PresenterEffect::Event(event) => {
                    self.pending_events.push_back(event);
                }
                PresenterEffect::Command(command) => {
                    let command_kind = presenter_command_label(&command);
                    // Detach is deliberately idempotent. A generation can be
                    // reset after a failed platform presenter construction,
                    // when there is no live driver left to receive it.
                    let result = if self.driver.is_none()
                        && matches!(&command, PresenterCommand::Detach { .. })
                    {
                        Ok(())
                    } else {
                        self.driver
                            .as_mut()
                            .ok_or_else(|| {
                                presenter_error(
                                    "native presenter command arrived before the platform video output was ready",
                                )
                            })
                            .and_then(|driver| driver.execute(command, host))
                    };
                    match result {
                        Ok(()) => {
                            if command_kind == "attach" {
                                log::debug!(
                                    "native presenter platform attach completed"
                                );
                            }
                        }
                        Err(error) => {
                            log::warn!(
                                "native presenter platform command failed: command={command_kind} error={error}"
                            );
                            self.fail(host, error);
                            break;
                        }
                    }
                }
            }
        }
    }

    fn fail(&mut self, host: Option<&CapturedIcedHost>, error: PlaybackError) {
        let identity = self.lifecycle.identity();
        let transition = self.lifecycle.handle(PresenterInputEnvelope::new(
            identity,
            PresenterInput::Failed(error),
        ));
        // A failure transition can contain only an idempotent detach command
        // and copied events. Execute it once without recursively failing an
        // already-failed lifecycle if platform detach itself is best-effort.
        for effect in transition.effects {
            match effect {
                PresenterEffect::Event(event) => {
                    self.pending_events.push_back(event);
                }
                PresenterEffect::Command(command) => {
                    if let Some(driver) = self.driver.as_mut() {
                        let _ = driver.execute(command, host);
                    }
                }
            }
        }
    }

    fn begin_generation(&mut self, identity: PresenterIdentity) {
        self.video_output_started = false;
        self.readiness_wait = None;
        let capabilities = self.lifecycle.capabilities().clone();
        let transition =
            self.lifecycle.begin_generation(identity, capabilities);
        self.apply_effects(None, transition.effects);
    }
}

/// One integrated native presentation attempt owned by a playback session.
pub(crate) struct NativePresentation {
    inner: Rc<RefCell<BridgeInner>>,
    slot: RefCell<Option<NativeVideoSlotHandle>>,
    slot_window: Cell<Option<window::Id>>,
    presenter_generation: Cell<PresenterGeneration>,
    video_output_ready: Cell<bool>,
    vo_configured: Cell<bool>,
    native_output_id: Cell<Option<i64>>,
    host_visible: Cell<bool>,
    confirmed_fullscreen: Cell<Option<bool>>,
    fullscreen_request: Rc<Cell<Option<bool>>>,
    #[cfg(target_os = "windows")]
    windows_build_mode: crate::windows_presenter::WindowsPresenterBuildMode,
    #[cfg(target_os = "macos")]
    macos_build_mode: crate::macos_presenter::MacOsPresenterBuildMode,
}

impl fmt::Debug for NativePresentation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativePresentation")
            .field("inner", &self.inner.borrow())
            .field("slot_window", &self.slot_window.get())
            .field("video_output_ready", &self.video_output_ready.get())
            .field("vo_configured", &self.vo_configured.get())
            .field("host_visible", &self.host_visible.get())
            .field(
                "native_output_id_observed",
                &self.native_output_id.get().is_some(),
            )
            .field("confirmed_fullscreen", &self.confirmed_fullscreen.get())
            .finish_non_exhaustive()
    }
}

impl NativePresentation {
    fn from_capabilities(
        generation: SessionGeneration,
        capabilities: PresenterCapabilities,
        #[cfg(target_os = "windows")]
        windows_build_mode: crate::windows_presenter::WindowsPresenterBuildMode,
        #[cfg(target_os = "macos")]
        macos_build_mode: crate::macos_presenter::MacOsPresenterBuildMode,
    ) -> Self {
        let presenter_generation = PresenterGeneration::INITIAL;
        let identity = PresenterIdentity::new(generation, presenter_generation);
        Self {
            inner: Rc::new(RefCell::new(BridgeInner {
                lifecycle: PresenterLifecycle::new(
                    identity,
                    PlaybackTarget::MPV_INTEGRATED,
                    PlaybackTarget::MPV_NATIVE_WINDOW,
                    capabilities,
                    false,
                ),
                driver: None,
                pending_events: VecDeque::new(),
                video_output_started: false,
                readiness_wait: None,
            })),
            slot: RefCell::new(None),
            slot_window: Cell::new(None),
            presenter_generation: Cell::new(presenter_generation),
            video_output_ready: Cell::new(false),
            vo_configured: Cell::new(false),
            native_output_id: Cell::new(None),
            host_visible: Cell::new(false),
            confirmed_fullscreen: Cell::new(None),
            fullscreen_request: Rc::new(Cell::new(None)),
            #[cfg(target_os = "windows")]
            windows_build_mode,
            #[cfg(target_os = "macos")]
            macos_build_mode,
        }
    }

    /// Preflight the target presenter before the adapter advertises integrated
    /// presentation. Handle-dependent validation still runs immediately before
    /// `VideoOutputReady` and again during attach.
    #[cfg(target_os = "windows")]
    pub(crate) fn try_new(
        generation: SessionGeneration,
    ) -> Result<Self, FallbackReason> {
        use crate::windows_presenter::{
            Win32WindowSystem, WindowsPresenterAvailability,
            WindowsPresenterBuildMode, WindowsPresenterProbe,
            windows_presenter_capabilities,
        };

        let build_mode = WindowsPresenterBuildMode::compiled();
        if matches!(build_mode, WindowsPresenterBuildMode::Disabled) {
            return Err(WindowsPresenterAvailability::evaluate(
                WindowsPresenterProbe {
                    build_mode,
                    ..WindowsPresenterProbe::default()
                },
            )
            .fallback_reason()
            .expect("disabled presenter has a fallback"));
        }
        let composition = Win32WindowSystem::composition_available()
            .map_err(|error| platform_fallback(error.to_string()))?;
        if !composition {
            return Err(platform_fallback(
                "Windows DWM composition is unavailable",
            ));
        }

        Ok(Self::from_capabilities(
            generation,
            windows_presenter_capabilities(build_mode),
            build_mode,
        ))
    }

    /// Conservative macOS gate. This remains explicit-spike-only until the
    /// representative Spaces/fullscreen/scale/teardown matrix is signed off.
    #[cfg(target_os = "macos")]
    pub(crate) fn try_new(
        generation: SessionGeneration,
    ) -> Result<Self, FallbackReason> {
        use crate::macos_presenter::{
            AppKitWindowSystem, MacOsPresenterBuildMode,
            macos_presenter_capabilities,
        };

        let build_mode = MacOsPresenterBuildMode::compiled();
        if matches!(build_mode, MacOsPresenterBuildMode::Disabled) {
            return Err(mac_platform_fallback(
                FallbackReasonCode::MissingCapability,
                "macOS integrated presenter is disabled in this build",
            ));
        }
        if !AppKitWindowSystem::main_thread_available() {
            return Err(mac_platform_fallback(
                FallbackReasonCode::UnsupportedPlatform,
                "macOS integrated presenter is not running on the AppKit main thread",
            ));
        }

        Ok(Self::from_capabilities(
            generation,
            macos_presenter_capabilities(build_mode),
            build_mode,
        ))
    }

    pub(crate) fn capabilities(&self) -> PresenterCapabilities {
        self.inner.borrow().lifecycle.capabilities().clone()
    }

    pub(crate) fn slot_handle(
        &self,
        window_id: window::Id,
    ) -> NativeVideoSlotHandle {
        let cached = self.slot.borrow().as_ref().cloned();
        if let Some(handle) = cached.as_ref()
            && self.slot_window.get() == Some(window_id)
            && !handle.is_detached()
        {
            return handle.clone();
        }

        let previous_window = self.slot_window.get();
        let replacing_slot = cached.is_some() || previous_window.is_some();
        let replay_video_output = self.video_output_ready.get();
        let replay_host_visible = self.host_visible.get();
        let replay_fullscreen = self.confirmed_fullscreen.get();
        let replay_video_output_started =
            self.inner.borrow().video_output_started;

        if cached
            .as_ref()
            .is_some_and(NativeVideoSlotHandle::is_detached)
        {
            log::debug!(
                "native presenter recovering detached cached slot: same_window={}",
                previous_window == Some(window_id),
            );
        }
        if let Some(old) = self.slot.borrow_mut().take() {
            if !old.is_detached() {
                let _ = old.detach();
            }
        }
        if replacing_slot {
            self.advance_generation();
        }

        let identity = self.inner.borrow().lifecycle.identity();
        let inner = Rc::clone(&self.inner);
        let handle = NativeVideoSlotHandle::new(
            window_id,
            identity,
            move |host, envelope| inner.borrow_mut().handle(host, envelope),
        );
        self.slot_window.set(Some(window_id));
        *self.slot.borrow_mut() = Some(handle.clone());

        // A slot can be recreated while mpv's native output remains live. The
        // lifecycle generation was reset above, so replay process-level
        // readiness into the new slot instead of waiting for an mpv property
        // transition that may never occur.
        self.inner
            .borrow_mut()
            .set_video_output_started(replay_video_output_started);
        let _ = handle
            .notify(PresenterInput::HostVisibilityChanged(replay_host_visible));
        if replay_video_output {
            let _ = handle.notify(PresenterInput::VideoOutputReady);
            if let Some(fullscreen) = replay_fullscreen {
                let _ = handle
                    .notify(PresenterInput::FullscreenConfirmed(fullscreen));
            }
        }

        handle
    }

    pub(crate) fn synchronize_native_output(
        &self,
        native_output_id: Option<i64>,
        vo_configured: bool,
        video_output_started: bool,
        fullscreen: bool,
    ) {
        #[cfg(target_os = "windows")]
        let ready = vo_configured && native_output_id.is_some();
        #[cfg(target_os = "macos")]
        let ready = vo_configured && native_output_id.is_some();
        #[cfg(all(test, not(any(target_os = "windows", target_os = "macos"))))]
        let ready = vo_configured;

        let previous_vo_configured = self.vo_configured.replace(vo_configured);
        let previous_output_id = self.native_output_id.get();
        let output_changed = previous_output_id != native_output_id;
        if previous_vo_configured != vo_configured || output_changed {
            log::debug!(
                "native presenter output observation transition: vo_configured={vo_configured} native_window_id_observed={} identity_changed={}",
                native_output_id.is_some(),
                previous_output_id.is_some()
                    && native_output_id.is_some()
                    && output_changed,
            );
        }
        if self.video_output_ready.get() && (!ready || output_changed) {
            self.dispatch(PresenterInput::VideoOutputLost);
            self.video_output_ready.set(false);
            log::debug!(
                "native presenter video output transition: ready=false identity_changed={output_changed}"
            );
            self.inner.borrow_mut().driver = None;
            self.confirmed_fullscreen.set(None);
            self.rotate_slot_generation();
        }
        self.native_output_id.set(native_output_id);

        if ready {
            // A close/rebuild can detach the cached UI handle without changing
            // mpv's already-ready output properties. Reconcile that condition
            // on every ready observation, not only on the property edge.
            self.recover_detached_slot();
        }

        let mut video_output_became_ready = false;
        if ready && !self.video_output_ready.get() {
            match self.create_platform_driver(native_output_id) {
                Ok(driver) => {
                    self.inner.borrow_mut().driver = Some(driver);
                    log::debug!(
                        "native presenter platform driver created: native_window_id_observed=true"
                    );
                    self.video_output_ready.set(true);
                    video_output_became_ready = true;
                    log::debug!(
                        "native presenter video output transition: ready=true identity_changed={output_changed}"
                    );
                    self.dispatch(PresenterInput::VideoOutputReady);
                }
                Err(error) => {
                    log::warn!(
                        "native presenter platform driver creation failed: {error}"
                    );
                    self.dispatch(PresenterInput::Failed(error));
                }
            }
        }

        self.inner.borrow_mut().set_video_output_started(
            video_output_started && cfg!(any(target_os = "macos", test)),
        );

        let previous_fullscreen =
            self.confirmed_fullscreen.replace(Some(fullscreen));
        if (previous_fullscreen != Some(fullscreen)
            || video_output_became_ready)
            && self.video_output_ready.get()
        {
            self.dispatch(PresenterInput::FullscreenConfirmed(fullscreen));
        }
    }

    #[cfg(target_os = "windows")]
    fn create_platform_driver(
        &self,
        native_output_id: Option<i64>,
    ) -> Result<Box<dyn PlatformPresenterDriver>, PlaybackError> {
        use crate::windows_presenter::{
            Win32WindowSystem, WindowsHwnd, WindowsPresenter,
        };

        let value = native_output_id.ok_or_else(|| {
            presenter_error("mpv did not expose a Windows window-id")
        })?;
        let video_root = WindowsHwnd::from_mpv_window_id(value)
            .map_err(PlaybackError::from)?;
        let windows = Win32WindowSystem;
        if !windows.is_live(video_root) {
            return Err(presenter_error(
                "mpv exposed a stale or destroyed Windows window-id",
            ));
        }

        let fullscreen_request = Rc::clone(&self.fullscreen_request);
        let fullscreen: WindowsFullscreenCallback = Box::new(move |value| {
            fullscreen_request.set(Some(value));
            Ok(())
        });
        Ok(Box::new(WindowsPresenterDriver {
            presenter: WindowsPresenter::new(
                windows,
                fullscreen,
                video_root,
                self.windows_build_mode,
            ),
        }))
    }

    #[cfg(target_os = "macos")]
    fn create_platform_driver(
        &self,
        native_output_id: Option<i64>,
    ) -> Result<Box<dyn PlatformPresenterDriver>, PlaybackError> {
        use crate::macos_presenter::{
            AppKitWindowSystem, MacOsPresenter, MacOsWindow,
        };

        let value = native_output_id.ok_or_else(|| {
            presenter_error("mpv did not expose a macOS window-id")
        })?;
        let video_root = MacOsWindow::from_mpv_window_id(value)
            .map_err(PlaybackError::from)?;
        let appkit =
            AppKitWindowSystem::new(video_root).map_err(PlaybackError::from)?;
        if !appkit.is_live(video_root) {
            return Err(presenter_error(
                "mpv exposed a stale or destroyed macOS window-id",
            ));
        }

        let fullscreen_request = Rc::clone(&self.fullscreen_request);
        let fullscreen: MacOsFullscreenCallback = Box::new(move |value| {
            fullscreen_request.set(Some(value));
            Ok(())
        });
        Ok(Box::new(MacOsPresenterDriver {
            presenter: MacOsPresenter::new(
                appkit,
                fullscreen,
                video_root,
                self.macos_build_mode,
            ),
        }))
    }

    #[cfg(all(test, not(any(target_os = "windows", target_os = "macos"))))]
    fn create_platform_driver(
        &self,
        _native_output_id: Option<i64>,
    ) -> Result<Box<dyn PlatformPresenterDriver>, PlaybackError> {
        Err(presenter_error(
            "native presenter drivers are target-specific",
        ))
    }

    pub(crate) fn request_fullscreen(&self, fullscreen: bool) {
        self.dispatch(PresenterInput::FullscreenRequested(fullscreen));
    }

    /// Reveal or hide the already-attached native controls host. Integrated
    /// overlays start hidden so the shell can hide its retained main window
    /// before this transition exposes a second native window.
    pub(crate) fn set_host_visible(&self, visible: bool) {
        self.host_visible.set(visible);
        self.dispatch(PresenterInput::HostVisibilityChanged(visible));
    }

    pub(crate) fn refresh_platform_window(&self) {
        self.inner.borrow_mut().refresh_platform_window(None);
    }

    pub(crate) fn take_fullscreen_request(&self) -> Option<bool> {
        self.fullscreen_request.take()
    }

    pub(crate) fn fail_host_capture(
        &self,
        window_id: window::Id,
        detail: String,
    ) {
        if self.slot_window.get() != Some(window_id) {
            return;
        }
        self.dispatch(PresenterInput::Failed(presenter_error(format!(
            "could not capture the native player overlay host: {detail}"
        ))));
    }

    pub(crate) fn drain_events(&self) -> Vec<PresenterEvent> {
        self.inner.borrow_mut().pending_events.drain(..).collect()
    }

    pub(crate) fn begin_media_load(&self) {
        if let Some(handle) = self.slot.borrow_mut().take() {
            let _ = handle.detach();
        }
        self.slot_window.set(None);
        self.video_output_ready.set(false);
        self.vo_configured.set(false);
        self.native_output_id.set(None);
        self.host_visible.set(false);
        self.confirmed_fullscreen.set(None);
        self.inner.borrow_mut().driver = None;
        self.advance_generation();
    }

    pub(crate) fn detach(&self) {
        if let Some(handle) = self.slot.borrow_mut().take() {
            let _ = handle.detach();
        } else {
            self.dispatch(PresenterInput::Detach);
        }
        self.slot_window.set(None);
        self.video_output_ready.set(false);
        self.vo_configured.set(false);
        self.native_output_id.set(None);
        self.host_visible.set(false);
        self.confirmed_fullscreen.set(None);
        self.inner.borrow_mut().set_video_output_started(false);
        self.inner.borrow_mut().driver = None;
    }

    fn dispatch(&self, input: PresenterInput) {
        let cached = self.slot.borrow().as_ref().cloned();
        if let Some(handle) = cached {
            if handle.is_detached()
                && matches!(&input, PresenterInput::VideoOutputReady)
            {
                // Readiness must never disappear into a detached handle. This
                // is a defensive path for callers that observe readiness
                // between widget teardown and the next view reconstruction.
                let replacement = self.slot_handle(handle.window_id());
                let _ = replacement.notify(input);
            } else if handle.is_detached() {
                log::debug!(
                    "native presenter bypassing detached cached slot: input={}",
                    presenter_input_label(&input),
                );
                let identity = self.inner.borrow().lifecycle.identity();
                let _ = self
                    .inner
                    .borrow_mut()
                    .handle(None, PresenterInputEnvelope::new(identity, input));
            } else {
                let _ = handle.notify(input);
            }
        } else {
            let identity = self.inner.borrow().lifecycle.identity();
            let _ = self
                .inner
                .borrow_mut()
                .handle(None, PresenterInputEnvelope::new(identity, input));
        }
    }

    fn recover_detached_slot(&self) {
        let detached_window = self
            .slot
            .borrow()
            .as_ref()
            .filter(|handle| handle.is_detached())
            .map(NativeVideoSlotHandle::window_id);
        if let Some(window_id) = detached_window {
            let _ = self.slot_handle(window_id);
        }
    }

    fn advance_generation(&self) {
        let Some(next) = self.presenter_generation.get().next() else {
            self.dispatch(PresenterInput::Failed(presenter_error(
                "native presenter generation exhausted",
            )));
            return;
        };
        self.presenter_generation.set(next);
        let session = self.inner.borrow().lifecycle.identity().session;
        self.inner
            .borrow_mut()
            .begin_generation(PresenterIdentity::new(session, next));
    }

    fn rotate_slot_generation(&self) {
        if let Some(handle) = self.slot.borrow_mut().take() {
            let _ = handle.detach();
        }
        self.slot_window.set(None);
        self.advance_generation();
    }
}

impl Drop for NativePresentation {
    fn drop(&mut self) {
        self.detach();
    }
}

#[cfg(target_os = "windows")]
type WindowsFullscreenCallback =
    Box<dyn FnMut(bool) -> Result<(), PlaybackError>>;

#[cfg(target_os = "macos")]
type MacOsFullscreenCallback =
    Box<dyn FnMut(bool) -> Result<(), PlaybackError>>;

#[cfg(target_os = "windows")]
struct WindowsPresenterDriver {
    presenter: crate::windows_presenter::WindowsPresenter<
        crate::windows_presenter::Win32WindowSystem,
        WindowsFullscreenCallback,
    >,
}

#[cfg(target_os = "windows")]
impl PlatformPresenterDriver for WindowsPresenterDriver {
    fn execute(
        &mut self,
        command: PresenterCommand,
        host: Option<&CapturedIcedHost>,
    ) -> Result<(), PlaybackError> {
        use crate::windows_presenter::WindowsPresenterHost;

        match command {
            PresenterCommand::Attach { identity } => {
                let host = host.ok_or_else(|| {
                    presenter_error(
                        "Windows presenter attach requires a captured Iced host",
                    )
                })?;
                self.presenter.attach(
                    identity,
                    WindowsPresenterHost::from_captured_iced_host(host)?,
                )
            }
            PresenterCommand::Synchronize { identity, geometry } => {
                self.presenter.synchronize(identity, geometry)
            }
            PresenterCommand::SetVisible { identity, visible } => {
                self.presenter.set_visible(identity, visible)
            }
            PresenterCommand::SetSuspended {
                identity,
                suspended,
            } => self.presenter.set_suspended(identity, suspended),
            PresenterCommand::SetFullscreen {
                identity,
                owner,
                fullscreen,
            } => self.presenter.set_fullscreen(identity, owner, fullscreen),
            PresenterCommand::Detach { identity } => {
                self.presenter.detach(identity);
                Ok(())
            }
        }
    }
}

#[cfg(target_os = "macos")]
struct MacOsPresenterDriver {
    presenter: crate::macos_presenter::MacOsPresenter<
        crate::macos_presenter::AppKitWindowSystem,
        MacOsFullscreenCallback,
    >,
}

#[cfg(target_os = "macos")]
impl PlatformPresenterDriver for MacOsPresenterDriver {
    fn execute(
        &mut self,
        command: PresenterCommand,
        host: Option<&CapturedIcedHost>,
    ) -> Result<(), PlaybackError> {
        use crate::macos_presenter::MacOsPresenterHost;

        match command {
            PresenterCommand::Attach { identity } => {
                let host = host.ok_or_else(|| {
                    presenter_error(
                        "macOS presenter attach requires a captured Iced host",
                    )
                })?;
                self.presenter.attach(
                    identity,
                    MacOsPresenterHost::from_captured_iced_host(host)?,
                )
            }
            PresenterCommand::Synchronize { identity, geometry } => {
                self.presenter.synchronize(identity, geometry)
            }
            PresenterCommand::SetVisible { identity, visible } => {
                self.presenter.set_visible(identity, visible)
            }
            PresenterCommand::SetSuspended {
                identity,
                suspended,
            } => self.presenter.set_suspended(identity, suspended),
            PresenterCommand::SetFullscreen {
                identity,
                owner,
                fullscreen,
            } => self.presenter.set_fullscreen(identity, owner, fullscreen),
            PresenterCommand::Detach { identity } => {
                self.presenter.detach(identity);
                Ok(())
            }
        }
    }
}

fn presenter_input_label(input: &PresenterInput) -> &'static str {
    match input {
        PresenterInput::HostReady { .. } => "host_ready",
        PresenterInput::VideoOutputReady => "video_output_ready",
        PresenterInput::GeometryChanged(_) => "geometry_changed",
        PresenterInput::HostVisibilityChanged(_) => "host_visibility_changed",
        PresenterInput::SuspensionChanged(_) => "suspension_changed",
        PresenterInput::FullscreenRequested(_) => "fullscreen_requested",
        PresenterInput::FullscreenConfirmed(_) => "fullscreen_confirmed",
        PresenterInput::HostLost => "host_lost",
        PresenterInput::VideoOutputLost => "video_output_lost",
        PresenterInput::Detach => "detach",
        PresenterInput::Failed(_) => "failed",
    }
}

fn presenter_command_label(command: &PresenterCommand) -> &'static str {
    match command {
        PresenterCommand::Attach { .. } => "attach",
        PresenterCommand::Synchronize { .. } => "synchronize",
        PresenterCommand::SetVisible { .. } => "set_visible",
        PresenterCommand::SetSuspended { .. } => "set_suspended",
        PresenterCommand::SetFullscreen { .. } => "set_fullscreen",
        PresenterCommand::Detach { .. } => "detach",
    }
}

fn presenter_error(message: impl Into<String>) -> PlaybackError {
    let mut error = PlaybackError::new(PlaybackErrorKind::Presenter, message);
    error.backend = Some(crate::contract::BackendKind::Mpv);
    error.recoverable = true;
    error
}

#[cfg(target_os = "windows")]
fn platform_fallback(detail: impl Into<String>) -> FallbackReason {
    FallbackReason {
        code: FallbackReasonCode::UnsupportedPlatform,
        from: Some(PlaybackTarget::MPV_INTEGRATED),
        to: PlaybackTarget::MPV_NATIVE_WINDOW,
        detail: detail.into(),
    }
}

#[cfg(target_os = "macos")]
fn mac_platform_fallback(
    code: FallbackReasonCode,
    detail: impl Into<String>,
) -> FallbackReason {
    FallbackReason {
        code,
        from: Some(PlaybackTarget::MPV_INTEGRATED),
        to: PlaybackTarget::MPV_NATIVE_WINDOW,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presenter::{
        FullscreenOwner, GeometryRevision, LogicalRect, SurfaceGeometry,
    };

    #[derive(Default)]
    struct FakeDriver {
        commands: Rc<RefCell<Vec<PresenterCommand>>>,
        fail_attach: bool,
    }

    impl PlatformPresenterDriver for FakeDriver {
        fn execute(
            &mut self,
            command: PresenterCommand,
            _host: Option<&CapturedIcedHost>,
        ) -> Result<(), PlaybackError> {
            if self.fail_attach
                && matches!(command, PresenterCommand::Attach { .. })
            {
                return Err(presenter_error("fake attach failure"));
            }
            self.commands.borrow_mut().push(command);
            Ok(())
        }
    }

    fn geometry() -> SurfaceGeometry {
        SurfaceGeometry::new(
            GeometryRevision::INITIAL,
            LogicalRect::new(0.0, 0.0, 1280.0, 720.0),
            Some(LogicalRect::new(0.0, 0.0, 1280.0, 720.0)),
            1.0,
        )
    }

    fn bridge(driver: FakeDriver) -> BridgeInner {
        let capabilities = PresenterCapabilities {
            integrated_overlay: true,
            fullscreen_owner: Some(FullscreenOwner::VideoOutput),
            native_window_fallback: true,
            ..PresenterCapabilities::default()
        };
        BridgeInner {
            lifecycle: PresenterLifecycle::new(
                PresenterIdentity::new(
                    SessionGeneration::new(9),
                    PresenterGeneration::INITIAL,
                ),
                PlaybackTarget::MPV_INTEGRATED,
                PlaybackTarget::MPV_NATIVE_WINDOW,
                capabilities,
                true,
            ),
            driver: Some(Box::new(driver)),
            pending_events: VecDeque::new(),
            video_output_started: false,
            readiness_wait: None,
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    fn presentation() -> NativePresentation {
        NativePresentation::from_capabilities(
            SessionGeneration::new(17),
            PresenterCapabilities {
                integrated_overlay: true,
                fullscreen_owner: Some(FullscreenOwner::VideoOutput),
                native_window_fallback: true,
                ..PresenterCapabilities::default()
            },
        )
    }

    #[test]
    fn host_and_video_readiness_execute_attach_and_queue_snapshot_sync() {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let mut bridge = bridge(FakeDriver {
            commands: Rc::clone(&commands),
            fail_attach: false,
        });
        let identity = bridge.lifecycle.identity();

        let host = bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::HostReady {
                    geometry: geometry(),
                },
            ),
        );
        assert!(host.requests_snapshot_sync());
        assert!(commands.borrow().is_empty());

        let video = bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::VideoOutputReady,
            ),
        );
        assert!(video.requests_snapshot_sync());
        assert!(matches!(
            commands.borrow().first(),
            Some(PresenterCommand::Attach { .. })
        ));
        assert!(bridge.pending_events.iter().any(|event| matches!(
            event,
            PresenterEvent::StateChanged(
                crate::contract::PresenterState::Attached
            )
        )));
    }

    #[test]
    fn platform_attach_failure_queues_deterministic_native_window_fallback() {
        let mut bridge = bridge(FakeDriver {
            fail_attach: true,
            ..FakeDriver::default()
        });
        let identity = bridge.lifecycle.identity();
        bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::HostReady {
                    geometry: geometry(),
                },
            ),
        );
        bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::VideoOutputReady,
            ),
        );

        assert!(bridge.pending_events.iter().any(|event| matches!(
            event,
            PresenterEvent::Failure(error)
                if error.kind == PlaybackErrorKind::Presenter
        )));
        assert!(bridge.pending_events.iter().any(|event| matches!(
            event,
            PresenterEvent::FallbackRequested(reason)
                if reason.to == PlaybackTarget::MPV_NATIVE_WINDOW
        )));
    }

    #[test]
    fn platform_refresh_reissues_sync_for_unchanged_iced_geometry() {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let mut bridge = bridge(FakeDriver {
            commands: Rc::clone(&commands),
            fail_attach: false,
        });
        let identity = bridge.lifecycle.identity();
        bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::HostReady {
                    geometry: geometry(),
                },
            ),
        );
        bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::VideoOutputReady,
            ),
        );
        commands.borrow_mut().clear();

        assert!(!bridge.refresh_platform_window(None));
        assert!(!bridge.refresh_platform_window(None));
        assert_eq!(
            commands
                .borrow()
                .iter()
                .filter(|command| matches!(
                    command,
                    PresenterCommand::Synchronize { .. }
                ))
                .count(),
            2
        );
    }

    #[test]
    fn readiness_timeout_reports_missing_host_and_falls_back_once() {
        let mut bridge = bridge(FakeDriver::default());
        let identity = bridge.lifecycle.identity();
        bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::VideoOutputReady,
            ),
        );
        bridge.set_video_output_started(true);

        let started_at = Instant::now();
        assert!(!bridge.check_readiness_timeout(None, started_at));
        assert!(!bridge.check_readiness_timeout(
            None,
            started_at + PRESENTER_READINESS_TIMEOUT - Duration::from_nanos(1),
        ));
        assert!(bridge.check_readiness_timeout(
            None,
            started_at + PRESENTER_READINESS_TIMEOUT,
        ));
        assert_eq!(bridge.lifecycle.state(), PresenterState::Failed);
        assert!(bridge.pending_events.iter().any(|event| matches!(
            event,
            PresenterEvent::Failure(error)
                if error.kind == PlaybackErrorKind::Presenter
                    && error.recoverable
                    && error.message.contains("missing=host")
        )));
        assert_eq!(
            bridge
                .pending_events
                .iter()
                .filter(|event| matches!(
                    event,
                    PresenterEvent::FallbackRequested(reason)
                        if reason.to == PlaybackTarget::MPV_NATIVE_WINDOW
                ))
                .count(),
            1
        );

        let event_count = bridge.pending_events.len();
        assert!(!bridge.check_readiness_timeout(
            None,
            started_at + PRESENTER_READINESS_TIMEOUT + Duration::from_secs(1),
        ));
        assert_eq!(bridge.pending_events.len(), event_count);
    }

    #[test]
    fn readiness_timeout_reports_missing_video_output_and_clears_geometry() {
        let mut bridge = bridge(FakeDriver::default());
        let identity = bridge.lifecycle.identity();
        bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::HostReady {
                    geometry: geometry(),
                },
            ),
        );
        bridge.set_video_output_started(true);

        let started_at = Instant::now();
        assert!(!bridge.check_readiness_timeout(None, started_at));
        assert!(bridge.check_readiness_timeout(
            None,
            started_at + PRESENTER_READINESS_TIMEOUT,
        ));
        assert_eq!(bridge.lifecycle.state(), PresenterState::Failed);
        assert!(bridge.pending_events.iter().any(|event| matches!(
            event,
            PresenterEvent::GeometryChanged(None)
        )));
        assert!(bridge.pending_events.iter().any(|event| matches!(
            event,
            PresenterEvent::Failure(error)
                if error.message.contains("missing=video_output")
        )));
    }

    #[test]
    fn completed_readiness_cancels_an_armed_timeout() {
        let mut bridge = bridge(FakeDriver::default());
        let identity = bridge.lifecycle.identity();
        bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::VideoOutputReady,
            ),
        );
        bridge.set_video_output_started(true);
        let started_at = Instant::now();
        assert!(!bridge.check_readiness_timeout(None, started_at));

        bridge.handle(
            None,
            PresenterInputEnvelope::new(
                identity,
                PresenterInput::HostReady {
                    geometry: geometry(),
                },
            ),
        );
        assert_eq!(bridge.lifecycle.state(), PresenterState::Attached);
        assert!(bridge.readiness_wait.is_none());
        assert!(!bridge.check_readiness_timeout(
            None,
            started_at + PRESENTER_READINESS_TIMEOUT + Duration::from_secs(1),
        ));
        assert!(!bridge.pending_events.iter().any(|event| matches!(
            event,
            PresenterEvent::Failure(_) | PresenterEvent::FallbackRequested(_)
        )));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn recreated_slot_replays_live_video_output_before_host_readiness() {
        let presentation = presentation();
        let window_id = window::Id::unique();
        let old_slot = presentation.slot_handle(window_id);
        let old_identity = old_slot.identity();
        presentation.set_host_visible(true);
        let commands = Rc::new(RefCell::new(Vec::new()));
        presentation.inner.borrow_mut().driver = Some(Box::new(FakeDriver {
            commands: Rc::clone(&commands),
            fail_attach: false,
        }));
        presentation.video_output_ready.set(true);
        presentation
            .inner
            .borrow_mut()
            .set_video_output_started(true);
        presentation.dispatch(PresenterInput::VideoOutputReady);
        old_slot.notify(PresenterInput::HostReady {
            geometry: geometry(),
        });
        assert_eq!(
            presentation.inner.borrow().lifecycle.state(),
            PresenterState::Attached
        );

        old_slot.detach();
        let replacement = presentation.slot_handle(window_id);

        assert!(!replacement.is_detached());
        assert_ne!(replacement.identity(), old_identity);
        assert_eq!(
            presentation.inner.borrow().lifecycle.readiness(),
            (false, true, false)
        );
        assert!(presentation.host_visible.get());
        assert!(presentation.inner.borrow().video_output_started);

        replacement.notify(PresenterInput::HostReady {
            geometry: geometry(),
        });
        assert_eq!(
            presentation.inner.borrow().lifecycle.readiness(),
            (true, true, true)
        );
        assert_eq!(
            commands
                .borrow()
                .iter()
                .filter(|command| matches!(
                    command,
                    PresenterCommand::Attach { .. }
                ))
                .count(),
            2
        );
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn video_readiness_dispatch_recovers_a_detached_cached_handle() {
        let presentation = presentation();
        let window_id = window::Id::unique();
        let old_slot = presentation.slot_handle(window_id);
        let old_identity = old_slot.identity();
        old_slot.detach();
        presentation.inner.borrow_mut().driver =
            Some(Box::new(FakeDriver::default()));

        presentation.dispatch(PresenterInput::VideoOutputReady);

        let replacement = presentation
            .slot
            .borrow()
            .as_ref()
            .expect("detached slot replaced")
            .clone();
        assert!(old_slot.is_detached());
        assert!(!replacement.is_detached());
        assert_ne!(replacement.identity(), old_identity);
        assert_eq!(
            presentation.inner.borrow().lifecycle.readiness(),
            (false, true, false)
        );

        replacement.notify(PresenterInput::HostReady {
            geometry: geometry(),
        });
        assert_eq!(
            presentation.inner.borrow().lifecycle.readiness(),
            (true, true, true)
        );
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn video_output_loss_rotates_the_slot_identity_before_recreation() {
        let presentation = presentation();
        let window_id = window::Id::unique();
        let old_slot = presentation.slot_handle(window_id);
        let old_identity = old_slot.identity();
        old_slot.notify(PresenterInput::HostReady {
            geometry: geometry(),
        });
        presentation.inner.borrow_mut().driver =
            Some(Box::new(FakeDriver::default()));
        presentation.video_output_ready.set(true);
        presentation.dispatch(PresenterInput::VideoOutputReady);

        presentation.synchronize_native_output(None, false, false, false);

        assert!(old_slot.is_detached());
        assert!(presentation.slot.borrow().is_none());
        assert_eq!(presentation.slot_window.get(), None);
        let replacement = presentation.slot_handle(window_id);
        assert_ne!(replacement.identity(), old_identity);
        assert_eq!(
            replacement.identity().presenter,
            old_identity
                .presenter
                .next()
                .expect("next presenter generation")
        );
    }
}
