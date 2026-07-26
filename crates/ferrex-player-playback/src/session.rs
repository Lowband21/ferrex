//! Backend-neutral playback session handle used by player state and views.

use std::time::Duration;

#[cfg(feature = "mpv")]
use std::ops::{Deref, DerefMut};

#[cfg(all(
    feature = "mpv",
    feature = "ui",
    any(target_os = "windows", target_os = "macos")
))]
use crate::contract::PresenterEvent;
#[cfg(all(
    feature = "mpv",
    feature = "ui",
    any(target_os = "windows", target_os = "macos")
))]
use crate::native_presentation::NativePresentation;
use crate::{
    contract::{
        BackendRequest, ChapterId, EditionId, PlaybackCommand, PlaybackError,
        PlaybackEventSignal, PlaybackFilePath, PlaybackScreenshotMode,
        PlaybackSnapshot, TrackCatalog, TrackId, VideoProfileName,
    },
    diagnostics::PlaybackDiagnosticSnapshot,
    subwave_adapter::SubwavePlaybackAdapter,
};
#[cfg(feature = "mpv")]
use crate::{
    contract::{FallbackReason, PlaybackErrorKind, PlaybackTarget},
    mpv_adapter::MpvPlaybackAdapter,
};

/// Active in-process playback session.
///
/// Backend variants remain private so domain and view code cannot branch on a
/// concrete engine. Capability and target differences are read from
/// [`PlaybackSnapshot`].
pub struct PlaybackSession {
    requested_backend: BackendRequest,
    backend: BackendSession,
}

/// Completion gate for playback-root teardown that must finish before another
/// top-level application window may be revealed.
pub(crate) struct PlaybackShutdownBarrier {
    completion: tokio::sync::oneshot::Receiver<Result<(), String>>,
}

impl std::fmt::Debug for PlaybackShutdownBarrier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlaybackShutdownBarrier")
            .finish_non_exhaustive()
    }
}

impl PlaybackShutdownBarrier {
    pub(crate) fn new(
        completion: tokio::sync::oneshot::Receiver<Result<(), String>>,
    ) -> Self {
        Self { completion }
    }

    pub(crate) fn failed(message: String) -> Self {
        let (sender, completion) = tokio::sync::oneshot::channel();
        let _ = sender.send(Err(message));
        Self { completion }
    }

    pub(crate) async fn wait(self) -> Result<(), String> {
        self.completion.await.unwrap_or_else(|_| {
            Err("playback-root teardown completion was lost".to_string())
        })
    }
}

enum BackendSession {
    Subwave(Box<SubwavePlaybackAdapter>),
    #[cfg(feature = "mpv")]
    Mpv(Box<MpvBackendSession>),
}

/// Preflight result that chooses the mpv option set before its worker starts.
/// A live platform presenter is retained here only on supported UI targets.
#[cfg(feature = "mpv")]
pub(crate) struct MpvPresentationPlan {
    target: PlaybackTarget,
    fallback: Option<FallbackReason>,
    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    presentation: Option<NativePresentation>,
}

#[cfg(feature = "mpv")]
impl MpvPresentationPlan {
    pub(crate) const fn target(&self) -> PlaybackTarget {
        self.target
    }
}

#[cfg(feature = "mpv")]
struct MpvBackendSession {
    adapter: Box<MpvPlaybackAdapter>,
    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    presentation: Option<NativePresentation>,
    #[cfg(all(feature = "ui", target_os = "macos"))]
    native_window_fallback_pending: bool,
}

#[cfg(any(
    all(test, feature = "mpv"),
    all(
        feature = "mpv",
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    )
))]
const fn native_presenter_refresh_required(
    presentation_available: bool,
    native_window_fallback_pending: bool,
) -> bool {
    presentation_available || native_window_fallback_pending
}

#[cfg(any(
    all(test, feature = "mpv"),
    all(feature = "mpv", feature = "ui", target_os = "macos")
))]
fn complete_verified_native_window_fallback(
    pending: &mut bool,
    verified: bool,
) -> bool {
    if !*pending || !verified {
        return false;
    }
    *pending = false;
    true
}

#[cfg(feature = "mpv")]
impl Deref for MpvBackendSession {
    type Target = MpvPlaybackAdapter;

