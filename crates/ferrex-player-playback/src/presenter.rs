//! Platform-neutral native presenter lifecycle and geometry model.
//!
//! This module deliberately contains no raw window-system types. A widget or
//! event-loop-local platform adapter keeps native host/VO objects locally and
//! executes [`crate::presenter::PresenterCommand`] values on the UI thread.
//! Consequently, [`crate::presenter::NativePresenter`] does not require its
//! host resource or implementation to
//! be `Send`.

use crate::contract::{
    BackendKind, FallbackReason, FallbackReasonCode, PlaybackError,
    PlaybackErrorKind, PlaybackTarget, PresenterEvent, PresenterState,
    SessionGeneration,
};
pub use crate::contract::{
    GeometryRevision, LogicalRect, SurfaceGeometry, SurfaceGeometryError,
};

/// Monotonically increasing identity for one presenter attachment attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresenterGeneration(u64);

impl PresenterGeneration {
    /// First valid presenter generation.
    pub const INITIAL: Self = Self(1);

    /// Construct a presenter generation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the numeric generation.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advance without wrapping.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Session and presenter generations that scope every lifecycle operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresenterIdentity {
    /// Playback session that owns the video output.
    pub session: SessionGeneration,
    /// Attachment attempt within/across host window recreation.
    pub presenter: PresenterGeneration,
}

impl PresenterIdentity {
    /// Construct one scoped presenter identity.
    pub const fn new(
        session: SessionGeneration,
        presenter: PresenterGeneration,
    ) -> Self {
        Self { session, presenter }
    }
}

/// Native object that owns confirmed fullscreen state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenOwner {
    /// The Iced host/top-level owns fullscreen (for example, Wayland).
    HostWindow,
    /// mpv's native root video window owns fullscreen.
    VideoOutput,
}

/// Capabilities reported by a platform presenter implementation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresenterCapabilities {
    pub integrated_overlay: bool,
    pub embedded_surface: bool,
    pub native_hdr: bool,
    pub fractional_scaling: bool,
    pub native_window_fallback: bool,
    pub fullscreen_owner: Option<FullscreenOwner>,
    pub compositor_requirement: Option<String>,
}

/// UI-thread-local native presenter interface.
///
/// The generic associated host may be a borrowed `Rc`/AppKit/Wayland value and
/// intentionally has no `Send` bound.
pub trait NativePresenter {
    /// Borrowed host representation used only while attaching.
    type Host<'host>
    where
        Self: 'host;

    fn attach(
        &mut self,
        identity: PresenterIdentity,
        host: Self::Host<'_>,
    ) -> Result<(), PlaybackError>;

    fn synchronize(
        &mut self,
        identity: PresenterIdentity,
        geometry: SurfaceGeometry,
    ) -> Result<(), PlaybackError>;

    fn set_visible(
        &mut self,
        identity: PresenterIdentity,
        visible: bool,
    ) -> Result<(), PlaybackError>;

    fn set_suspended(
        &mut self,
        identity: PresenterIdentity,
        suspended: bool,
    ) -> Result<(), PlaybackError>;

    fn set_fullscreen(
        &mut self,
        identity: PresenterIdentity,
        owner: FullscreenOwner,
        fullscreen: bool,
    ) -> Result<(), PlaybackError>;

    fn detach(&mut self, identity: PresenterIdentity);

    fn capabilities(&self) -> &PresenterCapabilities;
}

/// Event-loop-local operation emitted by [`PresenterLifecycle`].
#[derive(Debug, Clone, PartialEq)]
pub enum PresenterCommand {
    Attach {
        identity: PresenterIdentity,
    },
    Synchronize {
        identity: PresenterIdentity,
        geometry: SurfaceGeometry,
    },
    SetVisible {
        identity: PresenterIdentity,
        visible: bool,
    },
    SetSuspended {
        identity: PresenterIdentity,
        suspended: bool,
    },
    SetFullscreen {
        identity: PresenterIdentity,
        owner: FullscreenOwner,
        fullscreen: bool,
    },
    Detach {
        identity: PresenterIdentity,
    },
}

/// Host, VO, or user input consumed by the presenter lifecycle reducer.
#[derive(Debug, Clone, PartialEq)]
pub enum PresenterInput {
    HostReady { geometry: SurfaceGeometry },
    VideoOutputReady,
    GeometryChanged(SurfaceGeometry),
    HostVisibilityChanged(bool),
    SuspensionChanged(bool),
    FullscreenRequested(bool),
    FullscreenConfirmed(bool),
    HostLost,
    VideoOutputLost,
    Detach,
    Failed(PlaybackError),
}

/// Generation-scoped presenter input.
#[derive(Debug, Clone, PartialEq)]
pub struct PresenterInputEnvelope {
    pub identity: PresenterIdentity,
    pub input: PresenterInput,
}

impl PresenterInputEnvelope {
    /// Wrap one input with its attachment identity.
    pub const fn new(
        identity: PresenterIdentity,
        input: PresenterInput,
    ) -> Self {
        Self { identity, input }
    }
}