    fn deref(&self) -> &Self::Target {
        &self.adapter
    }
}

#[cfg(feature = "mpv")]
impl DerefMut for MpvBackendSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.adapter
    }
}

#[cfg(feature = "mpv")]
impl MpvBackendSession {
    fn new(adapter: MpvPlaybackAdapter, plan: MpvPresentationPlan) -> Self {
        #[cfg(all(
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        let mut adapter = adapter;

        #[cfg(all(
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        let presentation = plan.presentation;

        #[cfg(all(
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        if let Some(presentation) = presentation.as_ref() {
            adapter.configure_integrated_presentation(
                &presentation.capabilities(),
            );
        }

        #[cfg(not(all(
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        )))]
        let _ = plan;

        Self {
            adapter: Box::new(adapter),
            #[cfg(all(
                feature = "ui",
                any(target_os = "windows", target_os = "macos")
            ))]
            presentation,
            #[cfg(all(feature = "ui", target_os = "macos"))]
            native_window_fallback_pending: false,
        }
    }

    #[cfg(target_os = "macos")]
    fn begin_shutdown_barrier(
        &mut self,
    ) -> Result<Option<PlaybackShutdownBarrier>, PlaybackError> {
        // Drain a just-arrived window identity, detach the in-root controls,
        // and then begin owner teardown. The returned completion is the only
        // proof that a root whose identity arrived late can no longer appear.
        self.adapter.poll_events();
        #[cfg(feature = "ui")]
        if let Some(presentation) = self.presentation.as_ref() {
            presentation.detach();
        }
        self.adapter.begin_shutdown_barrier()
    }

    fn apply_command(
        &mut self,
        command: PlaybackCommand,
    ) -> Result<(), PlaybackError> {
        #[cfg(all(feature = "ui", target_os = "macos"))]
        if self.native_window_fallback_pending
            && matches!(
                &command,
                PlaybackCommand::Load(_)
                    | PlaybackCommand::Stop
                    | PlaybackCommand::Shutdown
            )
        {
            // A pending proof belongs only to the still-live failed presenter
            // session. A stop, replacement, or shutdown cannot be credited by
            // a later unrelated mpv root.
            self.native_window_fallback_pending = false;
        }

        #[cfg(all(
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        {
            if matches!(&command, PlaybackCommand::Shutdown) {
                // Drain a just-arrived native window-id before deciding
                // whether an AppKit root must be withdrawn.
                self.adapter.poll_events();
                #[cfg(target_os = "macos")]
                self.adapter.withdraw_native_root_for_shutdown()?;
            }
            if let PlaybackCommand::SetFullscreen(fullscreen) = &command
                && self.presentation.is_some()
                && self.adapter.snapshot().target
                    == crate::contract::PlaybackTarget::MPV_INTEGRATED
            {
                self.presentation
                    .as_ref()
                    .expect("checked")
                    .request_fullscreen(*fullscreen);
                self.drain_presenter_state()?;
                return Ok(());
            }
            if matches!(&command, PlaybackCommand::Load(_))
                && let Some(presentation) = self.presentation.as_ref()
            {
                presentation.begin_media_load();
            }
            if matches!(&command, PlaybackCommand::Shutdown)
                && let Some(presentation) = self.presentation.as_ref()
            {
                presentation.detach();
            }
        }

        self.adapter.apply_command(command)
    }

    fn poll_events(&mut self) {
        self.adapter.poll_events();
        #[cfg(all(
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        if let Err(error) = self.synchronize_presenter() {
            log::warn!("Could not synchronize native presenter: {error}");
        }
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    fn synchronize_presenter(&mut self) -> Result<(), PlaybackError> {
        if let Some(presentation) = self.presentation.as_ref() {
            presentation.synchronize_native_output(
                self.adapter.native_window_id(),
                self.adapter.vo_configured(),
                self.adapter.native_video_output_started(),
                self.adapter.snapshot().fullscreen,
            );
        }
        self.drain_presenter_state()
    }

    #[cfg(all(feature = "ui", target_os = "macos"))]
    fn confirm_native_window_fallback_if_live(&mut self) {
        if !self.native_window_fallback_pending
            || !self.adapter.vo_configured()
            || !self.adapter.native_video_output_started()
        {
            return;
        }
        let Some(native_window_id) = self.adapter.native_window_id() else {
            return;
        };
        match crate::macos_presenter::verify_mpv_native_fallback_window(
            native_window_id,
        ) {
            Ok(verified) => {
                complete_verified_native_window_fallback(
                    &mut self.native_window_fallback_pending,
                    verified,
                );
            }
            Ok(_) => {}
            Err(error) => {
                log::debug!(
                    "mpv native fallback root is not yet verifiable: {error}"
                );
            }
        }
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    fn drain_presenter_state(&mut self) -> Result<(), PlaybackError> {
        // Verification can legitimately lag the failure event by one or more
        // AppKit turns. Keep retrying the pending native root even after the
        // failed integrated presentation has been retired.
        #[cfg(target_os = "macos")]
        self.confirm_native_window_fallback_if_live();

        let Some(presentation) = self.presentation.as_ref() else {
            return Ok(());
        };

        if let Some(fullscreen) = presentation.take_fullscreen_request() {
            self.adapter
                .apply_command(PlaybackCommand::SetFullscreen(fullscreen))?;
        }

        let events = presentation.drain_events();
        let mut fallback = false;
        #[cfg(target_os = "macos")]
        let mut native_window_fallback = false;
        for event in events {
            let fallback_reason = match &event {
                PresenterEvent::FallbackRequested(reason) => {
                    Some(reason.clone())
                }
                _ => None,
            };
            self.adapter
                .record_event(crate::contract::PlaybackEvent::Presenter(event));
            if let Some(reason) = fallback_reason {
                if reason.to == PlaybackTarget::MPV_NATIVE_WINDOW {
                    self.adapter.begin_native_window_fallback(reason);
                    #[cfg(target_os = "macos")]
                    {
                        native_window_fallback = true;
                    }
                } else {
                    self.adapter.record_event(
                        crate::contract::PlaybackEvent::Fallback(reason),
                    );
                }
                fallback = true;
            }
        }
        if fallback {
            presentation.detach();
            // Detach after failure is a terminal lifecycle transition. Drain
            // it before dropping the presentation so the normal playback
            // projection and macOS runtime evidence both record a fully
            // retired integrated presenter.
            for event in presentation.drain_events() {
                self.adapter.record_event(
                    crate::contract::PlaybackEvent::Presenter(event),
                );
            }
            self.presentation = None;
            #[cfg(target_os = "macos")]
            if native_window_fallback {
                self.native_window_fallback_pending = true;
            }
        }
        #[cfg(target_os = "macos")]
        self.confirm_native_window_fallback_if_live();
        Ok(())
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    fn fail_host_capture(
        &mut self,
        window_id: iced::window::Id,
        detail: String,
    ) {
        if let Some(presentation) = self.presentation.as_ref() {
            presentation.fail_host_capture(window_id, detail);
            let _ = self.drain_presenter_state();
        }
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    fn refresh_presenter(&mut self) -> Result<(), PlaybackError> {
        if let Some(presentation) = self.presentation.as_ref() {
            presentation.refresh_platform_window();
        }
        self.drain_presenter_state()
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    fn set_presenter_host_visible(
        &mut self,
        visible: bool,
    ) -> Result<bool, PlaybackError> {
        let Some(presentation) = self.presentation.as_ref() else {
            return Ok(false);
        };
        presentation.set_host_visible(visible);
        self.drain_presenter_state()?;
        Ok(self.presentation.is_some()
            && self.adapter.snapshot().target == PlaybackTarget::MPV_INTEGRATED
            && (!visible
                || matches!(
                    self.adapter.snapshot().presenter,
                    crate::contract::PresenterState::Attached
                        | crate::contract::PresenterState::Hidden
                        | crate::contract::PresenterState::Suspended
                )))
    }
}

impl std::fmt::Debug for PlaybackSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlaybackSession")
            .field("requested_backend", &self.requested_backend)
            .field("snapshot", self.snapshot())
            .finish_non_exhaustive()
    }
}

impl PlaybackSession {
    /// Resolve integrated presenter availability before libmpv receives its
    /// immutable startup option set. A failed preflight therefore starts the
    /// ordinary native-window path with OSC and native input enabled.
    #[cfg(feature = "mpv")]
    pub(crate) fn preflight_mpv_presentation(
        requested_target: PlaybackTarget,
        generation: crate::contract::SessionGeneration,
    ) -> MpvPresentationPlan {
        if requested_target != PlaybackTarget::MPV_INTEGRATED {
            return MpvPresentationPlan {
                target: requested_target,
                fallback: None,
                #[cfg(all(
                    feature = "ui",
                    any(target_os = "windows", target_os = "macos")
                ))]
                presentation: None,
            };
        }

        #[cfg(all(
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        {
            return match NativePresentation::try_new(generation) {
                Ok(presentation) => MpvPresentationPlan {
                    target: requested_target,
                    fallback: None,
                    presentation: Some(presentation),
                },
                Err(reason) => MpvPresentationPlan {
                    target: PlaybackTarget::MPV_NATIVE_WINDOW,
                    fallback: Some(reason),
                    presentation: None,
                },
            };
        }

        #[cfg(not(all(
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        )))]
        {
            let _ = generation;
            MpvPresentationPlan {
                target: PlaybackTarget::MPV_NATIVE_WINDOW,
                fallback: Some(FallbackReason {
                    code: crate::contract::FallbackReasonCode::UnsupportedPlatform,
                    from: Some(PlaybackTarget::MPV_INTEGRATED),
                    to: PlaybackTarget::MPV_NATIVE_WINDOW,
                    detail: "integrated mpv presentation is unavailable on this UI target"
                        .to_owned(),
                }),
            }
        }
    }

    pub(crate) fn from_subwave(
        adapter: SubwavePlaybackAdapter,
        requested_backend: BackendRequest,
    ) -> Self {
        Self {
            requested_backend,
            backend: BackendSession::Subwave(Box::new(adapter)),
        }
    }

    #[cfg(feature = "mpv")]
    pub(crate) fn from_mpv(
        mut adapter: MpvPlaybackAdapter,
        requested_backend: BackendRequest,
        mut plan: MpvPresentationPlan,
    ) -> Self {
        debug_assert_eq!(adapter.snapshot().target, plan.target);
        if let Some(reason) = plan.fallback.take() {
            adapter.record_fallback(reason);
        }
        Self {
            requested_backend,
            backend: BackendSession::Mpv(Box::new(MpvBackendSession::new(
                adapter, plan,
            ))),
        }
    }

    pub fn snapshot(&self) -> &PlaybackSnapshot {
        match &self.backend {
            BackendSession::Subwave(adapter) => adapter.snapshot(),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => adapter.snapshot(),
        }
    }

    /// Redacted backend, lifecycle, version, and native-output observations.
    pub fn diagnostics(&self) -> PlaybackDiagnosticSnapshot {
        match &self.backend {
            BackendSession::Subwave(adapter) => {
                adapter.diagnostics(self.requested_backend)
            }
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => {
                adapter.diagnostics(self.requested_backend)
            }
        }
    }

    pub fn apply_command(
        &mut self,
        command: PlaybackCommand,
    ) -> Result<(), PlaybackError> {
        match &mut self.backend {
            BackendSession::Subwave(adapter) => adapter.apply_command(command),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => adapter.apply_command(command),
        }
    }

    /// Begin backend shutdown and return a completion gate when native AppKit
    /// teardown must yield to the main run loop.
    pub(crate) fn begin_shutdown_barrier(
        &mut self,
    ) -> Result<Option<PlaybackShutdownBarrier>, PlaybackError> {
        match &mut self.backend {
            BackendSession::Subwave(adapter) => {
                adapter.apply_command(PlaybackCommand::Shutdown)?;
                Ok(None)
            }
            #[cfg(all(feature = "mpv", target_os = "macos"))]
            BackendSession::Mpv(adapter) => adapter.begin_shutdown_barrier(),
            #[cfg(all(feature = "mpv", not(target_os = "macos")))]
            BackendSession::Mpv(adapter) => {
                adapter.apply_command(PlaybackCommand::Shutdown)?;
                Ok(None)
            }
        }
    }

    pub fn synchronize_snapshot(&mut self) {
        match &mut self.backend {
            BackendSession::Subwave(adapter) => {
                adapter.synchronize_core_properties();
            }
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => adapter.poll_events(),
        }
    }

    pub fn refresh_tracks(&mut self) -> TrackCatalog {
        match &mut self.backend {
            BackendSession::Subwave(adapter) => adapter.refresh_tracks(),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => adapter.refresh_tracks(),
        }
    }

    pub fn select_audio_track(
        &mut self,
        track_id: &TrackId,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::SelectAudio(track_id.clone()))
    }

    pub fn select_subtitle_track(
        &mut self,
        track_id: Option<&TrackId>,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::SelectSubtitle(track_id.cloned()))
    }

    /// Add a local sidecar subtitle to the current playback generation.
    /// `select=true` requests immediate selection; `false` leaves selection to
    /// the backend's automatic track policy.
    pub fn add_external_subtitle(
        &mut self,
        source: PlaybackFilePath,
        select: bool,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::AddExternalSubtitle {
            source,
            select,
        })
    }

    pub fn select_chapter(
        &mut self,
        chapter_id: &ChapterId,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::SelectChapter(chapter_id.clone()))
    }

    pub fn select_edition(
        &mut self,
        edition_id: &EditionId,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::SelectEdition(edition_id.clone()))
    }

    pub fn set_subtitles_enabled(&mut self, enabled: bool) {
        match &mut self.backend {
            BackendSession::Subwave(adapter) => {
                adapter.set_subtitles_enabled(enabled);
            }
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => {
                adapter.set_subtitles_enabled(enabled);
            }
        }
    }

    pub fn subtitles_enabled(&self) -> bool {
        match &self.backend {
            BackendSession::Subwave(adapter) => adapter.subtitles_enabled(),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => adapter.subtitles_enabled(),
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        if let Err(error) =
            self.apply_command(PlaybackCommand::SetPaused(paused))
        {
            log::error!("Failed to set playback pause state: {error}");
        }
    }

    pub fn paused(&self) -> bool {
        self.snapshot().is_paused()
    }

    pub fn position(&mut self) -> Duration {
        self.synchronize_snapshot();
        self.snapshot().position
    }

    pub fn duration(&mut self) -> Duration {
        self.synchronize_snapshot();
        self.snapshot().duration.unwrap_or(Duration::ZERO)
    }

    pub fn seek(
        &mut self,
        position: Duration,
        _accurate: bool,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::SeekAbsolute(position))
    }

    pub fn set_volume(&mut self, volume: f64) {
        if let Err(error) =
            self.apply_command(PlaybackCommand::SetVolume(volume))
        {
            log::error!("Failed to set playback volume: {error}");
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        if let Err(error) = self.apply_command(PlaybackCommand::SetMuted(muted))
        {
            log::error!("Failed to set playback mute state: {error}");
        }
    }

    pub fn set_speed(&mut self, speed: f64) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::SetSpeed(speed))
    }

    /// Apply a capability-gated named video profile. User-defined mpv profiles
    /// require the explicit trusted-user configuration policy.
    pub fn apply_video_profile(
        &mut self,
        profile: VideoProfileName,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::ApplyVideoProfile(profile))
    }

    /// Replace the ordered native-video shader chain. Unsupported backends
    /// return [`crate::contract::PlaybackErrorKind::UnsupportedOperation`].
    pub fn set_video_shaders(
        &mut self,
        shaders: Vec<PlaybackFilePath>,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::SetVideoShaders(shaders))
    }

    /// Capture one native-output screenshot at an explicit local destination.
    pub fn capture_screenshot(
        &mut self,
        output: PlaybackFilePath,
        mode: PlaybackScreenshotMode,
    ) -> Result<(), PlaybackError> {
        self.apply_command(PlaybackCommand::CaptureScreenshot { output, mode })
    }

    pub fn has_video(&self) -> bool {
        match &self.backend {
            BackendSession::Subwave(adapter) => adapter.has_video(),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => adapter.has_video(),
        }
    }

    /// Legacy Subwave diagnostic retained during migration. This does not
    /// represent a generic playback capability.
    pub fn is_appsink(&self) -> bool {
        match &self.backend {
            BackendSession::Subwave(adapter) => adapter.is_appsink(),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(_) => false,
        }
    }

    pub fn uses_wayland_surface(&self) -> bool {
        match &self.backend {
            BackendSession::Subwave(adapter) => adapter.uses_wayland_surface(),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(_) => false,
        }
    }

    pub fn toggle_diagnostic_backend(&mut self) -> Result<(), PlaybackError> {
        match &mut self.backend {
            BackendSession::Subwave(adapter) => {
                adapter.toggle_diagnostic_backend()
            }
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(_) => Err(PlaybackError::new(
                PlaybackErrorKind::Command,
                "Subwave diagnostic backend toggle is unavailable for mpv",
            )),
        }
    }

    pub fn force_appsink(&mut self) -> Result<(), PlaybackError> {
        match &mut self.backend {
            BackendSession::Subwave(adapter) => adapter.force_appsink(),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(_) => Err(PlaybackError::new(
                PlaybackErrorKind::Command,
                "Subwave appsink mode is unavailable for mpv",
            )),
        }
    }

    /// Backend-neutral readiness signal for copied asynchronous events.
    pub fn event_signal(&self) -> Option<PlaybackEventSignal> {
        match &self.backend {
            BackendSession::Subwave(_) => None,
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => Some(adapter.event_signal()),
        }
    }

    /// Whether snapshot changes arrive through a push signal instead of the
    /// migration-only bounded legacy synchronization timer.
    pub fn uses_event_driven_snapshots(&self) -> bool {
        match &self.backend {
            BackendSession::Subwave(_) => false,
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(_) => true,
        }
    }

    /// Convert a failed Iced raw-host capture into the normal presenter
    /// failure/fallback transition for this session generation.
    pub fn native_host_capture_failed(
        &mut self,
        window_id: iced::window::Id,
        detail: String,
    ) {
        #[cfg(all(
            feature = "mpv",
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        if let BackendSession::Mpv(adapter) = &mut self.backend {
            adapter.fail_host_capture(window_id, detail);
        }

        #[cfg(not(all(
            feature = "mpv",
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        )))]
        let _ = (window_id, detail);
    }

    /// Re-query a platform-owned native video root from the UI thread. This is
    /// intentionally separate from decoded-frame/event polling because window
    /// movement and occlusion can change without producing an mpv event.
    pub fn refresh_native_presenter(&mut self) {
        #[cfg(all(
            feature = "mpv",
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        if let BackendSession::Mpv(adapter) = &mut self.backend
            && let Err(error) = adapter.refresh_presenter()
        {
            log::warn!("Could not refresh native presenter: {error}");
        }
    }

    /// Whether the UI must keep the native-presenter refresh clock alive.
    ///
    /// A verified native-window fallback can take one or more AppKit turns to
    /// become visible. The pending proof therefore keeps the clock alive after
    /// the integrated presentation itself has been retired.
    pub fn native_presenter_refresh_required(&self) -> bool {
        #[cfg(all(
            feature = "mpv",
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        if let BackendSession::Mpv(adapter) = &self.backend {
            #[cfg(target_os = "macos")]
            let fallback_pending = adapter.native_window_fallback_pending;
            #[cfg(target_os = "windows")]
            let fallback_pending = false;
            return native_presenter_refresh_required(
                adapter.presentation.is_some(),
                fallback_pending,
            );
        }

        false
    }

    /// Begin an AppKit-managed drag on the active integrated mpv root.
    ///
    /// The operation is synchronous because AppKit requires the current
    /// mouse-down event. Missing, fallback, or not-yet-attached presenters are
    /// benign no-ops.
    pub fn begin_native_root_drag(&mut self) -> bool {
        #[cfg(all(feature = "mpv", feature = "ui", target_os = "macos"))]
        if let BackendSession::Mpv(adapter) = &mut self.backend
            && adapter.snapshot().target
                == crate::contract::PlaybackTarget::MPV_INTEGRATED
            && !adapter.snapshot().fullscreen
            && let Some(presentation) = adapter.presentation.as_ref()
        {
            return match presentation.begin_native_root_drag() {
                Ok(started) => started,
                Err(error) => {
                    log::warn!(
                        "Could not begin native mpv root window drag: {error}"
                    );
                    false
                }
            };
        }

        false
    }

    /// Complete the shell-controlled visibility handoff for an integrated
    /// native presenter. The presenter attaches while hidden; the shell calls
    /// this only after its retained main window has been hidden.
    pub fn set_native_presenter_host_visible(&mut self, visible: bool) -> bool {
        #[cfg(all(
            feature = "mpv",
            feature = "ui",
            any(target_os = "windows", target_os = "macos")
        ))]
        if let BackendSession::Mpv(adapter) = &mut self.backend {
            return match adapter.set_presenter_host_visible(visible) {
                Ok(available) => available,
                Err(error) => {
                    log::warn!(
                        "Could not update native presenter host visibility: {error}"
                    );
                    false
                }
            };
        }

        let _ = visible;
        false
    }

    #[cfg(feature = "ui")]
    pub fn widget<'a, Theme>(
        &'a self,
        content_fit: iced::ContentFit,
        _native_host_window: Option<iced::window::Id>,
    ) -> iced::Element<'a, crate::PlayerMessage, Theme, iced_wgpu::Renderer>
    where
        Theme: 'a,
    {
        match &self.backend {
            BackendSession::Subwave(adapter) => adapter.widget(content_fit),
            #[cfg(feature = "mpv")]
            BackendSession::Mpv(adapter) => {
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                if let (Some(presentation), Some(window_id)) =
                    (adapter.presentation.as_ref(), _native_host_window)
                {
                    use crate::{
                        contract::PresenterState,
                        native_video_slot::{
                            NativeVideoSlot, NativeVideoSlotAppearance,
                        },
                    };

                    let appearance = match adapter.snapshot().presenter {
                        PresenterState::Attached
                        | PresenterState::Hidden
                        | PresenterState::Suspended => {
                            NativeVideoSlotAppearance::Transparent
                        }
                        PresenterState::Failed => {
                            NativeVideoSlotAppearance::Failed
                        }
                        PresenterState::Detached
                        | PresenterState::AwaitingHost
                        | PresenterState::AwaitingVideoOutput => {
                            NativeVideoSlotAppearance::Loading
                        }
                    };
                    return NativeVideoSlot::new(
                        presentation.slot_handle(window_id),
                        crate::PlayerMessage::CaptureNativeVideoHost,
                    )
                    .appearance(appearance)
                    .on_presenter_update(|| {
                        crate::PlayerMessage::NativePresenterUpdated
                    })
                    .into();
                }

                let _ = adapter;
                iced::widget::Space::new()
                    .width(iced::Length::Fill)
                    .height(iced::Length::Fill)
                    .into()
            }
        }
    }
}