/// Side effect produced by the pure lifecycle model.
#[derive(Debug, Clone, PartialEq)]
pub enum PresenterEffect {
    /// Execute on the native UI/event-loop thread.
    Command(PresenterCommand),
    /// Wrap in `PlaybackEvent::Presenter` for snapshot reduction.
    Event(PresenterEvent),
}

/// Why a presenter input did or did not mutate lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenterDisposition {
    Applied,
    IgnoredStaleGeneration,
    IgnoredDuplicateOrOutOfOrderGeometry,
    IgnoredNoChange,
    Unsupported,
}

/// Result of reducing one presenter input.
#[derive(Debug, Clone, PartialEq)]
pub struct PresenterTransition {
    pub disposition: PresenterDisposition,
    pub effects: Vec<PresenterEffect>,
}

impl PresenterTransition {
    fn new(
        disposition: PresenterDisposition,
        effects: Vec<PresenterEffect>,
    ) -> Self {
        Self {
            disposition,
            effects,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeometryAcceptance {
    Changed,
    Equivalent,
    Stale,
}

/// Deterministic host/VO readiness, geometry, visibility, and teardown model.
#[derive(Debug, Clone)]
pub struct PresenterLifecycle {
    identity: PresenterIdentity,
    source_target: PlaybackTarget,
    fallback_target: PlaybackTarget,
    capabilities: PresenterCapabilities,
    state: PresenterState,
    host_ready: bool,
    video_output_ready: bool,
    attached: bool,
    attached_once: bool,
    explicitly_detached: bool,
    geometry: Option<SurfaceGeometry>,
    host_visible: bool,
    suspended: bool,
    commanded_visible: Option<bool>,
    commanded_suspended: Option<bool>,
    actual_fullscreen: bool,
    requested_fullscreen: Option<bool>,
    commanded_fullscreen: Option<bool>,
    initial_host_visible: bool,
    failed: bool,
}

impl PresenterLifecycle {
    /// Create one presenter attempt with an explicit deterministic fallback.
    pub fn new(
        identity: PresenterIdentity,
        source_target: PlaybackTarget,
        fallback_target: PlaybackTarget,
        capabilities: PresenterCapabilities,
        initial_host_visible: bool,
    ) -> Self {
        Self {
            identity,
            source_target,
            fallback_target,
            capabilities,
            state: PresenterState::Detached,
            host_ready: false,
            video_output_ready: false,
            attached: false,
            attached_once: false,
            explicitly_detached: false,
            geometry: None,
            host_visible: initial_host_visible,
            suspended: false,
            commanded_visible: None,
            commanded_suspended: None,
            actual_fullscreen: false,
            requested_fullscreen: None,
            commanded_fullscreen: None,
            initial_host_visible,
            failed: false,
        }
    }

    pub const fn identity(&self) -> PresenterIdentity {
        self.identity
    }

    pub const fn state(&self) -> PresenterState {
        self.state
    }

    pub const fn geometry(&self) -> Option<SurfaceGeometry> {
        self.geometry
    }

    pub const fn actual_fullscreen(&self) -> bool {
        self.actual_fullscreen
    }

    pub const fn requested_fullscreen(&self) -> Option<bool> {
        self.requested_fullscreen
    }

    #[cfg(all(
        feature = "mpv",
        feature = "ui",
        any(target_os = "windows", target_os = "macos", test)
    ))]
    pub(crate) const fn readiness(&self) -> (bool, bool, bool) {
        (self.host_ready, self.video_output_ready, self.attached)
    }

    pub fn capabilities(&self) -> &PresenterCapabilities {
        &self.capabilities
    }

    /// Detach an old host and begin a strictly newer attachment generation.
    pub fn begin_generation(
        &mut self,
        identity: PresenterIdentity,
        capabilities: PresenterCapabilities,
    ) -> PresenterTransition {
        if identity <= self.identity {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredStaleGeneration,
                Vec::new(),
            );
        }

        let old_identity = self.identity;
        let old_state = self.state;
        let mut effects = Vec::new();
        if self.attached {
            effects.push(PresenterEffect::Command(PresenterCommand::Detach {
                identity: old_identity,
            }));
        }
        self.clear_geometry(&mut effects);

        self.identity = identity;
        self.capabilities = capabilities;
        self.state = PresenterState::Detached;
        self.host_ready = false;
        self.video_output_ready = false;
        self.attached = false;
        self.attached_once = false;
        self.explicitly_detached = false;
        self.geometry = None;
        self.host_visible = self.initial_host_visible;
        self.suspended = false;
        self.commanded_visible = None;
        self.commanded_suspended = None;
        self.actual_fullscreen = false;
        self.requested_fullscreen = None;
        self.commanded_fullscreen = None;
        self.failed = false;