impl Drop for PlaybackSession {
    fn drop(&mut self) {
        let _ = self.apply_command(PlaybackCommand::Shutdown);
    }
}

#[cfg(all(
    test,
    feature = "mpv",
    not(any(target_os = "windows", target_os = "macos"))
))]
mod tests {
    use super::*;
    use crate::contract::SessionGeneration;

    #[test]
    fn integrated_preflight_selects_native_controls_before_worker_spawn() {
        let plan = PlaybackSession::preflight_mpv_presentation(
            PlaybackTarget::MPV_INTEGRATED,
            SessionGeneration::new(5),
        );

        assert_eq!(plan.target(), PlaybackTarget::MPV_NATIVE_WINDOW);
        let fallback = plan.fallback.expect("unsupported presenter fallback");
        assert_eq!(fallback.from, Some(PlaybackTarget::MPV_INTEGRATED));
        assert_eq!(fallback.to, PlaybackTarget::MPV_NATIVE_WINDOW);
        assert_eq!(
            fallback.code,
            crate::contract::FallbackReasonCode::UnsupportedPlatform
        );
    }

    #[test]
    fn transient_native_fallback_observation_keeps_refresh_alive() {
        let mut pending = true;
        assert!(native_presenter_refresh_required(false, pending));
        assert!(!complete_verified_native_window_fallback(
            &mut pending,
            false
        ));
        assert!(pending);
        assert!(native_presenter_refresh_required(false, pending));

        assert!(complete_verified_native_window_fallback(&mut pending, true));
        assert!(!pending);
        assert!(!native_presenter_refresh_required(false, pending));
    }
}