        if old_state != PresenterState::Detached {
            effects.push(PresenterEffect::Event(PresenterEvent::StateChanged(
                PresenterState::Detached,
            )));
        }
        PresenterTransition::new(PresenterDisposition::Applied, effects)
    }

    /// Reduce one generation-scoped lifecycle input.
    pub fn handle(
        &mut self,
        envelope: PresenterInputEnvelope,
    ) -> PresenterTransition {
        if envelope.identity != self.identity {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredStaleGeneration,
                Vec::new(),
            );
        }
        if self.failed {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredNoChange,
                Vec::new(),
            );
        }

        match envelope.input {
            PresenterInput::HostReady { geometry } => {
                self.handle_host_ready(geometry)
            }
            PresenterInput::VideoOutputReady => {
                if self.video_output_ready {
                    return PresenterTransition::new(
                        PresenterDisposition::IgnoredNoChange,
                        Vec::new(),
                    );
                }
                self.video_output_ready = true;
                let mut effects = Vec::new();
                self.reconcile(&mut effects);
                PresenterTransition::new(PresenterDisposition::Applied, effects)
            }
            PresenterInput::GeometryChanged(geometry) => {
                self.handle_geometry(geometry)
            }
            PresenterInput::HostVisibilityChanged(visible) => {
                if self.host_visible == visible {
                    return PresenterTransition::new(
                        PresenterDisposition::IgnoredNoChange,
                        Vec::new(),
                    );
                }
                self.host_visible = visible;
                let mut effects = Vec::new();
                self.reconcile(&mut effects);
                PresenterTransition::new(PresenterDisposition::Applied, effects)
            }
            PresenterInput::SuspensionChanged(suspended) => {
                if self.suspended == suspended {
                    return PresenterTransition::new(
                        PresenterDisposition::IgnoredNoChange,
                        Vec::new(),
                    );
                }
                self.suspended = suspended;
                let mut effects = Vec::new();
                self.reconcile(&mut effects);
                PresenterTransition::new(PresenterDisposition::Applied, effects)
            }
            PresenterInput::FullscreenRequested(fullscreen) => {
                self.request_fullscreen(fullscreen)
            }
            PresenterInput::FullscreenConfirmed(fullscreen) => {
                self.confirm_fullscreen(fullscreen)
            }
            PresenterInput::HostLost => self.lose_host(),
            PresenterInput::VideoOutputLost => self.lose_video_output(),
            PresenterInput::Detach => self.detach(),
            PresenterInput::Failed(error) => self.fail(error),
        }
    }

    fn handle_host_ready(
        &mut self,
        geometry: SurfaceGeometry,
    ) -> PresenterTransition {
        let was_ready = self.host_ready;
        let acceptance = match self.accept_geometry(geometry) {
            Ok(acceptance) => acceptance,
            Err(error) => return self.fail(error),
        };
        self.host_ready = true;

        let mut effects = Vec::new();
        if acceptance == GeometryAcceptance::Changed {
            effects.push(PresenterEffect::Event(
                PresenterEvent::GeometryChanged(Some(geometry)),
            ));
            if self.attached {
                effects.push(PresenterEffect::Command(
                    PresenterCommand::Synchronize {
                        identity: self.identity,
                        geometry,
                    },
                ));
            }
        }
        self.reconcile(&mut effects);

        let disposition = if acceptance == GeometryAcceptance::Stale
            && was_ready
            && effects.is_empty()
        {
            PresenterDisposition::IgnoredDuplicateOrOutOfOrderGeometry
        } else if was_ready
            && acceptance == GeometryAcceptance::Equivalent
            && effects.is_empty()
        {
            PresenterDisposition::IgnoredNoChange
        } else {
            PresenterDisposition::Applied
        };
        PresenterTransition::new(disposition, effects)
    }

    fn handle_geometry(
        &mut self,
        geometry: SurfaceGeometry,
    ) -> PresenterTransition {
        let acceptance = match self.accept_geometry(geometry) {
            Ok(acceptance) => acceptance,
            Err(error) => return self.fail(error),
        };
        if acceptance == GeometryAcceptance::Stale {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredDuplicateOrOutOfOrderGeometry,
                Vec::new(),
            );
        }

        let mut effects = Vec::new();
        if acceptance == GeometryAcceptance::Changed {
            effects.push(PresenterEffect::Event(
                PresenterEvent::GeometryChanged(Some(geometry)),
            ));
            if self.attached {
                effects.push(PresenterEffect::Command(
                    PresenterCommand::Synchronize {
                        identity: self.identity,
                        geometry,
                    },
                ));
            }
        }
        self.reconcile(&mut effects);
        PresenterTransition::new(
            if acceptance == GeometryAcceptance::Equivalent
                && effects.is_empty()
            {
                PresenterDisposition::IgnoredNoChange
            } else {
                PresenterDisposition::Applied
            },
            effects,
        )
    }

    fn accept_geometry(
        &mut self,
        geometry: SurfaceGeometry,
    ) -> Result<GeometryAcceptance, PlaybackError> {
        geometry.validate().map_err(|error| {
            self.presenter_error(format!(
                "native presenter rejected host geometry: {error}"
            ))
        })?;

        let Some(previous) = self.geometry else {
            self.geometry = Some(geometry);
            return Ok(GeometryAcceptance::Changed);
        };
        if geometry.revision <= previous.revision {
            return Ok(GeometryAcceptance::Stale);
        }

        let acceptance = if previous.same_layout(geometry) {
            GeometryAcceptance::Equivalent
        } else {
            GeometryAcceptance::Changed
        };
        self.geometry = Some(geometry);
        Ok(acceptance)
    }

    fn request_fullscreen(&mut self, fullscreen: bool) -> PresenterTransition {
        let Some(_) = self.capabilities.fullscreen_owner else {
            return PresenterTransition::new(
                PresenterDisposition::Unsupported,
                Vec::new(),
            );
        };
        if self.requested_fullscreen == Some(fullscreen)
            || (self.requested_fullscreen.is_none()
                && self.actual_fullscreen == fullscreen)
        {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredNoChange,
                Vec::new(),
            );
        }

        self.requested_fullscreen = Some(fullscreen);
        let mut effects = Vec::new();
        self.reconcile_fullscreen(&mut effects);
        PresenterTransition::new(PresenterDisposition::Applied, effects)
    }

    fn confirm_fullscreen(&mut self, fullscreen: bool) -> PresenterTransition {
        if self.actual_fullscreen == fullscreen
            && self.requested_fullscreen.is_none()
        {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredNoChange,
                Vec::new(),
            );
        }

        let actual_changed = self.actual_fullscreen != fullscreen;
        let pending_matches_confirmation =
            self.requested_fullscreen == Some(fullscreen);
        let had_no_pending_request = self.requested_fullscreen.is_none();
        self.actual_fullscreen = fullscreen;
        if pending_matches_confirmation || had_no_pending_request {
            self.requested_fullscreen = None;
            self.commanded_fullscreen = None;
        }

        let mut effects = Vec::new();
        if actual_changed {
            effects.push(PresenterEffect::Event(
                PresenterEvent::FullscreenChanged(fullscreen),
            ));
        }
        // An initial or stale opposite property observation must not consume a
        // request made before attach. Once attached, reconcile it immediately;
        // otherwise the ordinary host/VO readiness transition will do so.
        self.reconcile_fullscreen(&mut effects);
        PresenterTransition::new(PresenterDisposition::Applied, effects)
    }

    fn lose_host(&mut self) -> PresenterTransition {
        if !self.host_ready {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredNoChange,
                Vec::new(),
            );
        }
        self.host_ready = false;
        let mut effects = Vec::new();
        self.detach_platform(&mut effects);
        self.clear_geometry(&mut effects);
        self.reconcile(&mut effects);
        PresenterTransition::new(PresenterDisposition::Applied, effects)
    }

    fn lose_video_output(&mut self) -> PresenterTransition {
        if !self.video_output_ready {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredNoChange,
                Vec::new(),
            );
        }
        self.video_output_ready = false;
        let mut effects = Vec::new();
        self.detach_platform(&mut effects);
        self.reconcile(&mut effects);
        PresenterTransition::new(PresenterDisposition::Applied, effects)
    }

    fn detach(&mut self) -> PresenterTransition {
        if self.explicitly_detached && self.state == PresenterState::Detached {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredNoChange,
                Vec::new(),
            );
        }

        let mut effects = Vec::new();
        self.detach_platform(&mut effects);
        self.clear_geometry(&mut effects);
        self.host_ready = false;
        self.video_output_ready = false;
        self.explicitly_detached = true;
        self.requested_fullscreen = None;
        self.commanded_fullscreen = None;
        self.set_state(PresenterState::Detached, &mut effects);
        PresenterTransition::new(PresenterDisposition::Applied, effects)
    }

    fn fail(&mut self, mut error: PlaybackError) -> PresenterTransition {
        if self.failed {
            return PresenterTransition::new(
                PresenterDisposition::IgnoredNoChange,
                Vec::new(),
            );
        }

        error.kind = PlaybackErrorKind::Presenter;
        error.backend.get_or_insert(self.source_target.backend);
        error.recoverable = true;

        let mut effects = Vec::new();
        self.detach_platform(&mut effects);
        self.clear_geometry(&mut effects);
        self.failed = true;
        self.state = PresenterState::Failed;
        let reason = FallbackReason {
            code: FallbackReasonCode::PresenterFailed,
            from: Some(self.source_target),
            to: self.fallback_target,
            detail: error.message.clone(),
        };
        effects.push(PresenterEffect::Event(PresenterEvent::Failure(error)));
        effects.push(PresenterEffect::Event(
            PresenterEvent::FallbackRequested(reason),
        ));
        PresenterTransition::new(PresenterDisposition::Applied, effects)
    }

    fn reconcile(&mut self, effects: &mut Vec<PresenterEffect>) {
        if self.failed {
            return;
        }

        if !self.host_ready || !self.video_output_ready {
            let state = match (self.host_ready, self.video_output_ready) {
                (true, false) => PresenterState::AwaitingVideoOutput,
                (false, true) => PresenterState::AwaitingHost,
                (false, false) => PresenterState::Detached,
                (true, true) => unreachable!(),
            };
            self.set_state(state, effects);
            return;
        }

        if !self.attached {
            if self.attached_once || self.explicitly_detached {
                let error = self.presenter_error(
                    "presenter host or video output was recreated without a new generation",
                );
                self.append_failure(error, effects);
                return;
            }

            self.attached = true;
            self.attached_once = true;
            self.commanded_suspended = (!self.suspended).then_some(false);
            effects.push(PresenterEffect::Command(PresenterCommand::Attach {
                identity: self.identity,
            }));
            if let Some(geometry) = self.geometry {
                effects.push(PresenterEffect::Command(
                    PresenterCommand::Synchronize {
                        identity: self.identity,
                        geometry,
                    },
                ));
            }
        }

        self.reconcile_suspension_and_visibility(effects);
        self.reconcile_fullscreen(effects);
    }

    fn reconcile_suspension_and_visibility(
        &mut self,
        effects: &mut Vec<PresenterEffect>,
    ) {
        if !self.attached {
            return;
        }

        if self.commanded_suspended != Some(self.suspended) {
            effects.push(PresenterEffect::Command(
                PresenterCommand::SetSuspended {
                    identity: self.identity,
                    suspended: self.suspended,
                },
            ));
            self.commanded_suspended = Some(self.suspended);
        }

        let visible = !self.suspended
            && self.host_visible
            && self.geometry.is_some_and(SurfaceGeometry::is_visible);
        if self.commanded_visible != Some(visible) {
            effects.push(PresenterEffect::Command(
                PresenterCommand::SetVisible {
                    identity: self.identity,
                    visible,
                },
            ));
            self.commanded_visible = Some(visible);
        }

        let state = if self.suspended {
            PresenterState::Suspended
        } else if visible {
            PresenterState::Attached
        } else {
            PresenterState::Hidden
        };
        self.set_state(state, effects);
    }

    fn reconcile_fullscreen(&mut self, effects: &mut Vec<PresenterEffect>) {
        if !self.attached {
            return;
        }
        let (Some(fullscreen), Some(owner)) = (
            self.requested_fullscreen,
            self.capabilities.fullscreen_owner,
        ) else {
            return;
        };
        if self.commanded_fullscreen == Some(fullscreen) {
            return;
        }

        effects.push(PresenterEffect::Command(
            PresenterCommand::SetFullscreen {
                identity: self.identity,
                owner,
                fullscreen,
            },
        ));
        self.commanded_fullscreen = Some(fullscreen);
    }

    fn detach_platform(&mut self, effects: &mut Vec<PresenterEffect>) {
        if self.attached {
            effects.push(PresenterEffect::Command(PresenterCommand::Detach {
                identity: self.identity,
            }));
        }
        self.attached = false;
        self.commanded_visible = None;
        self.commanded_suspended = None;
        self.commanded_fullscreen = None;
    }

    fn clear_geometry(&mut self, effects: &mut Vec<PresenterEffect>) {
        if self.geometry.take().is_some() {
            effects.push(PresenterEffect::Event(
                PresenterEvent::GeometryChanged(None),
            ));
        }
    }

    fn set_state(
        &mut self,
        state: PresenterState,
        effects: &mut Vec<PresenterEffect>,
    ) {
        if self.state == state {
            return;
        }
        self.state = state;
        effects
            .push(PresenterEffect::Event(PresenterEvent::StateChanged(state)));
    }

    fn append_failure(
        &mut self,
        mut error: PlaybackError,
        effects: &mut Vec<PresenterEffect>,
    ) {
        error.kind = PlaybackErrorKind::Presenter;
        error.backend.get_or_insert(self.source_target.backend);
        error.recoverable = true;
        self.detach_platform(effects);
        self.clear_geometry(effects);
        self.failed = true;
        self.state = PresenterState::Failed;
        let reason = FallbackReason {
            code: FallbackReasonCode::PresenterFailed,
            from: Some(self.source_target),
            to: self.fallback_target,
            detail: error.message.clone(),
        };
        effects.push(PresenterEffect::Event(PresenterEvent::Failure(error)));
        effects.push(PresenterEffect::Event(
            PresenterEvent::FallbackRequested(reason),
        ));
    }

    fn presenter_error(&self, message: impl Into<String>) -> PlaybackError {
        let mut error =
            PlaybackError::new(PlaybackErrorKind::Presenter, message);
        error.backend = Some(match self.source_target.backend {
            BackendKind::GStreamer => BackendKind::GStreamer,
            BackendKind::Mpv => BackendKind::Mpv,
            BackendKind::ExternalMpv => BackendKind::ExternalMpv,
        });
        error.recoverable = true;
        error
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    fn identity(presenter: u64) -> PresenterIdentity {
        PresenterIdentity::new(
            SessionGeneration::new(7),
            PresenterGeneration::new(presenter),
        )
    }

    fn capabilities() -> PresenterCapabilities {
        PresenterCapabilities {
            integrated_overlay: true,
            native_window_fallback: true,
            fullscreen_owner: Some(FullscreenOwner::VideoOutput),
            ..PresenterCapabilities::default()
        }
    }

    fn geometry(revision: u64) -> SurfaceGeometry {
        SurfaceGeometry::new(
            GeometryRevision::new(revision),
            LogicalRect::new(10.0, 20.0, 1280.0, 720.0),
            Some(LogicalRect::new(10.0, 20.0, 1280.0, 720.0)),
            1.0,
        )
    }

    fn lifecycle() -> PresenterLifecycle {
        PresenterLifecycle::new(
            identity(1),
            PlaybackTarget::MPV_INTEGRATED,
            PlaybackTarget::MPV_NATIVE_WINDOW,
            capabilities(),
            true,
        )
    }

    fn input(
        lifecycle: &mut PresenterLifecycle,
        input: PresenterInput,
    ) -> PresenterTransition {
        lifecycle
            .handle(PresenterInputEnvelope::new(lifecycle.identity(), input))
    }

    fn command_count(
        transition: &PresenterTransition,
        predicate: impl Fn(&PresenterCommand) -> bool,
    ) -> usize {
        transition
            .effects
            .iter()
            .filter(|effect| {
                matches!(effect, PresenterEffect::Command(command) if predicate(command))
            })
            .count()
    }

    fn geometry_events(
        transition: &PresenterTransition,
    ) -> Vec<Option<SurfaceGeometry>> {
        transition
            .effects
            .iter()
            .filter_map(|effect| match effect {
                PresenterEffect::Event(PresenterEvent::GeometryChanged(
                    geometry,
                )) => Some(*geometry),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn host_and_video_output_can_arrive_in_either_order_and_attach_once() {
        let mut host_first = lifecycle();
        let host = input(
            &mut host_first,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        assert_eq!(host_first.state(), PresenterState::AwaitingVideoOutput);
        assert_eq!(geometry_events(&host), vec![Some(geometry(1))]);
        assert_eq!(
            command_count(&host, |command| matches!(
                command,
                PresenterCommand::Attach { .. }
            )),
            0
        );
        let video = input(&mut host_first, PresenterInput::VideoOutputReady);
        assert_eq!(host_first.state(), PresenterState::Attached);
        assert_eq!(
            command_count(&video, |command| matches!(
                command,
                PresenterCommand::Attach { .. }
            )),
            1
        );
        let duplicate =
            input(&mut host_first, PresenterInput::VideoOutputReady);
        assert_eq!(
            duplicate.disposition,
            PresenterDisposition::IgnoredNoChange
        );

        let mut video_first = lifecycle();
        input(&mut video_first, PresenterInput::VideoOutputReady);
        assert_eq!(video_first.state(), PresenterState::AwaitingHost);
        let host = input(
            &mut video_first,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        assert_eq!(video_first.state(), PresenterState::Attached);
        assert_eq!(
            command_count(&host, |command| matches!(
                command,
                PresenterCommand::Attach { .. }
            )),
            1
        );
    }

    #[test]
    fn initially_hidden_host_attaches_without_becoming_visible_until_handoff() {
        let mut lifecycle = PresenterLifecycle::new(
            identity(1),
            PlaybackTarget::MPV_INTEGRATED,
            PlaybackTarget::MPV_NATIVE_WINDOW,
            capabilities(),
            false,
        );
        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        let attached = input(&mut lifecycle, PresenterInput::VideoOutputReady);

        assert_eq!(lifecycle.state(), PresenterState::Hidden);
        assert_eq!(
            command_count(&attached, |command| matches!(
                command,
                PresenterCommand::SetVisible { visible: false, .. }
            )),
            1
        );
        assert_eq!(
            command_count(&attached, |command| matches!(
                command,
                PresenterCommand::SetVisible { visible: true, .. }
            )),
            0
        );

        let shown =
            input(&mut lifecycle, PresenterInput::HostVisibilityChanged(true));
        assert_eq!(lifecycle.state(), PresenterState::Attached);
        assert_eq!(
            command_count(&shown, |command| matches!(
                command,
                PresenterCommand::SetVisible { visible: true, .. }
            )),
            1
        );
    }

    #[test]
    fn visibility_handoff_survives_transient_suspension() {
        let mut lifecycle = PresenterLifecycle::new(
            identity(1),
            PlaybackTarget::MPV_INTEGRATED,
            PlaybackTarget::MPV_NATIVE_WINDOW,
            capabilities(),
            false,
        );
        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        input(&mut lifecycle, PresenterInput::VideoOutputReady);
        input(&mut lifecycle, PresenterInput::SuspensionChanged(true));

        let handoff =
            input(&mut lifecycle, PresenterInput::HostVisibilityChanged(true));
        assert_eq!(lifecycle.state(), PresenterState::Suspended);
        assert_eq!(
            command_count(&handoff, |command| matches!(
                command,
                PresenterCommand::SetVisible { visible: true, .. }
            )),
            0
        );

        let resumed =
            input(&mut lifecycle, PresenterInput::SuspensionChanged(false));
        assert_eq!(lifecycle.state(), PresenterState::Attached);
        assert_eq!(
            command_count(&resumed, |command| matches!(
                command,
                PresenterCommand::SetVisible { visible: true, .. }
            )),
            1
        );
    }

    #[test]
    fn duplicate_geometry_is_suppressed_but_scale_changes_are_synchronized() {
        let mut lifecycle = lifecycle();
        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        input(&mut lifecycle, PresenterInput::VideoOutputReady);

        let mut scaled = geometry(2);
        scaled.scale_factor = 1.5;
        let transition =
            input(&mut lifecycle, PresenterInput::GeometryChanged(scaled));
        assert_eq!(
            command_count(&transition, |command| matches!(
                command,
                PresenterCommand::Synchronize { .. }
            )),
            1
        );
        assert_eq!(geometry_events(&transition), vec![Some(scaled)]);

        let stale =
            input(&mut lifecycle, PresenterInput::GeometryChanged(scaled));
        assert_eq!(
            stale.disposition,
            PresenterDisposition::IgnoredDuplicateOrOutOfOrderGeometry
        );
        assert!(stale.effects.is_empty());

        let mut equivalent = scaled;
        equivalent.revision = GeometryRevision::new(3);
        let equivalent =
            input(&mut lifecycle, PresenterInput::GeometryChanged(equivalent));
        assert_eq!(
            equivalent.disposition,
            PresenterDisposition::IgnoredNoChange
        );
        assert_eq!(
            command_count(&equivalent, |command| matches!(
                command,
                PresenterCommand::Synchronize { .. }
            )),
            0
        );
    }

    #[test]
    fn clipping_zero_size_visibility_and_suspension_are_explicit() {
        let mut lifecycle = lifecycle();
        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        input(&mut lifecycle, PresenterInput::VideoOutputReady);

        let mut clipped = geometry(2);
        clipped.visible_bounds = None;
        let hidden =
            input(&mut lifecycle, PresenterInput::GeometryChanged(clipped));
        assert_eq!(lifecycle.state(), PresenterState::Hidden);
        assert_eq!(
            command_count(&hidden, |command| matches!(
                command,
                PresenterCommand::SetVisible { visible: false, .. }
            )),
            1
        );

        let shown =
            input(&mut lifecycle, PresenterInput::GeometryChanged(geometry(3)));
        assert_eq!(lifecycle.state(), PresenterState::Attached);
        assert_eq!(
            command_count(&shown, |command| matches!(
                command,
                PresenterCommand::SetVisible { visible: true, .. }
            )),
            1
        );

        let mut zero_sized = geometry(4);
        zero_sized.logical_bounds.width = 0.0;
        zero_sized.visible_bounds =
            Some(LogicalRect::new(10.0, 20.0, 0.0, 720.0));
        input(&mut lifecycle, PresenterInput::GeometryChanged(zero_sized));
        assert_eq!(lifecycle.state(), PresenterState::Hidden);

        input(&mut lifecycle, PresenterInput::GeometryChanged(geometry(5)));
        assert_eq!(lifecycle.state(), PresenterState::Attached);

        let suspended =
            input(&mut lifecycle, PresenterInput::SuspensionChanged(true));
        assert_eq!(lifecycle.state(), PresenterState::Suspended);
        assert_eq!(
            command_count(&suspended, |command| matches!(
                command,
                PresenterCommand::SetSuspended {
                    suspended: true,
                    ..
                }
            )),
            1
        );
        assert_eq!(
            command_count(&suspended, |command| matches!(
                command,
                PresenterCommand::SetVisible { visible: false, .. }
            )),
            1
        );
    }

    #[test]
    fn window_recreation_requires_a_new_generation_and_rejects_stale_events() {
        let mut lifecycle = lifecycle();
        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        input(&mut lifecycle, PresenterInput::VideoOutputReady);

        let replacement =
            lifecycle.begin_generation(identity(2), capabilities());
        assert_eq!(
            command_count(&replacement, |command| matches!(
                command,
                PresenterCommand::Detach {
                    identity: detached,
                } if *detached == identity(1)
            )),
            1
        );
        assert_eq!(geometry_events(&replacement), vec![None]);
        assert_eq!(lifecycle.state(), PresenterState::Detached);

        let stale = lifecycle.handle(PresenterInputEnvelope::new(
            identity(1),
            PresenterInput::GeometryChanged(geometry(2)),
        ));
        assert_eq!(
            stale.disposition,
            PresenterDisposition::IgnoredStaleGeneration
        );
        assert!(stale.effects.is_empty());

        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        let attached = input(&mut lifecycle, PresenterInput::VideoOutputReady);
        assert_eq!(
            command_count(&attached, |command| matches!(
                command,
                PresenterCommand::Attach {
                    identity: attached,
                } if *attached == identity(2)
            )),
            1
        );
    }

    #[test]
    fn fullscreen_changes_only_after_native_confirmation() {
        let mut lifecycle = lifecycle();
        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        input(&mut lifecycle, PresenterInput::VideoOutputReady);

        let requested =
            input(&mut lifecycle, PresenterInput::FullscreenRequested(true));
        assert!(!lifecycle.actual_fullscreen());
        assert_eq!(lifecycle.requested_fullscreen(), Some(true));
        assert_eq!(
            command_count(&requested, |command| matches!(
                command,
                PresenterCommand::SetFullscreen {
                    owner: FullscreenOwner::VideoOutput,
                    fullscreen: true,
                    ..
                }
            )),
            1
        );
        assert!(!requested.effects.iter().any(|effect| matches!(
            effect,
            PresenterEffect::Event(PresenterEvent::FullscreenChanged(_))
        )));

        let confirmed =
            input(&mut lifecycle, PresenterInput::FullscreenConfirmed(true));
        assert!(lifecycle.actual_fullscreen());
        assert_eq!(lifecycle.requested_fullscreen(), None);
        assert!(confirmed.effects.contains(&PresenterEffect::Event(
            PresenterEvent::FullscreenChanged(true)
        )));
    }

    #[test]
    fn opposite_initial_fullscreen_observation_preserves_pre_attach_request() {
        let mut lifecycle = lifecycle();
        let requested =
            input(&mut lifecycle, PresenterInput::FullscreenRequested(true));
        assert!(requested.effects.is_empty());
        assert_eq!(lifecycle.requested_fullscreen(), Some(true));

        let initial =
            input(&mut lifecycle, PresenterInput::FullscreenConfirmed(false));
        assert!(initial.effects.is_empty());
        assert_eq!(lifecycle.requested_fullscreen(), Some(true));

        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        let attached = input(&mut lifecycle, PresenterInput::VideoOutputReady);
        assert_eq!(
            command_count(&attached, |command| matches!(
                command,
                PresenterCommand::SetFullscreen {
                    owner: FullscreenOwner::VideoOutput,
                    fullscreen: true,
                    ..
                }
            )),
            1
        );
        assert_eq!(lifecycle.requested_fullscreen(), Some(true));

        input(&mut lifecycle, PresenterInput::FullscreenConfirmed(true));
        assert_eq!(lifecycle.requested_fullscreen(), None);
        assert!(lifecycle.actual_fullscreen());
    }

    #[test]
    fn presenter_failure_detaches_and_requests_deterministic_fallback() {
        let mut lifecycle = lifecycle();
        input(
            &mut lifecycle,
            PresenterInput::HostReady {
                geometry: geometry(1),
            },
        );
        input(&mut lifecycle, PresenterInput::VideoOutputReady);

        let transition = input(
            &mut lifecycle,
            PresenterInput::Failed(PlaybackError::new(
                PlaybackErrorKind::Protocol,
                "native overlay relationship was lost",
            )),
        );
        assert_eq!(lifecycle.state(), PresenterState::Failed);
        assert_eq!(
            command_count(&transition, |command| matches!(
                command,
                PresenterCommand::Detach { .. }
            )),
            1
        );
        assert_eq!(geometry_events(&transition), vec![None]);
        assert!(transition.effects.iter().any(|effect| matches!(
            effect,
            PresenterEffect::Event(PresenterEvent::Failure(error))
                if error.kind == PlaybackErrorKind::Presenter
                    && error.backend == Some(BackendKind::Mpv)
                    && error.recoverable
        )));
        assert!(transition.effects.iter().any(|effect| matches!(
            effect,
            PresenterEffect::Event(PresenterEvent::FallbackRequested(reason))
                if reason.code == FallbackReasonCode::PresenterFailed
                    && reason.from == Some(PlaybackTarget::MPV_INTEGRATED)
                    && reason.to == PlaybackTarget::MPV_NATIVE_WINDOW
        )));
    }

    struct LocalHost(Rc<()>);

    struct FakeNativePresenter {
        attached: bool,
        detached_before_drop: Rc<Cell<bool>>,
        capabilities: PresenterCapabilities,
    }

    impl NativePresenter for FakeNativePresenter {
        type Host<'host> = &'host LocalHost;

        fn attach(
            &mut self,
            _identity: PresenterIdentity,
            host: Self::Host<'_>,
        ) -> Result<(), PlaybackError> {
            let _ = Rc::strong_count(&host.0);
            self.attached = true;
            self.detached_before_drop.set(false);
            Ok(())
        }

        fn synchronize(
            &mut self,
            _identity: PresenterIdentity,
            _geometry: SurfaceGeometry,
        ) -> Result<(), PlaybackError> {
            Ok(())
        }

        fn set_visible(
            &mut self,
            _identity: PresenterIdentity,
            _visible: bool,
        ) -> Result<(), PlaybackError> {
            Ok(())
        }

        fn set_suspended(
            &mut self,
            _identity: PresenterIdentity,
            _suspended: bool,
        ) -> Result<(), PlaybackError> {
            Ok(())
        }

        fn set_fullscreen(
            &mut self,
            _identity: PresenterIdentity,
            _owner: FullscreenOwner,
            _fullscreen: bool,
        ) -> Result<(), PlaybackError> {
            Ok(())
        }

        fn detach(&mut self, _identity: PresenterIdentity) {
            self.attached = false;
            self.detached_before_drop.set(true);
        }

        fn capabilities(&self) -> &PresenterCapabilities {
            &self.capabilities
        }
    }

    impl Drop for FakeNativePresenter {
        fn drop(&mut self) {
            assert!(!self.attached, "presenter dropped while still attached");
        }
    }

    #[test]
    fn local_only_host_is_explicitly_detached_before_presenter_drop() {
        let detached = Rc::new(Cell::new(false));
        let host = LocalHost(Rc::new(()));
        let mut presenter = FakeNativePresenter {
            attached: false,
            detached_before_drop: Rc::clone(&detached),
            capabilities: capabilities(),
        };

        presenter.attach(identity(1), &host).unwrap();
        assert!(presenter.attached);
        presenter.detach(identity(1));
        assert!(detached.get());
        drop(presenter);
    }
}
