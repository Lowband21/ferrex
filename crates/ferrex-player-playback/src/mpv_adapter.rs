//! In-process libmpv adapter using mpv's ordinary native video-output window.
//!
//! This is the P3 compatibility path: libmpv owns decoding, rendering, and the
//! top-level video window. No render context or decoded frame crosses into
//! Iced/wgpu. The adapter translates only Ferrex-owned commands and copied mpv
//! events.

use std::{collections::HashMap, ffi::OsStr, sync::mpsc, time::Duration};

#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};

use ferrex_player_mpv::{
    MpvAsyncReply, MpvCompatibilityReport, MpvConfigPolicy, MpvEndFileReason,
    MpvEvent, MpvFormat, MpvFunctionTable, MpvLogLevel, MpvMessageLevel,
    MpvNode, MpvPropertyChange, MpvRequestId, MpvSessionConfig, MpvWorker,
    MpvWorkerConfig,
};
use zeroize::Zeroizing;

use crate::{
    contract::{
        AudioTrack, BackendKind, BackendRequest, BufferState, Chapter,
        ChapterId, Edition, EditionId, EndReason, EventSequence,
        PlaybackCapabilities, PlaybackCommand, PlaybackError,
        PlaybackErrorKind, PlaybackEvent, PlaybackEventEnvelope,
        PlaybackEventSignal, PlaybackFilePath, PlaybackScreenshotMode,
        PlaybackSnapshot, PlaybackSource, PlaybackState, PlaybackTarget,
        SessionGeneration, SubtitleKind, SubtitleTrack, TrackCatalog, TrackId,
        VideoParameters, VideoProfileName, reduce_event,
    },
    diagnostics::{
        MpvClientApiDiagnostics, MpvConfigurationDiagnostics,
        MpvConfigurationPolicy, MpvLogVerbosity, PlaybackDiagnosticSnapshot,
        redact_playback_url,
    },
};

const MPV_CONFIG_POLICY_ENV: &str = "FERREX_MPV_CONFIG_POLICY";
const MPV_LOG_LEVEL_ENV: &str = "FERREX_MPV_LOG_LEVEL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MpvLoggingPolicy {
    initial: MpvLogLevel,
    steady: MpvLogLevel,
    startup_verbose_capture: bool,
}

impl Default for MpvLoggingPolicy {
    fn default() -> Self {
        Self {
            initial: MpvLogLevel::Verbose,
            steady: MpvLogLevel::Info,
            startup_verbose_capture: true,
        }
    }
}

impl MpvLoggingPolicy {
    const fn fixed(level: MpvLogLevel) -> Self {
        Self {
            initial: level,
            steady: level,
            startup_verbose_capture: false,
        }
    }
}

const OBSERVED_PROPERTIES: &[(&str, MpvFormat)] = &[
    ("pause", MpvFormat::Flag),
    ("time-pos", MpvFormat::Double),
    ("duration", MpvFormat::Double),
    ("paused-for-cache", MpvFormat::Flag),
    ("cache-buffering-state", MpvFormat::Double),
    ("demuxer-cache-duration", MpvFormat::Double),
    ("demuxer-cache-state", MpvFormat::Node),
    ("core-idle", MpvFormat::Flag),
    ("seeking", MpvFormat::Flag),
    ("eof-reached", MpvFormat::Flag),
    ("idle-active", MpvFormat::Flag),
    ("track-list", MpvFormat::Node),
    ("chapter-list", MpvFormat::Node),
    ("chapter", MpvFormat::Int64),
    ("edition-list", MpvFormat::Node),
    ("edition", MpvFormat::Int64),
    ("video-params", MpvFormat::Node),
    ("video-out-params", MpvFormat::Node),
    ("vo-configured", MpvFormat::Flag),
    ("current-vo", MpvFormat::String),
    ("current-gpu-context", MpvFormat::String),
    ("hwdec-current", MpvFormat::String),
    ("hwdec-interop", MpvFormat::String),
    ("frame-drop-count", MpvFormat::Int64),
    ("decoder-frame-drop-count", MpvFormat::Int64),
    ("mistimed-frame-count", MpvFormat::Int64),
    ("vo-delayed-frame-count", MpvFormat::Int64),
    ("avsync", MpvFormat::Double),
    ("mpv-version", MpvFormat::String),
    ("ffmpeg-version", MpvFormat::String),
    ("volume", MpvFormat::Double),
    ("mute", MpvFormat::Flag),
    ("speed", MpvFormat::Double),
    ("fullscreen", MpvFormat::Flag),
    ("glsl-shaders", MpvFormat::Node),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    Load,
    Stop,
    AbsoluteSeek,
    Control(&'static str),
    NativeWindowIdRefresh(NativeWindowIdRefreshTicket),
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    NativeControl(NativeControl),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeWindowIdRefreshTicket {
    output_epoch: u64,
    observation_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeWindowIdRefreshResult {
    value_available: bool,
    stale: bool,
    applied: Option<i64>,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeControl {
    Osc,
    DefaultBindings,
    VoKeyboard,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl NativeControl {
    const fn operation(self) -> &'static str {
        match self {
            Self::Osc => "enable native-window OSC",
            Self::DefaultBindings => {
                "enable native-window default input bindings"
            }
            Self::VoKeyboard => "enable native-window keyboard input",
        }
    }
}

/// Keep pointer-driven absolute seeking bounded independently of Iced's event
/// cadence. One request may be in flight; newer positions replace the single
/// queued value until its reply arrives.
#[derive(Debug, Default)]
struct AbsoluteSeekCoalescer {
    active: Option<MpvRequestId>,
    queued: Option<Duration>,
}

impl AbsoluteSeekCoalescer {
    fn enqueue(&mut self, position: Duration) -> Option<Duration> {
        if self.active.is_some() {
            self.queued = Some(position);
            None
        } else {
            Some(position)
        }
    }

    fn submitted(&mut self, id: MpvRequestId) {
        debug_assert!(self.active.is_none());
        self.active = Some(id);
    }

    fn completed(&mut self, id: MpvRequestId) -> Option<Duration> {
        if self.active != Some(id) {
            return None;
        }
        self.active = None;
        self.queued.take()
    }

    fn clear(&mut self) {
        self.active = None;
        self.queued = None;
    }
}

/// Native-window libmpv provider adapted to the Ferrex playback contract.
pub(crate) struct MpvPlaybackAdapter {
    worker: Option<MpvWorker>,
    snapshot: PlaybackSnapshot,
    next_sequence: EventSequence,
    pending: HashMap<MpvRequestId, PendingAction>,
    absolute_seeks: AbsoluteSeekCoalescer,
    mapper: MpvEventMapper,
    redactor: MpvSourceRedactor,
    compatibility: MpvCompatibilityReport,
    config_policy: MpvConfigPolicy,
    logging_policy: MpvLoggingPolicy,
    osc_enabled: bool,
    input_default_bindings_enabled: bool,
    input_vo_keyboard_enabled: bool,
    event_signal: PlaybackEventSignal,
    startup_diagnostics_active: bool,
    native_output_epoch: u64,
    native_window_observation_revision: u64,
    native_window_id_refresh: Option<MpvRequestId>,
}

impl std::fmt::Debug for MpvPlaybackAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MpvPlaybackAdapter")
            .field("snapshot", &self.snapshot)
            .field("compatibility", &self.compatibility)
            .field("config_policy", &self.config_policy)
            .field("logging_policy", &self.logging_policy)
            .field("osc_enabled", &self.osc_enabled)
            .field(
                "input_bindings_enabled",
                &(self.input_default_bindings_enabled
                    && self.input_vo_keyboard_enabled),
            )
            .field("mpv_version", &self.mapper.mpv_version)
            .field("ffmpeg_version", &self.mapper.ffmpeg_version)
            .field("current_vo", &self.mapper.current_vo)
            .field("current_gpu_context", &self.mapper.current_gpu_context)
            .field("vo_configured", &self.mapper.vo_configured)
            .field(
                "native_window_id_observed",
                &self.mapper.native_window_id.is_some(),
            )
            .field("core_idle", &self.mapper.core_idle)
            .field(
                "startup_diagnostics_active",
                &self.startup_diagnostics_active,
            )
            .field(
                "native_window_id_refresh_pending",
                &self.native_window_id_refresh.is_some(),
            )
            .field("pending_requests", &self.pending.len())
            .field("absolute_seeks", &self.absolute_seeks)
            .finish_non_exhaustive()
    }
}

impl MpvPlaybackAdapter {
    /// Start one serialized libmpv owner and submit a per-file native-window
    /// load. Source credentials stay in process memory and are redacted before
    /// any copied mpv log reaches the application logger.
    #[cfg(test)]
    pub(crate) fn open(
        source: &PlaybackSource,
        start: Duration,
        generation: SessionGeneration,
    ) -> Result<Self, PlaybackError> {
        Self::open_for_target(
            source,
            start,
            generation,
            PlaybackTarget::MPV_NATIVE_WINDOW,
        )
    }

    /// Open mpv for an explicit presentation target. Integrated presenters
    /// keep OSC and VO input disabled so Iced remains the sole controls/input
    /// owner; ordinary native-window compatibility retains mpv's controls.
    pub(crate) fn open_for_target(
        source: &PlaybackSource,
        start: Duration,
        generation: SessionGeneration,
        target: PlaybackTarget,
    ) -> Result<Self, PlaybackError> {
        let functions = MpvFunctionTable::linked();
        let compatibility = functions.compatibility_report();
        if !compatibility.compatible {
            return Err(mpv_error(
                PlaybackErrorKind::BackendUnavailable,
                format!(
                    "incompatible libmpv client API {}; Ferrex requires {}",
                    compatibility.runtime, compatibility.minimum
                ),
                true,
            ));
        }

        // OSC and native input are enabled only for this explicit
        // native-window compatibility mode. User config and arbitrary
        // scripts remain disabled unless trusted-code mode was explicitly
        // selected through the developer-only policy switch.
        let config_policy = configured_mpv_config_policy();
        let logging_policy = configured_mpv_logging_policy();
        let native_controls = mpv_native_controls_enabled(target);
        let mut config =
            MpvSessionConfig::native_window_with_config_policy(config_policy)
                // Capture startup-only version, feature, GPU API, and adapter
                // lines, then return to concise informational logging after
                // the first file finishes initializing.
                .with_log_level(logging_policy.initial)
                .with_option("idle", "yes")
                .with_option("keep-open", "no")
                .with_option("save-position-on-quit", "no");
        if target == PlaybackTarget::MPV_NATIVE_WINDOW {
            config = config
                .with_option("osc", "yes")
                .with_option("input-default-bindings", "yes")
                .with_option("input-vo-keyboard", "yes");
        }
        let (event_notifier, event_notifications) = mpsc::sync_channel(1);
        let worker = MpvWorker::spawn_with_event_notifier(
            functions,
            config,
            MpvWorkerConfig::default(),
            event_notifier,
        )
        .map_err(|error| {
            worker_error(
                PlaybackErrorKind::BackendInitialization,
                "could not initialize in-process mpv",
                error,
            )
        })?;

        let mut adapter = Self {
            worker: Some(worker),
            snapshot: PlaybackSnapshot::new(
                generation,
                target,
                mpv_capabilities(config_policy),
            ),
            next_sequence: EventSequence::FIRST,
            pending: HashMap::new(),
            absolute_seeks: AbsoluteSeekCoalescer::default(),
            mapper: MpvEventMapper::default(),
            redactor: MpvSourceRedactor::new(source),
            compatibility,
            config_policy,
            logging_policy,
            osc_enabled: native_controls,
            input_default_bindings_enabled: native_controls,
            input_vo_keyboard_enabled: native_controls,
            event_signal: PlaybackEventSignal::new(
                generation,
                event_notifications,
            ),
            startup_diagnostics_active: logging_policy.startup_verbose_capture,
            native_output_epoch: 0,
            native_window_observation_revision: 0,
            native_window_id_refresh: None,
        };
        adapter.register_observations()?;
        adapter.submit_load(source, start)?;
        adapter.poll_events();
        Ok(adapter)
    }

    pub(crate) fn snapshot(&self) -> &PlaybackSnapshot {
        &self.snapshot
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    pub(crate) fn configure_integrated_presentation(
        &mut self,
        capabilities: &crate::presenter::PresenterCapabilities,
    ) {
        self.snapshot.target = PlaybackTarget::MPV_INTEGRATED;
        self.snapshot.capabilities.integrated_presentation =
            capabilities.integrated_overlay;
        self.snapshot.capabilities.native_window_fallback =
            capabilities.native_window_fallback;
        self.snapshot.capabilities.native_hdr = capabilities.native_hdr;
        self.snapshot.capabilities.fractional_scaling =
            capabilities.fractional_scaling;
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    pub(crate) fn vo_configured(&self) -> bool {
        self.mapper.vo_configured
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    pub(crate) fn native_window_id(&self) -> Option<i64> {
        self.mapper.native_window_id
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    pub(crate) fn native_video_output_started(&self) -> bool {
        self.mapper.vo_configured
            || self.mapper.native_window_id.is_some()
            || self.mapper.current_vo.is_some()
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    pub(crate) fn record_event(&mut self, event: PlaybackEvent) {
        self.record(event);
    }

    pub(crate) const fn compatibility_report(&self) -> MpvCompatibilityReport {
        self.compatibility
    }

    pub(crate) fn diagnostics(
        &self,
        requested_backend: BackendRequest,
    ) -> PlaybackDiagnosticSnapshot {
        let mut diagnostics = PlaybackDiagnosticSnapshot::from_snapshot(
            &self.snapshot,
            requested_backend,
        );
        diagnostics.versions.client_api = Some(MpvClientApiDiagnostics {
            bindings: self.compatibility.bindings.to_string(),
            runtime: self.compatibility.runtime.to_string(),
            minimum: self.compatibility.minimum.to_string(),
            compatible: self.compatibility.compatible,
        });
        diagnostics.mpv_configuration = Some(mpv_configuration_diagnostics(
            self.config_policy,
            self.logging_policy,
            self.osc_enabled,
            self.input_default_bindings_enabled
                && self.input_vo_keyboard_enabled,
        ));
        self.mapper.populate_diagnostics(&mut diagnostics);
        diagnostics
    }

    pub(crate) fn record_fallback(
        &mut self,
        reason: crate::contract::FallbackReason,
    ) {
        self.record(PlaybackEvent::Fallback(reason));
    }

    /// Transfer input ownership back to mpv before exposing a runtime
    /// presenter failure as ordinary native-window playback.
    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    pub(crate) fn enable_native_window_controls(
        &mut self,
    ) -> Result<(), PlaybackError> {
        if !self.osc_enabled {
            self.submit_native_control("osc", NativeControl::Osc)?;
        }
        if !self.input_default_bindings_enabled {
            self.submit_native_control(
                "input-default-bindings",
                NativeControl::DefaultBindings,
            )?;
        }
        if !self.input_vo_keyboard_enabled {
            self.submit_native_control(
                "input-vo-keyboard",
                NativeControl::VoKeyboard,
            )?;
        }
        Ok(())
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    pub(crate) fn commit_native_window_fallback(
        &mut self,
        reason: crate::contract::FallbackReason,
    ) {
        if let Err(error) = self.enable_native_window_controls() {
            // Native-window playback remains controllable through Ferrex even
            // if mpv cannot restore one of its optional in-window controls.
            // Keep the fallback alive and expose the control state through
            // diagnostics instead of turning a presenter failure into a
            // terminal playback failure.
            log::warn!(
                "could not restore optional mpv native-window controls: {error}"
            );
        }
        self.record(PlaybackEvent::CapabilitiesChanged(mpv_capabilities(
            self.config_policy,
        )));
        self.record(PlaybackEvent::Fallback(reason));
    }

    pub(crate) fn event_signal(&self) -> PlaybackEventSignal {
        self.event_signal.clone()
    }

    pub(crate) fn apply_command(
        &mut self,
        command: PlaybackCommand,
    ) -> Result<(), PlaybackError> {
        self.poll_events();

        match command {
            PlaybackCommand::Load(source) => {
                self.absolute_seeks.clear();
                self.begin_startup_diagnostics()?;
                self.redactor.replace_source(&source);
                self.mapper.reset_for_load();
                let result = self.submit_load(&source, Duration::ZERO);
                if result.is_err() {
                    self.finish_startup_diagnostics();
                }
                result
            }
            PlaybackCommand::SetPaused(paused) => {
                self.submit_property(
                    "pause",
                    MpvNode::Bool(paused),
                    "set pause",
                )?;
                Ok(())
            }
            PlaybackCommand::SeekAbsolute(position) => {
                // The UI already limits preview cadence, but replies can be
                // slower than that interval. Keep only the newest target while
                // one asynchronous seek is outstanding.
                if let Some(position) = self.absolute_seeks.enqueue(position) {
                    self.submit_absolute_seek(position)?;
                }
                self.mapper.seeking = true;
                self.record(PlaybackEvent::StateChanged(
                    PlaybackState::Seeking,
                ));
                Ok(())
            }
            PlaybackCommand::SeekRelative(delta) => {
                let seconds = delta.as_seconds_f64();
                if !seconds.is_finite() {
                    return Err(mpv_error(
                        PlaybackErrorKind::Command,
                        "relative seek must be finite",
                        false,
                    ));
                }
                let request = self.worker()?.command_async([
                    "seek".to_string(),
                    seconds.to_string(),
                    "relative+exact".to_string(),
                ]);
                self.track_request(
                    request,
                    PendingAction::Control("relative seek"),
                )?;
                self.mapper.seeking = true;
                self.record(PlaybackEvent::StateChanged(
                    PlaybackState::Seeking,
                ));
                Ok(())
            }
            PlaybackCommand::SetVolume(volume) => {
                if !volume.is_finite() {
                    return Err(mpv_error(
                        PlaybackErrorKind::Command,
                        "volume must be finite",
                        false,
                    ));
                }
                self.submit_property(
                    "volume",
                    MpvNode::Double(volume.clamp(0.0, 1.0) * 100.0),
                    "set volume",
                )?;
                Ok(())
            }
            PlaybackCommand::SetMuted(muted) => {
                self.submit_property("mute", MpvNode::Bool(muted), "set mute")?;
                Ok(())
            }
            PlaybackCommand::SetSpeed(speed) => {
                if !speed.is_finite() || speed <= 0.0 {
                    return Err(mpv_error(
                        PlaybackErrorKind::Command,
                        "playback speed must be finite and positive",
                        false,
                    ));
                }
                self.submit_property(
                    "speed",
                    MpvNode::Double(speed),
                    "set speed",
                )?;
                Ok(())
            }
            PlaybackCommand::SelectAudio(track_id) => {
                let native_id = self
                    .mapper
                    .audio_ids
                    .get(&track_id)
                    .copied()
                    .ok_or_else(|| {
                    mpv_error(
                        PlaybackErrorKind::Command,
                        format!("unknown mpv audio track identity: {track_id}"),
                        false,
                    )
                })?;
                self.submit_property(
                    "aid",
                    MpvNode::Int(native_id),
                    "select audio track",
                )?;
                Ok(())
            }
            PlaybackCommand::SelectSubtitle(track_id) => {
                let value = match track_id {
                    Some(track_id) => {
                        let native_id = self
                            .mapper
                            .subtitle_ids
                            .get(&track_id)
                            .copied()
                            .ok_or_else(|| {
                                mpv_error(
                                    PlaybackErrorKind::Command,
                                    format!(
                                        "unknown mpv subtitle track identity: {track_id}"
                                    ),
                                    false,
                                )
                            })?;
                        MpvNode::Int(native_id)
                    }
                    None => MpvNode::String("no".to_string()),
                };
                self.submit_property("sid", value, "select subtitle track")?;
                Ok(())
            }
            PlaybackCommand::AddExternalSubtitle { source, select } => {
                if !self.snapshot.capabilities.external_subtitle_loading {
                    return Err(unsupported_mpv_extension(
                        "external subtitle loading is unavailable",
                    ));
                }
                let command = build_external_subtitle_command(&source, select)?;
                self.redactor.remember_local_values([command[1].as_str()])?;
                let request = self.worker()?.command_async(command);
                self.track_request(
                    request,
                    PendingAction::Control("add external subtitle"),
                )?;
                Ok(())
            }
            PlaybackCommand::SelectChapter(chapter_id) => {
                let native_id = self
                    .mapper
                    .chapter_ids
                    .get(&chapter_id)
                    .copied()
                    .ok_or_else(|| {
                        mpv_error(
                            PlaybackErrorKind::Command,
                            format!(
                                "unknown mpv chapter identity: {}",
                                chapter_id.as_str()
                            ),
                            false,
                        )
                    })?;
                self.submit_property(
                    "chapter",
                    MpvNode::Int(native_id),
                    "select chapter",
                )?;
                Ok(())
            }
            PlaybackCommand::SelectEdition(edition_id) => {
                // mpv reports the `edition` property as unavailable for files
                // with a single default edition. Treat selecting that already
                // active catalog entry as an idempotent command.
                if self.snapshot.current_edition.as_ref() == Some(&edition_id) {
                    return Ok(());
                }
                let native_id = self
                    .mapper
                    .edition_ids
                    .get(&edition_id)
                    .copied()
                    .ok_or_else(|| {
                        mpv_error(
                            PlaybackErrorKind::Command,
                            format!(
                                "unknown mpv edition identity: {}",
                                edition_id.as_str()
                            ),
                            false,
                        )
                    })?;
                self.submit_property(
                    "edition",
                    MpvNode::Int(native_id),
                    "select edition",
                )?;
                Ok(())
            }
            PlaybackCommand::SetContentFit(content_fit) => {
                for (name, value) in content_fit_properties(content_fit) {
                    self.submit_property(name, value, "set content fit")?;
                }
                self.record(PlaybackEvent::ContentFitChanged(content_fit));
                Ok(())
            }
            PlaybackCommand::SetFullscreen(fullscreen) => {
                self.submit_property(
                    "fullscreen",
                    MpvNode::Bool(fullscreen),
                    "set fullscreen",
                )?;
                Ok(())
            }
            PlaybackCommand::ApplyVideoProfile(profile) => {
                if !self.snapshot.capabilities.video_profile_passthrough {
                    return Err(unsupported_mpv_extension(
                        "user video profiles require the trusted-user mpv configuration policy",
                    ));
                }
                let command = build_apply_profile_command(&profile)?;
                self.redactor.remember_local_values([command[1].as_str()])?;
                let request = self.worker()?.command_async(command);
                self.track_request(
                    request,
                    PendingAction::Control("apply video profile"),
                )?;
                Ok(())
            }
            PlaybackCommand::SetVideoShaders(shaders) => {
                if !self.snapshot.capabilities.video_shader_passthrough {
                    return Err(unsupported_mpv_extension(
                        "video shader passthrough is unavailable",
                    ));
                }
                let commands = build_shader_commands(&shaders)?;
                self.redactor.remember_local_values(
                    commands.iter().map(|command| command[3].as_str()),
                )?;
                for command in commands {
                    let request = self.worker()?.command_async(command);
                    self.track_request(
                        request,
                        PendingAction::Control("set video shaders"),
                    )?;
                }
                Ok(())
            }
            PlaybackCommand::CaptureScreenshot { output, mode } => {
                if !self.snapshot.capabilities.screenshot {
                    return Err(unsupported_mpv_extension(
                        "native video screenshots are unavailable",
                    ));
                }
                let command = build_screenshot_command(&output, mode)?;
                self.redactor.remember_local_values([command[1].as_str()])?;
                let request = self.worker()?.command_async(command);
                self.track_request(
                    request,
                    PendingAction::Control("capture screenshot"),
                )?;
                Ok(())
            }
            PlaybackCommand::Stop => {
                self.absolute_seeks.clear();
                if self.worker.is_none() {
                    return Ok(());
                }
                self.mapper.stopping = true;
                self.record(PlaybackEvent::StateChanged(
                    PlaybackState::Stopping,
                ));
                let request = self.worker()?.command_async(["stop"]);
                self.track_request(request, PendingAction::Stop)?;
                Ok(())
            }
            PlaybackCommand::Shutdown => self.shutdown(),
        }
    }

    pub(crate) fn poll_events(&mut self) {
        let events = self
            .worker
            .as_ref()
            .map(MpvWorker::drain_events)
            .unwrap_or_default();
        for event in events {
            self.handle_event(event);
        }
    }

    pub(crate) fn refresh_tracks(&mut self) -> TrackCatalog {
        self.poll_events();
        self.snapshot.tracks.clone()
    }

    pub(crate) fn has_video(&self) -> bool {
        !matches!(
            self.snapshot.state,
            PlaybackState::Idle
                | PlaybackState::Failed
                | PlaybackState::Terminated
        )
    }

    pub(crate) fn subtitles_enabled(&self) -> bool {
        self.snapshot.tracks.selected_subtitle.is_some()
    }

    pub(crate) fn set_subtitles_enabled(&mut self, enabled: bool) {
        if !enabled {
            let _ = self.apply_command(PlaybackCommand::SelectSubtitle(None));
        }
    }

    fn register_observations(&self) -> Result<(), PlaybackError> {
        for (name, format) in OBSERVED_PROPERTIES {
            self.worker()?.observe_property(*name, *format).map_err(
                |error| {
                    worker_error(
                        PlaybackErrorKind::BackendInitialization,
                        "could not observe required mpv property",
                        error,
                    )
                },
            )?;
        }
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        self.worker()?
            .observe_property("window-id", MpvFormat::Int64)
            .map_err(|error| {
                worker_error(
                    PlaybackErrorKind::BackendInitialization,
                    "could not observe mpv's native window-id",
                    error,
                )
            })?;
        Ok(())
    }

    fn submit_load(
        &mut self,
        source: &PlaybackSource,
        start: Duration,
    ) -> Result<(), PlaybackError> {
        let command = build_load_command(source, start)?;
        self.invalidate_native_output_refresh();
        self.mapper.reset_for_load();
        // A replacement file must not expose selectable identities from the
        // previous demuxer while mpv is rebuilding its property catalogs.
        self.record(PlaybackEvent::TracksChanged(TrackCatalog::default()));
        self.record(PlaybackEvent::ChaptersChanged(Vec::new()));
        self.record(PlaybackEvent::ChapterChanged(None));
        self.record(PlaybackEvent::EditionsChanged(Vec::new()));
        self.record(PlaybackEvent::EditionChanged(None));
        self.record(PlaybackEvent::VideoParametersChanged(None));
        self.record(PlaybackEvent::DurationChanged(None));
        self.record(PlaybackEvent::StateChanged(PlaybackState::Loading));
        let request = self.worker()?.command_node_async(command);
        self.track_request(request, PendingAction::Load)?;
        Ok(())
    }

    fn submit_absolute_seek(
        &mut self,
        position: Duration,
    ) -> Result<(), PlaybackError> {
        let seconds = finite_seconds(position)?;
        let request = self.worker()?.command_async([
            "seek".to_string(),
            seconds,
            "absolute+exact".to_string(),
        ]);
        let id = self.track_request(request, PendingAction::AbsoluteSeek)?;
        self.absolute_seeks.submitted(id);
        Ok(())
    }

    fn submit_property(
        &mut self,
        name: &str,
        value: MpvNode,
        operation: &'static str,
    ) -> Result<MpvRequestId, PlaybackError> {
        let request = self.worker()?.set_property_async(name, value);
        self.track_request(request, PendingAction::Control(operation))
    }

    #[cfg(all(
        feature = "ui",
        any(target_os = "windows", target_os = "macos")
    ))]
    fn submit_native_control(
        &mut self,
        name: &str,
        control: NativeControl,
    ) -> Result<MpvRequestId, PlaybackError> {
        let request =
            self.worker()?.set_property_async(name, MpvNode::Bool(true));
        self.track_request(request, PendingAction::NativeControl(control))
    }

    fn invalidate_native_output_refresh(&mut self) {
        self.native_output_epoch = self.native_output_epoch.wrapping_add(1);
        self.native_window_id_refresh = None;
    }

    fn submit_native_window_id_refresh(&mut self) -> Result<(), PlaybackError> {
        if self.native_window_id_refresh.is_some()
            || self.mapper.native_window_id.is_some()
            || !self.mapper.vo_configured
        {
            return Ok(());
        }
        let ticket = NativeWindowIdRefreshTicket {
            output_epoch: self.native_output_epoch,
            observation_revision: self.native_window_observation_revision,
        };
        let request = self
            .worker()?
            .get_property_async("window-id", MpvFormat::Int64);
        let id = self.track_request(
            request,
            PendingAction::NativeWindowIdRefresh(ticket),
        )?;
        self.native_window_id_refresh = Some(id);
        log::debug!("mpv native window-id refresh submitted");
        Ok(())
    }

    fn handle_native_window_id_refresh_reply(
        &mut self,
        reply: &MpvAsyncReply,
        ticket: NativeWindowIdRefreshTicket,
    ) {
        let request_is_current =
            self.native_window_id_refresh == Some(reply.id);
        if request_is_current {
            self.native_window_id_refresh = None;
        }
        let value = reply.result.as_ref().ok().and_then(|value| value.as_ref());
        let result = evaluate_native_window_id_refresh(
            ticket,
            self.native_output_epoch,
            self.native_window_observation_revision,
            request_is_current,
            self.mapper.vo_configured,
            self.mapper.native_window_id,
            value,
            cfg!(target_os = "macos"),
        );
        if let Some(native_window_id) = result.applied {
            self.mapper.native_window_id = Some(native_window_id);
        }
        log::debug!(
            "mpv native window-id refresh completed: request_succeeded={} value_available={} stale={} applied={}",
            reply.result.is_ok(),
            result.value_available,
            result.stale,
            result.applied.is_some(),
        );
    }

    fn track_request(
        &mut self,
        result: Result<MpvRequestId, ferrex_player_mpv::MpvWorkerError>,
        action: PendingAction,
    ) -> Result<MpvRequestId, PlaybackError> {
        let id = result.map_err(|error| {
            worker_error(
                if matches!(action, PendingAction::Load) {
                    PlaybackErrorKind::BackendInitialization
                } else {
                    PlaybackErrorKind::Command
                },
                "libmpv rejected an asynchronous request",
                error,
            )
        })?;
        self.pending.insert(id, action);
        Ok(id)
    }

    fn handle_event(&mut self, event: MpvEvent) {
        match event {
            MpvEvent::Log(message) => {
                let text = self.redactor.redact(&message.text);
                self.mapper.observe_log(&message.prefix, &text);
                let line = format!("mpv[{}]: {text}", message.prefix);
                match message.level {
                    MpvMessageLevel::Fatal | MpvMessageLevel::Error => {
                        log::error!("{line}")
                    }
                    MpvMessageLevel::Warn => log::warn!("{line}"),
                    MpvMessageLevel::Info => log::info!("{line}"),
                    MpvMessageLevel::Verbose | MpvMessageLevel::Debug => {
                        log::debug!("{line}")
                    }
                    MpvMessageLevel::Trace => log::trace!("{line}"),
                    MpvMessageLevel::Unknown(_) => log::debug!("{line}"),
                }
            }
            MpvEvent::AsyncReply(reply) => {
                let action = self.pending.remove(&reply.id);
                if let Some(PendingAction::NativeWindowIdRefresh(ticket)) =
                    action
                {
                    self.handle_native_window_id_refresh_reply(&reply, ticket);
                    return;
                }
                let queued_absolute_seek =
                    matches!(action, Some(PendingAction::AbsoluteSeek))
                        .then(|| self.absolute_seeks.completed(reply.id))
                        .flatten();

                #[cfg(any(target_os = "windows", target_os = "macos"))]
                {
                    if reply.result.is_ok()
                        && let Some(PendingAction::NativeControl(control)) =
                            action
                    {
                        match control {
                            NativeControl::Osc => self.osc_enabled = true,
                            NativeControl::DefaultBindings => {
                                self.input_default_bindings_enabled = true;
                            }
                            NativeControl::VoKeyboard => {
                                self.input_vo_keyboard_enabled = true;
                            }
                        }
                    }
                }

                if let Err(error) = reply.result {
                    if matches!(action, Some(PendingAction::Load)) {
                        self.mapper.terminal = true;
                        self.record(PlaybackEvent::Error(native_error(
                            PlaybackErrorKind::UnsupportedMedia,
                            "mpv could not load the media source",
                            error.code,
                            true,
                        )));
                        self.finish_startup_diagnostics();
                    } else {
                        let operation = match action {
                            Some(PendingAction::Stop) => "stop",
                            Some(PendingAction::AbsoluteSeek) => {
                                "absolute seek"
                            }
                            Some(PendingAction::Control(operation)) => {
                                operation
                            }
                            Some(PendingAction::NativeWindowIdRefresh(_)) => {
                                "refresh native window identity"
                            }
                            #[cfg(any(
                                target_os = "windows",
                                target_os = "macos"
                            ))]
                            Some(PendingAction::NativeControl(control)) => {
                                control.operation()
                            }
                            Some(PendingAction::Load) => "load",
                            None => "unknown request",
                        };
                        #[cfg(any(target_os = "windows", target_os = "macos"))]
                        if matches!(
                            action,
                            Some(PendingAction::NativeControl(_))
                        ) {
                            log::warn!(
                                "mpv could not restore optional native-window control `{operation}`; Ferrex controls remain available"
                            );
                        }
                        log::warn!(
                            "mpv {operation} request failed with native error {} ({})",
                            error.code,
                            error.description
                        );
                    }
                }

                if let Some(position) = queued_absolute_seek
                    && let Err(error) = self.submit_absolute_seek(position)
                {
                    self.absolute_seeks.clear();
                    log::warn!(
                        "could not submit the coalesced mpv absolute seek: {error}"
                    );
                }
            }
            MpvEvent::PropertyChanged(change) => {
                self.handle_property_change(change);
            }
            MpvEvent::UnmatchedAsyncReply { id, kind, error } => {
                log::warn!(
                    "received unmatched mpv {kind:?} reply {}: {:?}",
                    id.get(),
                    error
                );
            }
            MpvEvent::Hook(hook) => {
                if let Some(worker) = self.worker.as_ref()
                    && let Err(error) = worker.continue_hook(hook.id)
                {
                    log::error!("could not continue mpv hook: {error}");
                }
            }
            MpvEvent::ClientMessage(arguments) => {
                log::debug!(
                    "received mpv client message with {} argument(s)",
                    arguments.len()
                );
            }
            MpvEvent::ProtocolError { event_id, message } => {
                self.mapper.terminal = true;
                self.record(PlaybackEvent::Error(mpv_error(
                    PlaybackErrorKind::Protocol,
                    format!("invalid mpv event {event_id}: {message}"),
                    true,
                )));
                self.finish_startup_diagnostics();
            }
            MpvEvent::QueueOverflow => {
                self.mapper.terminal = true;
                self.record(PlaybackEvent::Error(mpv_error(
                    PlaybackErrorKind::Protocol,
                    "mpv event queue overflowed",
                    true,
                )));
                self.finish_startup_diagnostics();
            }
            other => {
                if matches!(&other, MpvEvent::StartFile { .. }) {
                    self.invalidate_native_output_refresh();
                }
                let startup_complete = matches!(
                    &other,
                    MpvEvent::FileLoaded
                        | MpvEvent::EndFile(_)
                        | MpvEvent::Shutdown
                );
                for event in self.mapper.map_event(&other) {
                    self.record(event);
                }
                if startup_complete {
                    self.finish_startup_diagnostics();
                }
            }
        }
    }

    fn handle_property_change(&mut self, change: MpvPropertyChange) {
        let previous_vo_configured = self.mapper.vo_configured;
        let previous_native_window_id = self.mapper.native_window_id;
        if change.name == "window-id" {
            self.native_window_observation_revision =
                self.native_window_observation_revision.wrapping_add(1);
        }

        for event in self.mapper.map_property(&change) {
            self.record(event);
        }

        let vo_changed = previous_vo_configured != self.mapper.vo_configured;
        let window_id_changed =
            previous_native_window_id != self.mapper.native_window_id;
        if (change.name == "vo-configured" && vo_changed)
            || (change.name == "window-id" && window_id_changed)
        {
            log::debug!(
                "mpv native-output readiness transition: property={} vo_configured={} native_window_id_observed={} identity_changed={}",
                change.name,
                self.mapper.vo_configured,
                self.mapper.native_window_id.is_some(),
                previous_native_window_id.is_some()
                    && self.mapper.native_window_id.is_some()
                    && window_id_changed,
            );
        }

        if previous_vo_configured && !self.mapper.vo_configured {
            self.invalidate_native_output_refresh();
        }
        if cfg!(target_os = "macos")
            && native_window_id_refresh_needed(
                previous_vo_configured,
                self.mapper.vo_configured,
                self.mapper.native_window_id,
                self.native_window_id_refresh.is_some(),
            )
            && let Err(error) = self.submit_native_window_id_refresh()
        {
            log::warn!(
                "could not submit one-shot mpv native window-id refresh: {error}"
            );
        }
    }

    fn shutdown(&mut self) -> Result<(), PlaybackError> {
        self.absolute_seeks.clear();
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };

        // mpv's macOS VO synchronously dispatches parts of teardown to the
        // AppKit main queue. PlaybackSession::shutdown is called from Iced's
        // AppKit callback, so waiting for the owner here can deadlock both
        // threads. Move the worker to a named reaper and return immediately;
        // the event loop can then service the native teardown dispatch.
        #[cfg(target_os = "macos")]
        {
            let result = reap_macos_worker(worker);
            self.mapper.terminal = true;
            self.record(PlaybackEvent::StateChanged(PlaybackState::Terminated));
            return result;
        }

        #[cfg(not(target_os = "macos"))]
        {
            let mut worker = worker;
            let report = worker.shutdown().map_err(|error| {
                worker_error(
                    PlaybackErrorKind::Shutdown,
                    "ordered libmpv shutdown failed",
                    error,
                )
            })?;
            for event in worker.drain_events() {
                self.handle_event(event);
            }
            if report.timed_out {
                log::warn!("libmpv stop drain reached its shutdown deadline");
            }
            self.mapper.terminal = true;
            self.record(PlaybackEvent::StateChanged(PlaybackState::Terminated));
            Ok(())
        }
    }

    fn begin_startup_diagnostics(&mut self) -> Result<(), PlaybackError> {
        if !self.logging_policy.startup_verbose_capture {
            return Ok(());
        }
        if self.startup_diagnostics_active {
            return Ok(());
        }
        self.worker()?.set_log_level(MpvLogLevel::Verbose).map_err(
            |error| {
                worker_error(
                    PlaybackErrorKind::Command,
                    "could not enable mpv startup diagnostics",
                    error,
                )
            },
        )?;
        self.startup_diagnostics_active = true;
        Ok(())
    }

    fn finish_startup_diagnostics(&mut self) {
        if !self.startup_diagnostics_active {
            return;
        }
        self.startup_diagnostics_active = false;
        if let Some(worker) = self.worker.as_ref()
            && let Err(error) = worker.set_log_level(self.logging_policy.steady)
        {
            log::warn!(
                "could not restore concise mpv logging after startup diagnostics: {error}"
            );
        }
    }

    fn worker(&self) -> Result<&MpvWorker, PlaybackError> {
        self.worker.as_ref().ok_or_else(|| {
            mpv_error(
                PlaybackErrorKind::Shutdown,
                "libmpv owner has already terminated",
                false,
            )
        })
    }

    fn record(&mut self, event: PlaybackEvent) {
        let sequence = self.next_sequence;
        let Some(next) = sequence.next() else {
            self.snapshot.state = PlaybackState::Failed;
            self.snapshot.last_error = Some(mpv_error(
                PlaybackErrorKind::Protocol,
                "mpv event sequence exhausted",
                false,
            ));
            return;
        };
        self.next_sequence = next;
        let generation = self.snapshot.generation;
        let _ = reduce_event(
            &mut self.snapshot,
            PlaybackEventEnvelope {
                generation,
                sequence,
                event,
            },
        );
    }
}

/// Hand a macOS libmpv owner to a background reaper without ever dropping the
/// worker on AppKit's main thread. The extra `Arc<Mutex<Option<_>>>` matters on
/// the rare thread-spawn failure path: `Builder::spawn` consumes and drops its
/// closure on the caller, so capturing the worker directly would invoke its
/// blocking `Drop` exactly where it is unsafe.
#[cfg(target_os = "macos")]
fn reap_macos_worker(worker: MpvWorker) -> Result<(), PlaybackError> {
    let worker = Arc::new(Mutex::new(Some(worker)));
    let reaper_worker = Arc::clone(&worker);
    let spawn = std::thread::Builder::new()
        .name("ferrex-libmpv-appkit-reaper".to_string())
        .spawn(move || {
            let mut worker = reaper_worker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .expect("macOS mpv reaper owns one worker");
            match worker.shutdown() {
                Ok(report) if report.timed_out => {
                    log::warn!(
                        "libmpv macOS stop drain reached its shutdown deadline"
                    );
                }
                Ok(_) => {
                    log::debug!("libmpv macOS reaper completed native teardown");
                }
                Err(error) => {
                    log::error!(
                        "libmpv macOS reaper could not complete ordered shutdown: {error}"
                    );
                }
            }
        });

    if let Err(error) = spawn {
        let mut worker = worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("failed reaper spawn preserves the mpv worker");
        let begin_error = worker.begin_shutdown().err();

        // `MpvWorker::drop` is intentionally blocking. If no reaper thread can
        // be created, leaking this already-shutting-down handle is preferable
        // to freezing AppKit. The owner receives the shutdown request and may
        // still finish native teardown once this callback returns.
        std::mem::forget(worker);

        let detail = begin_error.map_or_else(
            || format!("could not start the macOS libmpv reaper: {error}"),
            |begin_error| {
                format!(
                    "could not start the macOS libmpv reaper ({error}) or begin ordered shutdown ({begin_error})"
                )
            },
        );
        return Err(mpv_error(PlaybackErrorKind::Shutdown, detail, true));
    }

    Ok(())
}

impl Drop for MpvPlaybackAdapter {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Debug, Default)]
struct MpvEventMapper {
    paused: bool,
    buffering: bool,
    paused_for_cache: bool,
    cache_underrun: bool,
    core_idle: bool,
    seeking: bool,
    stopping: bool,
    file_loaded: bool,
    terminal: bool,
    buffer: BufferState,
    audio_ids: HashMap<TrackId, i64>,
    subtitle_ids: HashMap<TrackId, i64>,
    chapter_ids: HashMap<ChapterId, i64>,
    current_chapter_id: Option<i64>,
    edition_ids: HashMap<EditionId, i64>,
    current_edition_id: Option<i64>,
    video: Option<VideoParameters>,
    input_video: Option<VideoParameters>,
    output_video: Option<VideoParameters>,
    vo_configured: bool,
    /// Opaque platform window identity. Never log or serialize this value.
    native_window_id: Option<i64>,
    current_vo: Option<String>,
    current_gpu_api: Option<String>,
    current_gpu_context: Option<String>,
    active_video_shader_count: Option<usize>,
    gpu_adapter: Option<String>,
    hardware_decoder: Option<String>,
    hardware_decoder_interop: Option<String>,
    decoder_frame_drop_count: Option<u64>,
    frame_drop_count: Option<u64>,
    mistimed_frame_count: Option<u64>,
    delayed_frame_count: Option<u64>,
    av_sync_seconds: Option<f64>,
    mpv_version: Option<String>,
    ffmpeg_version: Option<String>,
    libplacebo_version: Option<String>,
    compiled_features: Vec<String>,
}

impl MpvEventMapper {
    fn reset_for_load(&mut self) {
        self.buffering = false;
        self.paused_for_cache = false;
        self.cache_underrun = false;
        self.core_idle = true;
        self.seeking = false;
        self.stopping = false;
        self.file_loaded = false;
        self.terminal = false;
        self.buffer = BufferState::default();
        self.audio_ids.clear();
        self.subtitle_ids.clear();
        self.chapter_ids.clear();
        self.current_chapter_id = None;
        self.edition_ids.clear();
        self.current_edition_id = None;
        self.video = None;
        self.input_video = None;
        self.output_video = None;
        self.vo_configured = false;
        self.native_window_id = None;
        self.current_vo = None;
        self.current_gpu_api = None;
        self.current_gpu_context = None;
        self.gpu_adapter = None;
        self.hardware_decoder = None;
        self.hardware_decoder_interop = None;
        self.decoder_frame_drop_count = None;
        self.frame_drop_count = None;
        self.mistimed_frame_count = None;
        self.delayed_frame_count = None;
        self.av_sync_seconds = None;
    }

    fn populate_diagnostics(
        &self,
        diagnostics: &mut PlaybackDiagnosticSnapshot,
    ) {
        diagnostics.versions.mpv = self.mpv_version.clone();
        diagnostics.versions.ffmpeg = self.ffmpeg_version.clone();
        diagnostics.versions.libplacebo = self.libplacebo_version.clone();
        diagnostics.versions.compiled_features = self.compiled_features.clone();
        diagnostics.output.vo_configured = Some(self.vo_configured);
        diagnostics.output.video_output = self.current_vo.clone();
        diagnostics.output.gpu_api = self.current_gpu_api.clone();
        diagnostics.output.gpu_context = self.current_gpu_context.clone();
        diagnostics.output.gpu_adapter = self.gpu_adapter.clone();
        diagnostics.output.hardware_decoder = self.hardware_decoder.clone();
        diagnostics.output.hardware_decoder_interop =
            self.hardware_decoder_interop.clone();
        diagnostics.output.input_video = self.input_video.clone();
        diagnostics.output.output_video = self.output_video.clone();
        diagnostics.output.frames.decoder_dropped =
            self.decoder_frame_drop_count;
        diagnostics.output.frames.output_dropped = self.frame_drop_count;
        diagnostics.output.frames.mistimed = self.mistimed_frame_count;
        diagnostics.output.frames.delayed = self.delayed_frame_count;
        diagnostics.output.frames.av_sync_seconds = self.av_sync_seconds;
        if let Some(configuration) = diagnostics.mpv_configuration.as_mut() {
            configuration.active_video_shader_count =
                self.active_video_shader_count;
        }
    }

    fn observe_log(&mut self, prefix: &str, text: &str) {
        if self.libplacebo_version.is_none()
            && (prefix.contains("libplacebo")
                || text.to_ascii_lowercase().contains("libplacebo"))
        {
            self.libplacebo_version = extract_libplacebo_version(text);
        }
        if self.compiled_features.is_empty() {
            self.compiled_features = parse_compiled_features(text);
        }
        if self.current_gpu_api.is_none() {
            self.current_gpu_api = gpu_api_from_log_prefix(prefix);
        }
        if self.gpu_adapter.is_none() {
            self.gpu_adapter = gpu_adapter_from_log(prefix, text);
        }
    }

    fn map_event(&mut self, event: &MpvEvent) -> Vec<PlaybackEvent> {
        match event {
            MpvEvent::StartFile { .. } => {
                self.reset_for_load();
                vec![
                    PlaybackEvent::StateChanged(PlaybackState::Loading),
                    PlaybackEvent::TracksChanged(TrackCatalog::default()),
                    PlaybackEvent::ChaptersChanged(Vec::new()),
                    PlaybackEvent::ChapterChanged(None),
                    PlaybackEvent::EditionsChanged(Vec::new()),
                    PlaybackEvent::EditionChanged(None),
                    PlaybackEvent::VideoParametersChanged(None),
                    PlaybackEvent::DurationChanged(None),
                ]
            }
            MpvEvent::FileLoaded => {
                self.file_loaded = true;
                vec![PlaybackEvent::StateChanged(self.normal_state())]
            }
            MpvEvent::PropertyChanged(change) => self.map_property(change),
            MpvEvent::Seek => {
                self.seeking = true;
                vec![PlaybackEvent::StateChanged(PlaybackState::Seeking)]
            }
            MpvEvent::PlaybackRestart => {
                self.file_loaded = true;
                self.seeking = false;
                vec![PlaybackEvent::StateChanged(self.normal_state())]
            }
            MpvEvent::EndFile(end) => {
                self.file_loaded = false;
                self.seeking = false;
                self.buffering = false;
                self.stopping = false;
                self.terminal = true;
                match end.reason {
                    MpvEndFileReason::Eof => {
                        vec![PlaybackEvent::Ended(EndReason::Eof)]
                    }
                    MpvEndFileReason::Stop => {
                        vec![PlaybackEvent::Ended(EndReason::Stopped)]
                    }
                    MpvEndFileReason::Redirect => {
                        vec![PlaybackEvent::Ended(EndReason::Replaced)]
                    }
                    MpvEndFileReason::Quit => {
                        vec![PlaybackEvent::Ended(EndReason::Closed)]
                    }
                    MpvEndFileReason::Unknown(_) => {
                        vec![PlaybackEvent::Ended(EndReason::BackendTerminated)]
                    }
                    MpvEndFileReason::Error => {
                        let code = end.error.map(|error| i64::from(error.code));
                        let mut error = mpv_error(
                            PlaybackErrorKind::UnsupportedMedia,
                            "mpv playback ended with a native media error",
                            true,
                        );
                        error.code = code;
                        vec![PlaybackEvent::Error(error)]
                    }
                }
            }
            MpvEvent::Idle if !self.terminal => {
                self.file_loaded = false;
                vec![PlaybackEvent::StateChanged(PlaybackState::Idle)]
            }
            MpvEvent::Idle => Vec::new(),
            MpvEvent::Shutdown if self.terminal => Vec::new(),
            MpvEvent::Shutdown => {
                self.terminal = true;
                vec![PlaybackEvent::Ended(EndReason::BackendTerminated)]
            }
            MpvEvent::VideoReconfigured
            | MpvEvent::AudioReconfigured
            | MpvEvent::Tick
            | MpvEvent::Unknown { .. }
            | MpvEvent::Log(_)
            | MpvEvent::AsyncReply(_)
            | MpvEvent::UnmatchedAsyncReply { .. }
            | MpvEvent::ClientMessage(_)
            | MpvEvent::QueueOverflow
            | MpvEvent::Hook(_)
            | MpvEvent::ProtocolError { .. } => Vec::new(),
        }
    }

    fn map_property(
        &mut self,
        change: &MpvPropertyChange,
    ) -> Vec<PlaybackEvent> {
        match change.name.as_str() {
            "pause" => {
                if let Some(paused) = node_bool(change.value.as_ref()) {
                    self.paused = paused;
                    if self.file_loaded && !self.terminal {
                        return vec![PlaybackEvent::StateChanged(
                            self.normal_state(),
                        )];
                    }
                }
                Vec::new()
            }
            "time-pos" => node_nonnegative_f64(change.value.as_ref())
                .and_then(duration_from_seconds)
                .map(PlaybackEvent::PositionChanged)
                .into_iter()
                .collect(),
            "duration" => vec![PlaybackEvent::DurationChanged(
                node_nonnegative_f64(change.value.as_ref())
                    .and_then(duration_from_seconds),
            )],
            "paused-for-cache" => {
                if let Some(paused_for_cache) = node_bool(change.value.as_ref())
                {
                    self.paused_for_cache = paused_for_cache;
                    self.buffering =
                        self.paused_for_cache || self.cache_underrun;
                    self.buffer.buffering = self.buffering;
                    let mut events =
                        vec![PlaybackEvent::BufferChanged(self.buffer.clone())];
                    if self.file_loaded && !self.terminal {
                        events.push(PlaybackEvent::StateChanged(
                            self.normal_state(),
                        ));
                    }
                    events
                } else {
                    Vec::new()
                }
            }
            "cache-buffering-state" => {
                self.buffer.percentage = node_f64(change.value.as_ref())
                    .filter(|value| value.is_finite())
                    .map(|value| (value / 100.0).clamp(0.0, 1.0));
                vec![PlaybackEvent::BufferChanged(self.buffer.clone())]
            }
            "demuxer-cache-duration" => {
                self.buffer.cached_duration =
                    node_nonnegative_f64(change.value.as_ref())
                        .and_then(duration_from_seconds);
                vec![PlaybackEvent::BufferChanged(self.buffer.clone())]
            }
            "demuxer-cache-state" => {
                if let Some(fields) = change.value.as_ref().and_then(node_map) {
                    self.buffer.cached_duration =
                        map_f64(fields, "cache-duration")
                            .and_then(duration_from_seconds);
                    if let Some(underrun) = map_bool(fields, "underrun") {
                        self.cache_underrun = underrun;
                        self.buffering =
                            self.paused_for_cache || self.cache_underrun;
                        self.buffer.buffering = self.buffering;
                    }
                } else {
                    self.buffer.cached_duration = None;
                }
                let mut events =
                    vec![PlaybackEvent::BufferChanged(self.buffer.clone())];
                if self.file_loaded && !self.terminal {
                    events
                        .push(PlaybackEvent::StateChanged(self.normal_state()));
                }
                events
            }
            "core-idle" => {
                if let Some(core_idle) = node_bool(change.value.as_ref()) {
                    self.core_idle = core_idle;
                }
                Vec::new()
            }
            "seeking" => {
                if let Some(seeking) = node_bool(change.value.as_ref()) {
                    self.seeking = seeking;
                    if self.file_loaded && !self.terminal {
                        return vec![PlaybackEvent::StateChanged(
                            self.normal_state(),
                        )];
                    }
                }
                Vec::new()
            }
            // End-file is the authoritative terminal event. Keeping these
            // observations still exposes state and catches future reducer use.
            "eof-reached" => Vec::new(),
            "idle-active" => {
                if node_bool(change.value.as_ref()) == Some(true)
                    && !self.terminal
                {
                    self.file_loaded = false;
                    vec![PlaybackEvent::StateChanged(PlaybackState::Idle)]
                } else {
                    Vec::new()
                }
            }
            "track-list" => {
                let (catalog, audio_ids, subtitle_ids, selected_video_codec) =
                    parse_track_list(change.value.as_ref());
                self.audio_ids = audio_ids;
                self.subtitle_ids = subtitle_ids;
                if let Some(codec) = selected_video_codec {
                    let video =
                        self.video.get_or_insert_with(VideoParameters::default);
                    video.codec = Some(codec.clone());
                    let input = self
                        .input_video
                        .get_or_insert_with(VideoParameters::default);
                    input.codec = Some(codec);
                }
                let mut events = vec![PlaybackEvent::TracksChanged(catalog)];
                if self.video.is_some() {
                    events.push(PlaybackEvent::VideoParametersChanged(
                        self.video.clone(),
                    ));
                }
                events
            }
            "chapter-list" => {
                let (chapters, chapter_ids) =
                    parse_chapters(change.value.as_ref());
                self.chapter_ids = chapter_ids;
                vec![
                    PlaybackEvent::ChaptersChanged(chapters),
                    PlaybackEvent::ChapterChanged(self.current_chapter()),
                ]
            }
            "chapter" => {
                self.current_chapter_id = node_i64(change.value.as_ref())
                    .filter(|native_id| *native_id >= 0);
                vec![PlaybackEvent::ChapterChanged(self.current_chapter())]
            }
            "edition-list" => {
                let (editions, edition_ids) =
                    parse_editions(change.value.as_ref());
                if self.current_edition_id.is_none() {
                    self.current_edition_id = editions
                        .iter()
                        .find(|edition| edition.is_default)
                        .and_then(|edition| edition_ids.get(&edition.id))
                        .copied();
                }
                self.edition_ids = edition_ids;
                vec![
                    PlaybackEvent::EditionsChanged(editions),
                    PlaybackEvent::EditionChanged(self.current_edition()),
                ]
            }
            "edition" => {
                if let Some(native_id) = node_i64(change.value.as_ref())
                    .filter(|native_id| *native_id >= 0)
                {
                    self.current_edition_id = Some(native_id);
                } else if self.edition_ids.is_empty() {
                    self.current_edition_id = None;
                }
                vec![PlaybackEvent::EditionChanged(self.current_edition())]
            }
            "video-params" => {
                self.input_video = parse_video_parameters(
                    change.value.as_ref(),
                    self.input_video.take(),
                );
                self.video = parse_video_parameters(
                    change.value.as_ref(),
                    self.video.take(),
                );
                vec![PlaybackEvent::VideoParametersChanged(self.video.clone())]
            }
            "video-out-params" => {
                self.output_video = parse_video_parameters(
                    change.value.as_ref(),
                    self.output_video.take(),
                );
                self.video = parse_video_parameters(
                    change.value.as_ref(),
                    self.video.take(),
                );
                vec![PlaybackEvent::VideoParametersChanged(self.video.clone())]
            }
            "vo-configured" => {
                self.vo_configured =
                    node_bool(change.value.as_ref()).unwrap_or(false);
                Vec::new()
            }
            "window-id" => {
                self.native_window_id = node_i64(change.value.as_ref())
                    .and_then(|value| {
                        normalize_native_window_id(
                            value,
                            cfg!(target_os = "macos"),
                        )
                    });
                Vec::new()
            }
            "current-vo" => {
                self.current_vo = node_string(change.value.as_ref());
                Vec::new()
            }
            "current-gpu-context" => {
                self.current_gpu_context = node_string(change.value.as_ref());
                if self.current_gpu_api.is_none() {
                    self.current_gpu_api = self
                        .current_gpu_context
                        .as_deref()
                        .and_then(gpu_api_from_context);
                }
                Vec::new()
            }
            "mpv-version" => {
                self.mpv_version = node_string(change.value.as_ref());
                Vec::new()
            }
            "ffmpeg-version" => {
                self.ffmpeg_version = node_string(change.value.as_ref());
                Vec::new()
            }
            "hwdec-current" => {
                let decoder = node_string(change.value.as_ref());
                self.hardware_decoder = decoder.clone();
                if decoder.is_some() || self.video.is_some() {
                    let video =
                        self.video.get_or_insert_with(VideoParameters::default);
                    video.hardware_decoder = decoder.clone();
                    let input = self
                        .input_video
                        .get_or_insert_with(VideoParameters::default);
                    input.hardware_decoder = decoder;
                    vec![PlaybackEvent::VideoParametersChanged(
                        self.video.clone(),
                    )]
                } else {
                    Vec::new()
                }
            }
            "hwdec-interop" => {
                self.hardware_decoder_interop =
                    node_string(change.value.as_ref());
                Vec::new()
            }
            "frame-drop-count" => {
                self.frame_drop_count =
                    node_nonnegative_u64(change.value.as_ref());
                Vec::new()
            }
            "decoder-frame-drop-count" => {
                self.decoder_frame_drop_count =
                    node_nonnegative_u64(change.value.as_ref());
                Vec::new()
            }
            "mistimed-frame-count" => {
                self.mistimed_frame_count =
                    node_nonnegative_u64(change.value.as_ref());
                Vec::new()
            }
            "vo-delayed-frame-count" => {
                self.delayed_frame_count =
                    node_nonnegative_u64(change.value.as_ref());
                Vec::new()
            }
            "avsync" => {
                self.av_sync_seconds = node_f64(change.value.as_ref())
                    .filter(|value| value.is_finite());
                Vec::new()
            }
            "volume" => node_f64(change.value.as_ref())
                .filter(|value| value.is_finite())
                .map(|value| PlaybackEvent::VolumeChanged(value / 100.0))
                .into_iter()
                .collect(),
            "mute" => node_bool(change.value.as_ref())
                .map(PlaybackEvent::MutedChanged)
                .into_iter()
                .collect(),
            "speed" => node_f64(change.value.as_ref())
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(PlaybackEvent::SpeedChanged)
                .into_iter()
                .collect(),
            "fullscreen" => node_bool(change.value.as_ref())
                .map(PlaybackEvent::FullscreenChanged)
                .into_iter()
                .collect(),
            "glsl-shaders" => {
                self.active_video_shader_count = match change.value.as_ref() {
                    Some(MpvNode::Array(shaders))
                        if shaders.iter().all(|shader| {
                            matches!(shader, MpvNode::String(_))
                        }) =>
                    {
                        Some(shaders.len())
                    }
                    Some(MpvNode::String(shaders)) if shaders.is_empty() => {
                        Some(0)
                    }
                    Some(MpvNode::Null) => Some(0),
                    _ => None,
                };
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn current_chapter(&self) -> Option<ChapterId> {
        let native_id = self.current_chapter_id?;
        self.chapter_ids.iter().find_map(|(id, candidate)| {
            (*candidate == native_id).then(|| id.clone())
        })
    }

    fn current_edition(&self) -> Option<EditionId> {
        let native_id = self.current_edition_id?;
        self.edition_ids.iter().find_map(|(id, candidate)| {
            (*candidate == native_id).then(|| id.clone())
        })
    }

    fn normal_state(&self) -> PlaybackState {
        if self.stopping {
            PlaybackState::Stopping
        } else if self.seeking {
            PlaybackState::Seeking
        } else if self.paused {
            // Explicit user pause takes precedence over cache state so the
            // backend-neutral play/pause toggle retains the correct intent.
            PlaybackState::Paused
        } else if self.buffering {
            PlaybackState::Buffering
        } else {
            PlaybackState::Playing
        }
    }
}

fn build_external_subtitle_command(
    source: &PlaybackFilePath,
    select: bool,
) -> Result<Vec<String>, PlaybackError> {
    Ok(vec![
        "sub-add".to_string(),
        local_path_argument("external subtitle path", source)?,
        if select { "select" } else { "auto" }.to_string(),
    ])
}

fn build_apply_profile_command(
    profile: &VideoProfileName,
) -> Result<Vec<String>, PlaybackError> {
    let name =
        validate_extension_text("video profile name", profile.as_str(), 256)?;
    Ok(vec!["apply-profile".to_string(), name, "apply".to_string()])
}

fn build_shader_commands(
    shaders: &[PlaybackFilePath],
) -> Result<Vec<Vec<String>>, PlaybackError> {
    if shaders.is_empty() {
        return Ok(vec![vec![
            "change-list".to_string(),
            "glsl-shaders".to_string(),
            "clr".to_string(),
            String::new(),
        ]]);
    }

    shaders
        .iter()
        .enumerate()
        .map(|(index, path)| {
            Ok(vec![
                "change-list".to_string(),
                "glsl-shaders".to_string(),
                if index == 0 { "set" } else { "append" }.to_string(),
                local_path_argument("video shader path", path)?,
            ])
        })
        .collect()
}

fn build_screenshot_command(
    output: &PlaybackFilePath,
    mode: PlaybackScreenshotMode,
) -> Result<Vec<String>, PlaybackError> {
    let mode = match mode {
        PlaybackScreenshotMode::VideoOnly => "video",
        PlaybackScreenshotMode::VideoWithSubtitles => "subtitles",
        PlaybackScreenshotMode::Window => "window",
    };
    Ok(vec![
        "screenshot-to-file".to_string(),
        local_path_argument("screenshot output path", output)?,
        mode.to_string(),
    ])
}

fn local_path_argument(
    category: &'static str,
    path: &PlaybackFilePath,
) -> Result<String, PlaybackError> {
    let Some(path) = path.as_path().to_str() else {
        return Err(mpv_error(
            PlaybackErrorKind::Command,
            format!("{category} must be valid Unicode for libmpv"),
            false,
        ));
    };
    validate_extension_text(category, path, 32 * 1024)
}

fn validate_extension_text(
    category: &'static str,
    value: &str,
    maximum_bytes: usize,
) -> Result<String, PlaybackError> {
    if value.is_empty() {
        return Err(mpv_error(
            PlaybackErrorKind::Command,
            format!("{category} must not be empty"),
            false,
        ));
    }
    if value.len() > maximum_bytes {
        return Err(mpv_error(
            PlaybackErrorKind::Command,
            format!("{category} exceeds the supported length"),
            false,
        ));
    }
    if value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n')) {
        return Err(mpv_error(
            PlaybackErrorKind::Command,
            format!("{category} contains a forbidden control character"),
            false,
        ));
    }
    Ok(value.to_string())
}

fn build_load_command(
    source: &PlaybackSource,
    start: Duration,
) -> Result<MpvNode, PlaybackError> {
    let mut options = Vec::new();
    if start > Duration::ZERO {
        options.push((
            "start".to_string(),
            MpvNode::String(start.as_secs_f64().to_string()),
        ));
    }
    if let Some(title) = source.title() {
        validate_single_line("media title", title)?;
        options.push((
            "force-media-title".to_string(),
            MpvNode::String(title.to_string()),
        ));
    }

    let mut headers = Vec::new();
    for header in source.headers() {
        validate_header_name(&header.name)?;
        let value = header.value.expose_secret();
        validate_single_line("HTTP header value", value)?;
        headers.push(format!("{}: {value}", header.name));
    }
    if !source.cookies().is_empty() {
        let mut cookies = Vec::new();
        for cookie in source.cookies() {
            validate_cookie_name(&cookie.name)?;
            let value = cookie.value.expose_secret();
            validate_single_line("cookie value", value)?;
            cookies.push(format!("{}={value}", cookie.name));
        }
        headers.push(format!("Cookie: {}", cookies.join("; ")));
    }
    if !headers.is_empty() {
        options.push((
            "http-header-fields".to_string(),
            MpvNode::String(encode_mpv_string_list(&headers)),
        ));
    }

    Ok(MpvNode::Array(vec![
        MpvNode::String("loadfile".to_string()),
        MpvNode::String(source.uri().as_str().to_string()),
        MpvNode::String("replace".to_string()),
        MpvNode::Int(-1),
        MpvNode::Map(options),
    ]))
}

fn encode_mpv_string_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| value.replace('\\', "\\\\").replace(',', "\\,"))
        .collect::<Vec<_>>()
        .join(",")
}

fn validate_header_name(name: &str) -> Result<(), PlaybackError> {
    if name.is_empty()
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
    {
        return Err(mpv_error(
            PlaybackErrorKind::InvalidSource,
            "HTTP header name is not a valid token",
            false,
        ));
    }
    Ok(())
}

fn validate_cookie_name(name: &str) -> Result<(), PlaybackError> {
    if name.is_empty()
        || name.bytes().any(|byte| {
            byte.is_ascii_control()
                || matches!(byte, b' ' | b'\t' | b'=' | b';' | b',')
        })
    {
        return Err(mpv_error(
            PlaybackErrorKind::InvalidSource,
            "cookie name contains an invalid character",
            false,
        ));
    }
    Ok(())
}

fn validate_single_line(
    category: &'static str,
    value: &str,
) -> Result<(), PlaybackError> {
    if value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n')) {
        return Err(mpv_error(
            PlaybackErrorKind::InvalidSource,
            format!("{category} contains a forbidden control character"),
            false,
        ));
    }
    Ok(())
}

struct MpvSourceRedactor {
    source_values: Vec<Zeroizing<String>>,
    local_values: Vec<Zeroizing<String>>,
}

impl MpvSourceRedactor {
    const MAX_LOCAL_VALUES: usize = 4_096;

    fn new(source: &PlaybackSource) -> Self {
        let mut source_values = Vec::new();
        source_values.push(Zeroizing::new(source.uri().as_str().to_string()));
        if let Some(password) = source.uri().password()
            && !password.is_empty()
        {
            source_values.push(Zeroizing::new(password.to_string()));
        }
        for (_, value) in source.uri().query_pairs() {
            if !value.is_empty() {
                source_values.push(Zeroizing::new(value.into_owned()));
            }
        }
        source_values.extend(
            source
                .headers()
                .iter()
                .map(|header| {
                    Zeroizing::new(header.value.expose_secret().to_string())
                })
                .chain(source.cookies().iter().map(|cookie| {
                    Zeroizing::new(cookie.value.expose_secret().to_string())
                }))
                .filter(|value| !value.is_empty()),
        );
        Self {
            source_values,
            local_values: Vec::new(),
        }
    }

    fn replace_source(&mut self, source: &PlaybackSource) {
        // Runtime profiles and shader options survive replacement loads. Keep
        // their redactions, and retain prior source values because end-file
        // logs copied after `loadfile replace` may still mention the old URI.
        let mut values = Self::new(source).source_values;
        for previous in std::mem::take(&mut self.source_values) {
            if !values
                .iter()
                .any(|current| current.as_str() == previous.as_str())
            {
                values.push(previous);
            }
        }
        self.source_values = values;
    }

    fn remember_local_values<'a>(
        &mut self,
        values: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), PlaybackError> {
        let mut additions = Vec::new();
        for value in values {
            if value.is_empty()
                || self
                    .source_values
                    .iter()
                    .chain(&self.local_values)
                    .any(|existing| existing.as_str() == value)
                || additions.contains(&value)
            {
                continue;
            }
            additions.push(value);
        }
        if self.local_values.len() + additions.len() > Self::MAX_LOCAL_VALUES {
            return Err(mpv_error(
                PlaybackErrorKind::Command,
                "local extension redaction capacity exhausted; restart playback before submitting more distinct paths",
                false,
            ));
        }
        self.local_values.extend(
            additions
                .into_iter()
                .map(|value| Zeroizing::new(value.to_string())),
        );
        Ok(())
    }

    fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();
        for value in self.source_values.iter().chain(&self.local_values) {
            output = output.replace(value.as_str(), "<redacted>");
        }
        redact_playback_url(&output)
    }
}

fn parse_track_list(
    value: Option<&MpvNode>,
) -> (
    TrackCatalog,
    HashMap<TrackId, i64>,
    HashMap<TrackId, i64>,
    Option<String>,
) {
    let Some(MpvNode::Array(tracks)) = value else {
        return (
            TrackCatalog::default(),
            HashMap::new(),
            HashMap::new(),
            None,
        );
    };

    let mut catalog = TrackCatalog::default();
    let mut audio_ids = HashMap::new();
    let mut subtitle_ids = HashMap::new();
    let mut occurrences = HashMap::<String, usize>::new();
    let mut selected_video_codec = None;

    for track in tracks {
        let Some(fields) = node_map(track) else {
            continue;
        };
        let Some(native_id) = map_i64(fields, "id") else {
            continue;
        };
        let kind = map_str(fields, "type").unwrap_or_default();
        let title = map_owned_string(fields, "title");
        let language = map_owned_string(fields, "lang");
        let codec = map_owned_string(fields, "codec");
        let selected = map_bool(fields, "selected").unwrap_or(false);
        let source_id = map_i64(fields, "src-id");

        if kind == "video" {
            if selected {
                selected_video_codec = codec;
            }
            continue;
        }
        if kind != "audio" && kind != "sub" {
            continue;
        }

        let base = format!(
            "mpv:{kind}:{}:{}:{}:{}",
            source_id
                .map_or_else(|| "src-_".to_string(), |id| format!("src-{id}")),
            identity_component(language.as_deref()),
            identity_component(title.as_deref()),
            identity_component(codec.as_deref()),
        );
        let occurrence = occurrences.entry(base.clone()).or_default();
        let id = TrackId::new(format!("{base}#{occurrence}"));
        *occurrence += 1;

        if kind == "audio" {
            audio_ids.insert(id.clone(), native_id);
            if selected {
                catalog.selected_audio = Some(id.clone());
            }
            catalog.audio.push(AudioTrack {
                id,
                title,
                language,
                codec,
                channels: map_i64(fields, "demux-channel-count")
                    .or_else(|| map_i64(fields, "audio-channels"))
                    .and_then(|value| u16::try_from(value).ok()),
                sample_rate: map_i64(fields, "demux-samplerate")
                    .and_then(|value| u32::try_from(value).ok()),
                is_default: map_bool(fields, "default").unwrap_or(false),
                is_forced: map_bool(fields, "forced").unwrap_or(false),
            });
        } else {
            subtitle_ids.insert(id.clone(), native_id);
            let is_primary = map_i64(fields, "main-selection")
                .is_none_or(|selection| selection == 0);
            if selected && is_primary {
                catalog.selected_subtitle = Some(id.clone());
            }
            let subtitle_kind = subtitle_kind(codec.as_deref());
            catalog.subtitles.push(SubtitleTrack {
                id,
                title,
                language,
                codec,
                kind: subtitle_kind,
                is_default: map_bool(fields, "default").unwrap_or(false),
                is_forced: map_bool(fields, "forced").unwrap_or(false),
                is_external: map_bool(fields, "external").unwrap_or(false),
            });
        }
    }

    (catalog, audio_ids, subtitle_ids, selected_video_codec)
}

fn parse_chapters(
    value: Option<&MpvNode>,
) -> (Vec<Chapter>, HashMap<ChapterId, i64>) {
    let Some(MpvNode::Array(chapters)) = value else {
        return (Vec::new(), HashMap::new());
    };
    let mut parsed = chapters
        .iter()
        .enumerate()
        .filter_map(|(native_index, chapter)| {
            let native_index = i64::try_from(native_index).ok()?;
            let fields = node_map(chapter)?;
            let seconds = map_f64(fields, "time")?;
            let start = duration_from_seconds(seconds)?;
            Some((native_index, map_owned_string(fields, "title"), start))
        })
        .collect::<Vec<_>>();
    parsed.sort_by_key(|(_, _, start)| *start);

    let mut native_ids = HashMap::new();
    let chapters = parsed
        .iter()
        .enumerate()
        .map(|(sorted_index, (native_index, title, start))| {
            // The public identity follows presentation order and timestamp;
            // the separate map retains mpv's native list index for commands.
            let id = ChapterId::new(format!(
                "mpv:chapter:{sorted_index}:{}",
                start.as_millis()
            ));
            native_ids.insert(id.clone(), *native_index);
            Chapter {
                id,
                title: title.clone(),
                start: *start,
                end: parsed.get(sorted_index + 1).map(|(_, _, start)| *start),
            }
        })
        .collect();
    (chapters, native_ids)
}

fn parse_editions(
    value: Option<&MpvNode>,
) -> (Vec<Edition>, HashMap<EditionId, i64>) {
    let Some(MpvNode::Array(editions)) = value else {
        return (Vec::new(), HashMap::new());
    };
    let mut native_ids = HashMap::new();
    let editions = editions
        .iter()
        .filter_map(|edition| {
            let fields = node_map(edition)?;
            let native_id = map_i64(fields, "id")?;
            let id = EditionId::new(format!("mpv:edition:{native_id}"));
            native_ids.insert(id.clone(), native_id);
            Some(Edition {
                id,
                title: map_owned_string(fields, "title"),
                is_default: map_bool(fields, "default").unwrap_or(false),
            })
        })
        .collect();
    (editions, native_ids)
}

fn parse_video_parameters(
    value: Option<&MpvNode>,
    previous: Option<VideoParameters>,
) -> Option<VideoParameters> {
    let fields = value.and_then(node_map)?;
    let mut video = previous.unwrap_or_default();
    video.width =
        map_i64(fields, "w").and_then(|value| u32::try_from(value).ok());
    video.height =
        map_i64(fields, "h").and_then(|value| u32::try_from(value).ok());
    video.pixel_format = map_owned_string(fields, "pixelformat")
        .or_else(|| map_owned_string(fields, "hw-pixelformat"));
    video.bit_depth = video
        .pixel_format
        .as_deref()
        .and_then(pixel_format_bit_depth);
    video.color_primaries = map_owned_string(fields, "primaries");
    video.color_transfer = map_owned_string(fields, "gamma");
    video.color_matrix = map_owned_string(fields, "colormatrix");
    video.hdr_metadata_observed =
        ["min-luma", "max-luma", "max-cll", "max-fall", "max-pq-y"]
            .iter()
            .any(|key| map_f64(fields, key).is_some())
            || video.color_primaries.as_deref().is_some_and(|value| {
                value.to_ascii_lowercase().contains("2020")
            })
            || video.color_transfer.as_deref().is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("pq")
                    || value.contains("2084")
                    || value.contains("hlg")
                    || value.contains("arib")
            });
    Some(video)
}

fn pixel_format_bit_depth(format: &str) -> Option<u8> {
    let format = format.to_ascii_lowercase();
    for (marker, depth) in [
        ("p016", 16),
        ("p012", 12),
        ("p010", 10),
        ("p16", 16),
        ("p14", 14),
        ("p12", 12),
        ("p10", 10),
        ("p9", 9),
    ] {
        if format.contains(marker) {
            return Some(depth);
        }
    }
    matches!(
        format.as_str(),
        "nv12" | "yuv420p" | "yuv422p" | "yuv444p" | "rgb24" | "rgba"
    )
    .then_some(8)
}

fn subtitle_kind(codec: Option<&str>) -> SubtitleKind {
    let codec = codec.unwrap_or_default().to_ascii_lowercase();
    if [
        "ass", "ssa", "subrip", "srt", "webvtt", "mov_text", "text", "microdvd",
    ]
    .iter()
    .any(|candidate| codec.contains(candidate))
    {
        SubtitleKind::Text
    } else if ["pgs", "dvd_subtitle", "dvb_subtitle", "xsub", "vobsub"]
        .iter()
        .any(|candidate| codec.contains(candidate))
    {
        SubtitleKind::Bitmap
    } else {
        SubtitleKind::Unknown
    }
}

fn identity_component(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '_' | '.')
                    {
                        character.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| "_".to_string())
}

fn node_map(node: &MpvNode) -> Option<&[(String, MpvNode)]> {
    match node {
        MpvNode::Map(values) => Some(values),
        _ => None,
    }
}

fn map_value<'a>(
    values: &'a [(String, MpvNode)],
    key: &str,
) -> Option<&'a MpvNode> {
    values
        .iter()
        .find_map(|(candidate, value)| (candidate == key).then_some(value))
}

fn map_str<'a>(values: &'a [(String, MpvNode)], key: &str) -> Option<&'a str> {
    match map_value(values, key) {
        Some(MpvNode::String(value)) => Some(value),
        _ => None,
    }
}

fn map_owned_string(values: &[(String, MpvNode)], key: &str) -> Option<String> {
    map_str(values, key).map(ToOwned::to_owned)
}

fn map_i64(values: &[(String, MpvNode)], key: &str) -> Option<i64> {
    match map_value(values, key) {
        Some(MpvNode::Int(value)) => Some(*value),
        _ => None,
    }
}

fn map_f64(values: &[(String, MpvNode)], key: &str) -> Option<f64> {
    node_f64(map_value(values, key))
}

fn map_bool(values: &[(String, MpvNode)], key: &str) -> Option<bool> {
    node_bool(map_value(values, key))
}

fn node_string(value: Option<&MpvNode>) -> Option<String> {
    match value {
        Some(MpvNode::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn node_bool(value: Option<&MpvNode>) -> Option<bool> {
    match value {
        Some(MpvNode::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn node_i64(value: Option<&MpvNode>) -> Option<i64> {
    match value {
        Some(MpvNode::Int(value)) => Some(*value),
        _ => None,
    }
}

fn normalize_native_window_id(
    value: i64,
    macos_pointer_bits: bool,
) -> Option<i64> {
    if value != 0 && (macos_pointer_bits || value > 0) {
        Some(value)
    } else {
        None
    }
}

fn native_window_id_refresh_needed(
    previous_vo_configured: bool,
    vo_configured: bool,
    native_window_id: Option<i64>,
    refresh_pending: bool,
) -> bool {
    !previous_vo_configured
        && vo_configured
        && native_window_id.is_none()
        && !refresh_pending
}

fn evaluate_native_window_id_refresh(
    ticket: NativeWindowIdRefreshTicket,
    current_output_epoch: u64,
    current_observation_revision: u64,
    request_is_current: bool,
    vo_configured: bool,
    current_native_window_id: Option<i64>,
    value: Option<&MpvNode>,
    macos_pointer_bits: bool,
) -> NativeWindowIdRefreshResult {
    let value = node_i64(value).and_then(|value| {
        normalize_native_window_id(value, macos_pointer_bits)
    });
    let stale = !request_is_current
        || ticket.output_epoch != current_output_epoch
        || ticket.observation_revision != current_observation_revision
        || !vo_configured
        || current_native_window_id.is_some();
    NativeWindowIdRefreshResult {
        value_available: value.is_some(),
        stale,
        applied: (!stale).then_some(value).flatten(),
    }
}

fn node_f64(value: Option<&MpvNode>) -> Option<f64> {
    match value {
        Some(MpvNode::Double(value)) => Some(*value),
        Some(MpvNode::Int(value)) => Some(*value as f64),
        _ => None,
    }
}

fn node_nonnegative_f64(value: Option<&MpvNode>) -> Option<f64> {
    node_f64(value).filter(|value| value.is_finite() && *value >= 0.0)
}

fn node_nonnegative_u64(value: Option<&MpvNode>) -> Option<u64> {
    match value {
        Some(MpvNode::Int(value)) => u64::try_from(*value).ok(),
        _ => None,
    }
}

fn extract_libplacebo_version(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let offset = lower.find("libplacebo")? + "libplacebo".len();
    text.get(offset..)?
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| {
                matches!(character, ':' | ',' | '(' | ')' | '[' | ']')
            })
        })
        .find(|token| {
            let version = token.strip_prefix('v').unwrap_or(token);
            !version.is_empty()
                && version
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_digit())
                && version.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '.' | '-' | '+' | '_')
                })
        })
        .map(ToOwned::to_owned)
}

fn parse_compiled_features(text: &str) -> Vec<String> {
    const MARKER: &str = "list of enabled features:";
    let lower = text.to_ascii_lowercase();
    let Some(offset) = lower.find(MARKER) else {
        return Vec::new();
    };
    text[offset + MARKER.len()..]
        .split_whitespace()
        .map(|feature| feature.trim_matches(','))
        .filter(|feature| {
            !feature.is_empty()
                && feature.len() <= 64
                && feature.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '_' | '.' | '+')
                })
        })
        .take(256)
        .map(ToOwned::to_owned)
        .collect()
}

fn gpu_api_from_log_prefix(prefix: &str) -> Option<String> {
    prefix
        .split('/')
        .map(str::to_ascii_lowercase)
        .find(|component| {
            matches!(
                component.as_str(),
                "vulkan" | "opengl" | "d3d11" | "metal"
            )
        })
}

fn gpu_api_from_context(context: &str) -> Option<String> {
    let context = context.to_ascii_lowercase();
    if context.contains("vulkan") || context.contains("vk") {
        Some("vulkan".to_string())
    } else if context.contains("d3d") {
        Some("d3d11".to_string())
    } else if context.contains("metal") {
        Some("metal".to_string())
    } else if ["opengl", "angle", "egl", "wayland", "x11"]
        .iter()
        .any(|marker| context.contains(marker))
    {
        Some("opengl".to_string())
    } else {
        None
    }
}

fn gpu_adapter_from_log(prefix: &str, text: &str) -> Option<String> {
    let prefix = prefix.to_ascii_lowercase();
    if !["vo/", "gpu", "libplacebo"]
        .iter()
        .any(|marker| prefix.contains(marker))
    {
        return None;
    }

    [
        "GL_RENDERER=",
        "GL_RENDERER:",
        "Vulkan device:",
        "deviceName:",
        "Device name:",
        "D3D11 adapter:",
        "Metal device:",
    ]
    .iter()
    .find_map(|marker| diagnostic_value_after_marker(text, marker))
}

fn diagnostic_value_after_marker(text: &str, marker: &str) -> Option<String> {
    let offset = text.find(marker)? + marker.len();
    let value = text[offset..]
        .lines()
        .next()?
        .trim()
        .trim_matches(|character| matches!(character, '\'' | '"'));
    (!value.is_empty()
        && value.len() <= 512
        && !value.chars().any(char::is_control))
    .then(|| value.to_string())
}

fn duration_from_seconds(seconds: f64) -> Option<Duration> {
    Duration::try_from_secs_f64(seconds).ok()
}

fn finite_seconds(duration: Duration) -> Result<String, PlaybackError> {
    let seconds = duration.as_secs_f64();
    if seconds.is_finite() {
        Ok(seconds.to_string())
    } else {
        Err(mpv_error(
            PlaybackErrorKind::Command,
            "seek position must be finite",
            false,
        ))
    }
}

fn content_fit_properties(
    content_fit: crate::contract::PlaybackContentFit,
) -> [(&'static str, MpvNode); 3] {
    use crate::contract::PlaybackContentFit;

    let (keep_aspect, video_unscaled, panscan) = match content_fit {
        PlaybackContentFit::Contain => (true, "no", 0.0),
        PlaybackContentFit::Cover => (true, "no", 1.0),
        PlaybackContentFit::Fill => (false, "no", 0.0),
        PlaybackContentFit::None => (true, "yes", 0.0),
        PlaybackContentFit::ScaleDown => (true, "downscale-big", 0.0),
    };

    [
        ("keepaspect", MpvNode::Bool(keep_aspect)),
        (
            "video-unscaled",
            MpvNode::String(video_unscaled.to_string()),
        ),
        ("panscan", MpvNode::Double(panscan)),
    ]
}

fn mpv_configuration_diagnostics(
    policy: MpvConfigPolicy,
    logging: MpvLoggingPolicy,
    osc_enabled: bool,
    input_bindings_enabled: bool,
) -> MpvConfigurationDiagnostics {
    MpvConfigurationDiagnostics {
        policy: match policy {
            MpvConfigPolicy::Deterministic => {
                MpvConfigurationPolicy::Deterministic
            }
            MpvConfigPolicy::TrustedUser => MpvConfigurationPolicy::TrustedUser,
        },
        user_config_enabled: policy.user_config_enabled(),
        user_scripts_enabled: policy.user_scripts_enabled(),
        osc_enabled,
        input_bindings_enabled,
        external_url_resolver_enabled: false,
        log_verbosity: diagnostic_log_verbosity(logging.steady),
        startup_verbose_capture: logging.startup_verbose_capture,
        active_video_shader_count: None,
    }
}

fn mpv_native_controls_enabled(target: PlaybackTarget) -> bool {
    target == PlaybackTarget::MPV_NATIVE_WINDOW
}

const fn diagnostic_log_verbosity(level: MpvLogLevel) -> MpvLogVerbosity {
    match level {
        MpvLogLevel::None => MpvLogVerbosity::None,
        MpvLogLevel::Fatal => MpvLogVerbosity::Fatal,
        MpvLogLevel::Error => MpvLogVerbosity::Error,
        MpvLogLevel::Warn => MpvLogVerbosity::Warn,
        MpvLogLevel::Info => MpvLogVerbosity::Info,
        MpvLogLevel::Verbose => MpvLogVerbosity::Verbose,
        MpvLogLevel::Debug => MpvLogVerbosity::Debug,
        MpvLogLevel::Trace => MpvLogVerbosity::Trace,
    }
}

fn parse_mpv_logging_policy(
    value: Option<&OsStr>,
) -> Result<MpvLoggingPolicy, ()> {
    let level = match value.and_then(OsStr::to_str) {
        None if value.is_none() => return Ok(MpvLoggingPolicy::default()),
        Some("none") => MpvLogLevel::None,
        Some("fatal") => MpvLogLevel::Fatal,
        Some("error") => MpvLogLevel::Error,
        Some("warn") => MpvLogLevel::Warn,
        Some("info") => MpvLogLevel::Info,
        Some("verbose") => MpvLogLevel::Verbose,
        Some("debug") => MpvLogLevel::Debug,
        Some("trace") => MpvLogLevel::Trace,
        Some(_) | None => return Err(()),
    };
    Ok(MpvLoggingPolicy::fixed(level))
}

fn configured_mpv_logging_policy() -> MpvLoggingPolicy {
    match parse_mpv_logging_policy(
        std::env::var_os(MPV_LOG_LEVEL_ENV).as_deref(),
    ) {
        Ok(policy) => policy,
        Err(()) => {
            // Never echo an invalid value: environment configuration can be
            // populated accidentally with sensitive material.
            log::warn!(
                "Ignoring invalid {MPV_LOG_LEVEL_ENV}; expected none, fatal, error, warn, info, verbose, debug, or trace"
            );
            MpvLoggingPolicy::default()
        }
    }
}

fn parse_mpv_config_policy(
    value: Option<&OsStr>,
) -> Result<MpvConfigPolicy, ()> {
    match value.and_then(OsStr::to_str) {
        None if value.is_none() => Ok(MpvConfigPolicy::Deterministic),
        Some("deterministic") => Ok(MpvConfigPolicy::Deterministic),
        Some("trusted-user") => Ok(MpvConfigPolicy::TrustedUser),
        Some(_) | None => Err(()),
    }
}

fn configured_mpv_config_policy() -> MpvConfigPolicy {
    match parse_mpv_config_policy(
        std::env::var_os(MPV_CONFIG_POLICY_ENV).as_deref(),
    ) {
        Ok(MpvConfigPolicy::TrustedUser) => {
            log::warn!(
                "{MPV_CONFIG_POLICY_ENV}=trusted-user enables trusted mpv config and scripts inside the Ferrex process"
            );
            MpvConfigPolicy::TrustedUser
        }
        Ok(policy) => policy,
        Err(()) => {
            // Do not echo an arbitrary environment value into diagnostics: it
            // may have been populated accidentally with sensitive material.
            log::warn!(
                "Ignoring invalid {MPV_CONFIG_POLICY_ENV}; expected deterministic or trusted-user"
            );
            MpvConfigPolicy::Deterministic
        }
    }
}

fn mpv_capabilities(config_policy: MpvConfigPolicy) -> PlaybackCapabilities {
    PlaybackCapabilities {
        seek: true,
        audio_track_selection: true,
        subtitle_track_selection: true,
        external_subtitle_loading: true,
        chapter_selection: true,
        edition_selection: true,
        speed: true,
        content_fit: true,
        fullscreen: true,
        screenshot: true,
        video_shader_passthrough: true,
        // Named user profiles come from standard mpv config and therefore
        // exist only under the explicit trusted-code policy.
        video_profile_passthrough: config_policy.user_config_enabled(),
        integrated_presentation: false,
        native_window_fallback: true,
        // These are observations, not promises based only on backend choice.
        native_hdr: false,
        fractional_scaling: false,
    }
}

fn unsupported_mpv_extension(message: &'static str) -> PlaybackError {
    let mut error =
        PlaybackError::new(PlaybackErrorKind::UnsupportedOperation, message);
    error.backend = Some(BackendKind::Mpv);
    error
}

fn worker_error(
    kind: PlaybackErrorKind,
    context: &str,
    error: impl std::fmt::Display,
) -> PlaybackError {
    mpv_error(kind, format!("{context}: {error}"), true)
}

fn native_error(
    kind: PlaybackErrorKind,
    context: &str,
    code: i32,
    recoverable: bool,
) -> PlaybackError {
    let mut error = mpv_error(
        kind,
        format!("{context} (libmpv error {code})"),
        recoverable,
    );
    error.code = Some(i64::from(code));
    error
}

fn mpv_error(
    kind: PlaybackErrorKind,
    message: impl Into<String>,
    recoverable: bool,
) -> PlaybackError {
    let mut error = PlaybackError::new(kind, message);
    error.backend = Some(BackendKind::Mpv);
    error.recoverable = recoverable;
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{DurationDelta, PlaybackContentFit};
    use ferrex_player_mpv::{MpvEndFile, MpvObservationId};

    fn property(name: &str, value: Option<MpvNode>) -> MpvPropertyChange {
        MpvPropertyChange {
            id: MpvObservationId::new(1),
            name: name.to_string(),
            value,
            registered: true,
        }
    }

    fn map(entries: Vec<(&str, MpvNode)>) -> MpvNode {
        MpvNode::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    #[derive(Debug, Clone, Copy)]
    struct ProcessResourceSample {
        resident_kib: u64,
        open_fds: usize,
    }

    #[cfg(target_os = "linux")]
    fn process_resource_sample() -> Option<ProcessResourceSample> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let resident_kib = status.lines().find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_ascii_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })?;
        let open_fds = std::fs::read_dir("/proc/self/fd").ok()?.count();
        Some(ProcessResourceSample {
            resident_kib,
            open_fds,
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn process_resource_sample() -> Option<ProcessResourceSample> {
        None
    }

    fn optional_stress_limit(name: &str) -> Option<u64> {
        std::env::var(name).ok().map(|value| {
            value
                .parse::<u64>()
                .unwrap_or_else(|_| panic!("{name} is a non-negative integer"))
        })
    }

    fn smoke_source_from_environment(title: &str) -> PlaybackSource {
        let source = if let Ok(url) = std::env::var("FERREX_MPV_SMOKE_URL") {
            let mut source = PlaybackSource::new(
                url.parse().expect("FERREX_MPV_SMOKE_URL is a valid URL"),
            );
            if let Ok(value) = std::env::var("FERREX_MPV_SMOKE_AUTHORIZATION") {
                source = source.with_header("Authorization", value);
            }
            if let Ok(value) = std::env::var("FERREX_MPV_SMOKE_COOKIE") {
                source = source.with_cookie("session", value);
            }
            source
        } else {
            let path = std::env::var("FERREX_MPV_SMOKE_MEDIA")
                .expect("set FERREX_MPV_SMOKE_MEDIA or FERREX_MPV_SMOKE_URL");
            let uri = url::Url::from_file_path(
                std::fs::canonicalize(path).expect("fixture path exists"),
            )
            .expect("fixture path converts to a file URL");
            PlaybackSource::new(uri)
        };
        source.with_title(title)
    }

    fn wait_for_smoke_playback(
        adapter: &mut MpvPlaybackAdapter,
        operation: &str,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            adapter.poll_events();
            match adapter.snapshot().state {
                PlaybackState::Playing | PlaybackState::Paused
                    if adapter.snapshot().duration.is_some() =>
                {
                    break;
                }
                PlaybackState::Failed => panic!(
                    "mpv {operation} failed: {:?}",
                    adapter.snapshot().last_error
                ),
                _ if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                state => panic!("mpv {operation} timed out: {state:?}"),
            }
        }
    }

    #[test]
    fn absolute_seek_coalescer_bounds_requests_and_keeps_latest_position() {
        let first = MpvRequestId::new(10);
        let second = MpvRequestId::new(11);
        let unrelated = MpvRequestId::new(99);
        let mut coalescer = AbsoluteSeekCoalescer::default();

        assert_eq!(
            coalescer.enqueue(Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        coalescer.submitted(first);
        assert_eq!(coalescer.enqueue(Duration::from_secs(2)), None);
        assert_eq!(coalescer.enqueue(Duration::from_secs(3)), None);

        assert_eq!(coalescer.completed(unrelated), None);
        assert_eq!(coalescer.active, Some(first));
        assert_eq!(coalescer.queued, Some(Duration::from_secs(3)));

        assert_eq!(coalescer.completed(first), Some(Duration::from_secs(3)));
        coalescer.submitted(second);
        assert_eq!(coalescer.completed(second), None);
        assert_eq!(coalescer.active, None);
        assert_eq!(coalescer.queued, None);
    }

    #[test]
    fn clearing_absolute_seek_coalescer_rejects_late_replies() {
        let request = MpvRequestId::new(12);
        let mut coalescer = AbsoluteSeekCoalescer::default();
        assert!(coalescer.enqueue(Duration::from_secs(4)).is_some());
        coalescer.submitted(request);
        assert!(coalescer.enqueue(Duration::from_secs(5)).is_none());

        coalescer.clear();

        assert_eq!(coalescer.completed(request), None);
        assert_eq!(coalescer.active, None);
        assert_eq!(coalescer.queued, None);
    }

    #[test]
    fn authenticated_load_uses_per_file_options_and_rejects_injection() {
        let source = PlaybackSource::new(
            "https://ferrex.example/media?id=1&access_token=query-secret"
                .parse()
                .unwrap(),
        )
        .with_title("Episode 1")
        .with_header("Authorization", "Bearer header-secret")
        .with_header("X-Comma", "one,two")
        .with_cookie("session", "cookie-secret");

        let command =
            build_load_command(&source, Duration::from_millis(2_500)).unwrap();
        let MpvNode::Array(arguments) = command else {
            panic!("load command must be an array")
        };
        assert_eq!(arguments[0], MpvNode::String("loadfile".into()));
        assert_eq!(arguments[2], MpvNode::String("replace".into()));
        assert_eq!(arguments[3], MpvNode::Int(-1));
        let MpvNode::Map(options) = &arguments[4] else {
            panic!("per-file options must be a map")
        };
        assert_eq!(map_str(options, "start"), Some("2.5"));
        let headers = map_str(options, "http-header-fields").unwrap();
        assert!(headers.contains("Authorization: Bearer header-secret"));
        assert!(headers.contains("X-Comma: one\\,two"));
        assert!(headers.contains("Cookie: session=cookie-secret"));

        let injected = PlaybackSource::new(
            "https://ferrex.example/media".parse().unwrap(),
        )
        .with_header("Authorization", "safe\r\nX-Injected: secret");
        let error = build_load_command(&injected, Duration::ZERO).unwrap_err();
        assert_eq!(error.kind, PlaybackErrorKind::InvalidSource);
        assert!(!error.message.contains("secret"));
    }

    #[test]
    fn source_aware_log_redaction_removes_url_and_custom_secrets() {
        let source = PlaybackSource::new(
            "https://user:password@example.test/private?ticket=query-secret"
                .parse()
                .unwrap(),
        )
        .with_header("X-Private", "header-secret")
        .with_cookie("session", "cookie-secret");
        let mut redactor = MpvSourceRedactor::new(&source);
        redactor
            .remember_local_values([
                "/home/private-user/shaders/private-name.hook",
                "private-profile",
            ])
            .unwrap();
        let output = redactor.redact(&format!(
            "opening {} X-Private=header-secret Cookie=session=cookie-secret password shader=/home/private-user/shaders/private-name.hook profile=private-profile",
            source.uri()
        ));

        for secret in [
            "password",
            "query-secret",
            "header-secret",
            "cookie-secret",
            "/private",
            "private-user",
            "private-name",
            "private-profile",
        ] {
            assert!(!output.contains(secret), "log leaked {secret}");
        }

        let replacement = PlaybackSource::new(
            "https://example.test/replacement?ticket=new-secret"
                .parse()
                .unwrap(),
        );
        redactor.replace_source(&replacement);
        let replacement_log = redactor.redact(
            "new-secret query-secret /home/private-user/shaders/private-name.hook private-profile",
        );
        assert!(!replacement_log.contains("new-secret"));
        assert!(!replacement_log.contains("query-secret"));
        assert!(!replacement_log.contains("private-user"));
        assert!(!replacement_log.contains("private-profile"));
    }

    #[test]
    fn observed_core_properties_map_to_contract_state() {
        let mut mapper = MpvEventMapper::default();
        assert_eq!(
            mapper.map_event(&MpvEvent::StartFile {
                playlist_entry_id: 1,
            }),
            vec![
                PlaybackEvent::StateChanged(PlaybackState::Loading),
                PlaybackEvent::TracksChanged(TrackCatalog::default()),
                PlaybackEvent::ChaptersChanged(Vec::new()),
                PlaybackEvent::ChapterChanged(None),
                PlaybackEvent::EditionsChanged(Vec::new()),
                PlaybackEvent::EditionChanged(None),
                PlaybackEvent::VideoParametersChanged(None),
                PlaybackEvent::DurationChanged(None),
            ]
        );
        mapper.map_property(&property("pause", Some(MpvNode::Bool(true))));
        assert_eq!(
            mapper.map_event(&MpvEvent::FileLoaded),
            vec![PlaybackEvent::StateChanged(PlaybackState::Paused)]
        );
        assert_eq!(
            mapper.map_property(&property(
                "time-pos",
                Some(MpvNode::Double(12.25)),
            )),
            vec![PlaybackEvent::PositionChanged(Duration::from_millis(
                12_250
            ))]
        );
        assert_eq!(
            mapper.map_property(&property(
                "duration",
                Some(MpvNode::Double(90.0)),
            )),
            vec![PlaybackEvent::DurationChanged(Some(Duration::from_secs(
                90
            )))]
        );
        mapper.map_property(&property("pause", Some(MpvNode::Bool(false))));
        let buffering = mapper.map_property(&property(
            "paused-for-cache",
            Some(MpvNode::Bool(true)),
        ));
        assert!(
            buffering.contains(&PlaybackEvent::StateChanged(
                PlaybackState::Buffering
            ))
        );
        let cache = mapper.map_property(&property(
            "demuxer-cache-state",
            Some(map(vec![
                ("cache-duration", MpvNode::Double(4.5)),
                ("underrun", MpvNode::Bool(false)),
            ])),
        ));
        assert!(cache.contains(&PlaybackEvent::BufferChanged(BufferState {
            buffering: true,
            percentage: None,
            cached_duration: Some(Duration::from_millis(4_500)),
        })));
        assert_eq!(
            mapper
                .map_property(&property("seeking", Some(MpvNode::Bool(true)),))
                .last(),
            Some(&PlaybackEvent::StateChanged(PlaybackState::Seeking))
        );

        let pointer_width_value = i64::from(u32::MAX) + 17;
        assert!(
            mapper
                .map_property(&property(
                    "window-id",
                    Some(MpvNode::Int(pointer_width_value)),
                ))
                .is_empty()
        );
        assert_eq!(mapper.native_window_id, Some(pointer_width_value));
        assert_eq!(normalize_native_window_id(i64::MIN, true), Some(i64::MIN));
        assert_eq!(normalize_native_window_id(i64::MIN, false), None);
        assert_eq!(normalize_native_window_id(0, true), None);
    }

    #[test]
    fn native_window_id_refresh_is_requested_only_on_a_missing_rising_edge() {
        assert!(native_window_id_refresh_needed(false, true, None, false));
        assert!(!native_window_id_refresh_needed(true, true, None, false));
        assert!(!native_window_id_refresh_needed(
            false,
            true,
            Some(7),
            false
        ));
        assert!(!native_window_id_refresh_needed(false, true, None, true));
        assert!(!native_window_id_refresh_needed(false, false, None, false));
    }

    #[test]
    fn native_window_id_refresh_never_overwrites_newer_observations() {
        let ticket = NativeWindowIdRefreshTicket {
            output_epoch: 4,
            observation_revision: 8,
        };
        let value = MpvNode::Int(42);
        let current = evaluate_native_window_id_refresh(
            ticket,
            4,
            8,
            true,
            true,
            None,
            Some(&value),
            false,
        );
        assert_eq!(
            current,
            NativeWindowIdRefreshResult {
                value_available: true,
                stale: false,
                applied: Some(42),
            }
        );

        for stale in [
            evaluate_native_window_id_refresh(
                ticket,
                5,
                8,
                true,
                true,
                None,
                Some(&value),
                false,
            ),
            evaluate_native_window_id_refresh(
                ticket,
                4,
                9,
                true,
                true,
                None,
                Some(&value),
                false,
            ),
            evaluate_native_window_id_refresh(
                ticket,
                4,
                8,
                true,
                false,
                None,
                Some(&value),
                false,
            ),
            evaluate_native_window_id_refresh(
                ticket,
                4,
                8,
                true,
                true,
                Some(99),
                Some(&value),
                false,
            ),
        ] {
            assert!(stale.stale);
            assert_eq!(stale.applied, None);
        }

        let invalid = evaluate_native_window_id_refresh(
            ticket,
            4,
            8,
            true,
            true,
            None,
            Some(&MpvNode::Int(0)),
            false,
        );
        assert!(!invalid.value_available);
        assert!(!invalid.stale);
        assert_eq!(invalid.applied, None);
    }

    #[test]
    fn tracks_chapters_editions_and_video_parameters_are_owned() {
        let tracks = MpvNode::Array(vec![
            map(vec![
                ("id", MpvNode::Int(7)),
                ("src-id", MpvNode::Int(42)),
                ("type", MpvNode::String("audio".into())),
                ("lang", MpvNode::String("eng".into())),
                ("title", MpvNode::String("Main".into())),
                ("codec", MpvNode::String("aac".into())),
                ("demux-channel-count", MpvNode::Int(6)),
                ("demux-samplerate", MpvNode::Int(48_000)),
                ("selected", MpvNode::Bool(true)),
            ]),
            map(vec![
                ("id", MpvNode::Int(3)),
                ("type", MpvNode::String("sub".into())),
                ("lang", MpvNode::String("eng".into())),
                ("codec", MpvNode::String("hdmv_pgs_subtitle".into())),
                ("forced", MpvNode::Bool(true)),
                ("selected", MpvNode::Bool(true)),
            ]),
            map(vec![
                ("id", MpvNode::Int(1)),
                ("type", MpvNode::String("video".into())),
                ("codec", MpvNode::String("hevc".into())),
                ("selected", MpvNode::Bool(true)),
            ]),
        ]);
        let (catalog, audio_ids, subtitle_ids, codec) =
            parse_track_list(Some(&tracks));
        assert_eq!(catalog.audio[0].channels, Some(6));
        assert_eq!(catalog.audio[0].sample_rate, Some(48_000));
        assert_eq!(catalog.subtitles[0].kind, SubtitleKind::Bitmap);
        assert!(catalog.subtitles[0].is_forced);
        assert_eq!(audio_ids[&catalog.audio[0].id], 7);
        assert_eq!(subtitle_ids[&catalog.subtitles[0].id], 3);
        assert_eq!(codec.as_deref(), Some("hevc"));

        // Presentation order is chronological even if the native list is not;
        // command lookup still retains the original mpv indices.
        let chapters = MpvNode::Array(vec![
            map(vec![
                ("title", MpvNode::String("Two".into())),
                ("time", MpvNode::Double(10.0)),
            ]),
            map(vec![
                ("title", MpvNode::String("One".into())),
                ("time", MpvNode::Double(0.0)),
            ]),
        ]);
        let (chapters, chapter_ids) = parse_chapters(Some(&chapters));
        assert_eq!(chapters[0].title.as_deref(), Some("One"));
        assert_eq!(chapters[0].end, Some(Duration::from_secs(10)));
        assert_eq!(chapter_ids[&chapters[0].id], 1);
        assert_eq!(chapter_ids[&chapters[1].id], 0);

        let editions = MpvNode::Array(vec![map(vec![
            ("id", MpvNode::Int(2)),
            ("title", MpvNode::String("Director".into())),
            ("default", MpvNode::Bool(true)),
        ])]);
        let (editions, edition_ids) = parse_editions(Some(&editions));
        assert!(editions[0].is_default);
        assert_eq!(edition_ids[&editions[0].id], 2);

        let params = map(vec![
            ("w", MpvNode::Int(3840)),
            ("h", MpvNode::Int(2160)),
            ("pixelformat", MpvNode::String("yuv420p10le".into())),
            ("primaries", MpvNode::String("bt.2020".into())),
            ("gamma", MpvNode::String("pq".into())),
            ("max-cll", MpvNode::Double(1_000.0)),
        ]);
        let params = parse_video_parameters(Some(&params), None).unwrap();
        assert_eq!(params.width, Some(3840));
        assert_eq!(params.bit_depth, Some(10));
        assert!(params.hdr_metadata_observed);
    }

    #[test]
    fn observed_chapter_and_edition_selection_use_owned_identities() {
        let mut mapper = MpvEventMapper::default();
        let chapter_list = MpvNode::Array(vec![
            map(vec![
                ("title", MpvNode::String("Opening".into())),
                ("time", MpvNode::Double(0.0)),
            ]),
            map(vec![
                ("title", MpvNode::String("Feature".into())),
                ("time", MpvNode::Double(10.0)),
            ]),
        ]);
        let chapter_events =
            mapper.map_property(&property("chapter-list", Some(chapter_list)));
        let chapters = match &chapter_events[0] {
            PlaybackEvent::ChaptersChanged(chapters) => chapters,
            event => panic!("unexpected chapter event: {event:?}"),
        };
        let selected_chapter = chapters[1].id.clone();
        assert_eq!(
            mapper.map_property(&property("chapter", Some(MpvNode::Int(1)))),
            vec![PlaybackEvent::ChapterChanged(Some(
                selected_chapter.clone()
            ))]
        );

        let edition_list = MpvNode::Array(vec![map(vec![
            ("id", MpvNode::Int(9)),
            ("title", MpvNode::String("Extended".into())),
        ])]);
        let edition_events =
            mapper.map_property(&property("edition-list", Some(edition_list)));
        let editions = match &edition_events[0] {
            PlaybackEvent::EditionsChanged(editions) => editions,
            event => panic!("unexpected edition event: {event:?}"),
        };
        let selected_edition = editions[0].id.clone();
        assert_eq!(
            mapper.map_property(&property("edition", Some(MpvNode::Int(9)))),
            vec![PlaybackEvent::EditionChanged(Some(
                selected_edition.clone()
            ))]
        );

        assert_eq!(mapper.chapter_ids[&selected_chapter], 1);
        assert_eq!(mapper.edition_ids[&selected_edition], 9);

        // mpv exposes a one-edition Matroska catalog but reports the scalar
        // `edition` property as unavailable. Its default entry is still the
        // deterministic active selection.
        let mut single_edition = MpvEventMapper::default();
        let events = single_edition.map_property(&property(
            "edition-list",
            Some(MpvNode::Array(vec![map(vec![
                ("id", MpvNode::Int(0)),
                ("default", MpvNode::Bool(true)),
            ])])),
        ));
        let inferred = match &events[1] {
            PlaybackEvent::EditionChanged(Some(id)) => id.clone(),
            event => panic!("unexpected inferred edition event: {event:?}"),
        };
        assert_eq!(
            single_edition.map_property(&property("edition", None)),
            vec![PlaybackEvent::EditionChanged(Some(inferred))]
        );
    }

    #[test]
    fn track_identity_survives_native_id_reordering() {
        fn audio(native_id: i64) -> MpvNode {
            map(vec![
                ("id", MpvNode::Int(native_id)),
                ("src-id", MpvNode::Int(99)),
                ("type", MpvNode::String("audio".into())),
                ("lang", MpvNode::String("jpn".into())),
                ("title", MpvNode::String("Main".into())),
                ("codec", MpvNode::String("flac".into())),
            ])
        }
        let first = MpvNode::Array(vec![audio(1)]);
        let reloaded = MpvNode::Array(vec![audio(8)]);
        let (first, _, _, _) = parse_track_list(Some(&first));
        let (reloaded, ids, _, _) = parse_track_list(Some(&reloaded));
        assert_eq!(first.audio[0].id, reloaded.audio[0].id);
        assert_eq!(ids[&reloaded.audio[0].id], 8);
    }

    #[test]
    fn stop_during_load_seek_and_eof_have_deterministic_terminal_events() {
        let mut mapper = MpvEventMapper::default();
        mapper.map_event(&MpvEvent::StartFile {
            playlist_entry_id: 1,
        });
        assert_eq!(
            mapper.map_event(&MpvEvent::Seek),
            vec![PlaybackEvent::StateChanged(PlaybackState::Seeking)]
        );
        assert_eq!(
            mapper.map_event(&MpvEvent::EndFile(MpvEndFile {
                reason: MpvEndFileReason::Stop,
                error: None,
                playlist_entry_id: 1,
                playlist_insert_id: 0,
                playlist_insert_count: 0,
            })),
            vec![PlaybackEvent::Ended(EndReason::Stopped)]
        );

        mapper.reset_for_load();
        assert_eq!(
            mapper.map_event(&MpvEvent::EndFile(MpvEndFile {
                reason: MpvEndFileReason::Eof,
                error: None,
                playlist_entry_id: 2,
                playlist_insert_id: 0,
                playlist_insert_count: 0,
            })),
            vec![PlaybackEvent::Ended(EndReason::Eof)]
        );
    }

    #[test]
    fn native_window_quit_is_distinct_from_unexpected_core_shutdown() {
        let mut mapper = MpvEventMapper::default();
        mapper.map_event(&MpvEvent::StartFile {
            playlist_entry_id: 1,
        });
        mapper.map_event(&MpvEvent::FileLoaded);

        assert_eq!(
            mapper.map_event(&MpvEvent::EndFile(MpvEndFile {
                reason: MpvEndFileReason::Quit,
                error: None,
                playlist_entry_id: 1,
                playlist_insert_id: 0,
                playlist_insert_count: 0,
            })),
            vec![PlaybackEvent::Ended(EndReason::Closed)]
        );
        assert!(mapper.map_event(&MpvEvent::Shutdown).is_empty());

        let mut unexpected = MpvEventMapper::default();
        assert_eq!(
            unexpected.map_event(&MpvEvent::Shutdown),
            vec![PlaybackEvent::Ended(EndReason::BackendTerminated)]
        );
    }

    #[test]
    fn mpv_string_list_escapes_separator_and_backslash() {
        assert_eq!(
            encode_mpv_string_list(&[
                "X-One: a,b".to_string(),
                "X-Two: c\\d".to_string(),
            ]),
            "X-One: a\\,b,X-Two: c\\\\d"
        );
    }

    #[test]
    fn diagnostic_snapshot_captures_versions_vo_gpu_hwdec_and_timing() {
        let mut mapper = MpvEventMapper::default();
        mapper.map_property(&property(
            "mpv-version",
            Some(MpvNode::String("mpv v0.41.0".into())),
        ));
        mapper.map_property(&property(
            "ffmpeg-version",
            Some(MpvNode::String("8.1".into())),
        ));
        mapper.map_property(&property(
            "current-vo",
            Some(MpvNode::String("gpu-next".into())),
        ));
        mapper.map_property(&property(
            "current-gpu-context",
            Some(MpvNode::String("waylandvk".into())),
        ));
        mapper.map_property(&property(
            "vo-configured",
            Some(MpvNode::Bool(true)),
        ));
        mapper.map_property(&property(
            "video-params",
            Some(map(vec![
                ("w", MpvNode::Int(3840)),
                ("h", MpvNode::Int(2160)),
                ("gamma", MpvNode::String("pq".into())),
            ])),
        ));
        mapper.map_property(&property(
            "video-out-params",
            Some(map(vec![
                ("w", MpvNode::Int(1920)),
                ("h", MpvNode::Int(1080)),
                ("gamma", MpvNode::String("gamma2.2".into())),
            ])),
        ));
        mapper.map_property(&property(
            "hwdec-current",
            Some(MpvNode::String("vaapi".into())),
        ));
        mapper.map_property(&property(
            "hwdec-interop",
            Some(MpvNode::String("dmabuf-wayland".into())),
        ));
        mapper.map_property(&property(
            "decoder-frame-drop-count",
            Some(MpvNode::Int(2)),
        ));
        mapper
            .map_property(&property("frame-drop-count", Some(MpvNode::Int(3))));
        mapper.map_property(&property(
            "mistimed-frame-count",
            Some(MpvNode::Int(4)),
        ));
        mapper.map_property(&property(
            "vo-delayed-frame-count",
            Some(MpvNode::Int(5)),
        ));
        mapper.map_property(&property(
            "glsl-shaders",
            Some(MpvNode::Array(vec![
                MpvNode::String("/private/a.hook".into()),
                MpvNode::String("/private/b.hook".into()),
            ])),
        ));
        mapper.map_property(&property("avsync", Some(MpvNode::Double(-0.01))));
        mapper.observe_log(
            "cplayer",
            "List of enabled features: vulkan wayland libplacebo",
        );
        mapper.observe_log(
            "vo/gpu-next/vulkan",
            "Initialized libplacebo v7.360.1 (API v360)",
        );
        mapper.observe_log(
            "vo/gpu-next/vulkan",
            "Vulkan device: AMD Radeon RX 6800",
        );

        let snapshot = PlaybackSnapshot::new(
            SessionGeneration::new(9),
            PlaybackTarget::MPV_NATIVE_WINDOW,
            mpv_capabilities(MpvConfigPolicy::Deterministic),
        );
        let mut diagnostics = PlaybackDiagnosticSnapshot::from_snapshot(
            &snapshot,
            BackendRequest::Exact(PlaybackTarget::MPV_NATIVE_WINDOW),
        );
        diagnostics.mpv_configuration = Some(mpv_configuration_diagnostics(
            MpvConfigPolicy::Deterministic,
            MpvLoggingPolicy::default(),
            true,
            true,
        ));
        mapper.populate_diagnostics(&mut diagnostics);

        assert_eq!(diagnostics.versions.mpv.as_deref(), Some("mpv v0.41.0"));
        assert_eq!(diagnostics.versions.ffmpeg.as_deref(), Some("8.1"));
        assert_eq!(
            diagnostics.versions.libplacebo.as_deref(),
            Some("v7.360.1")
        );
        assert_eq!(
            diagnostics.versions.compiled_features,
            ["vulkan", "wayland", "libplacebo"]
        );
        assert_eq!(
            diagnostics.output.video_output.as_deref(),
            Some("gpu-next")
        );
        assert_eq!(diagnostics.output.gpu_api.as_deref(), Some("vulkan"));
        assert_eq!(
            diagnostics.output.gpu_context.as_deref(),
            Some("waylandvk")
        );
        assert_eq!(
            diagnostics.output.gpu_adapter.as_deref(),
            Some("AMD Radeon RX 6800")
        );
        assert_eq!(
            diagnostics.output.hardware_decoder.as_deref(),
            Some("vaapi")
        );
        assert_eq!(
            diagnostics.output.hardware_decoder_interop.as_deref(),
            Some("dmabuf-wayland")
        );
        assert_eq!(
            diagnostics
                .output
                .input_video
                .as_ref()
                .and_then(|video| video.width),
            Some(3840)
        );
        assert_eq!(
            diagnostics
                .output
                .output_video
                .as_ref()
                .and_then(|video| video.width),
            Some(1920)
        );
        assert_eq!(diagnostics.output.frames.decoder_dropped, Some(2));
        assert_eq!(diagnostics.output.frames.output_dropped, Some(3));
        assert_eq!(diagnostics.output.frames.mistimed, Some(4));
        assert_eq!(diagnostics.output.frames.delayed, Some(5));
        assert_eq!(diagnostics.output.frames.av_sync_seconds, Some(-0.01));
        assert_eq!(
            diagnostics
                .mpv_configuration
                .as_ref()
                .and_then(|configuration| {
                    configuration.active_video_shader_count
                }),
            Some(2)
        );
        let serialized = serde_json::to_string(&diagnostics).unwrap();
        assert!(!serialized.contains("/private/a.hook"));
        assert!(!serialized.contains("/private/b.hook"));
    }

    #[test]
    fn extension_commands_use_argument_boundaries_and_redact_invalid_inputs() {
        let subtitle = PlaybackFilePath::new("/tmp/external subtitle.srt");
        assert_eq!(
            build_external_subtitle_command(&subtitle, true).unwrap(),
            ["sub-add", "/tmp/external subtitle.srt", "select"]
        );
        assert_eq!(
            build_external_subtitle_command(&subtitle, false).unwrap(),
            ["sub-add", "/tmp/external subtitle.srt", "auto"]
        );

        let profile = VideoProfileName::new("cinema");
        assert_eq!(
            build_apply_profile_command(&profile).unwrap(),
            ["apply-profile", "cinema", "apply"]
        );

        let shaders = [
            PlaybackFilePath::new("/tmp/a shader.hook"),
            PlaybackFilePath::new("/tmp/b.shader.glsl"),
        ];
        assert_eq!(
            build_shader_commands(&shaders).unwrap(),
            vec![
                vec![
                    "change-list",
                    "glsl-shaders",
                    "set",
                    "/tmp/a shader.hook"
                ],
                vec![
                    "change-list",
                    "glsl-shaders",
                    "append",
                    "/tmp/b.shader.glsl"
                ],
            ]
        );
        assert_eq!(
            build_shader_commands(&[]).unwrap(),
            vec![vec!["change-list", "glsl-shaders", "clr", ""]]
        );
        for (mode, flag) in [
            (PlaybackScreenshotMode::VideoOnly, "video"),
            (PlaybackScreenshotMode::VideoWithSubtitles, "subtitles"),
            (PlaybackScreenshotMode::Window, "window"),
        ] {
            assert_eq!(
                build_screenshot_command(
                    &PlaybackFilePath::new("/tmp/frame with subtitles.png",),
                    mode,
                )
                .unwrap(),
                ["screenshot-to-file", "/tmp/frame with subtitles.png", flag,]
            );
        }

        let invalid = VideoProfileName::new("private-profile\nsecret");
        let error = build_apply_profile_command(&invalid).unwrap_err();
        assert_eq!(error.kind, PlaybackErrorKind::Command);
        let debug = format!("{error:?}");
        assert!(!debug.contains("private-profile"));
        assert!(!debug.contains("secret"));

        let invalid_path =
            PlaybackFilePath::new("/tmp/private-path\nsecret.png");
        let error = build_screenshot_command(
            &invalid_path,
            PlaybackScreenshotMode::VideoOnly,
        )
        .unwrap_err();
        assert_eq!(error.kind, PlaybackErrorKind::Command);
        let debug = format!("{error:?}");
        assert!(!debug.contains("private-path"));
        assert!(!debug.contains("secret.png"));
    }

    #[test]
    fn native_window_capabilities_report_policy_gated_extensions() {
        let deterministic = mpv_capabilities(MpvConfigPolicy::Deterministic);
        assert!(!deterministic.integrated_presentation);
        assert!(deterministic.native_window_fallback);
        assert!(!deterministic.native_hdr);
        assert!(deterministic.content_fit);
        assert!(deterministic.external_subtitle_loading);
        assert!(deterministic.chapter_selection);
        assert!(deterministic.edition_selection);
        assert!(deterministic.screenshot);
        assert!(deterministic.video_shader_passthrough);
        assert!(!deterministic.video_profile_passthrough);

        let trusted = mpv_capabilities(MpvConfigPolicy::TrustedUser);
        assert!(trusted.video_profile_passthrough);
    }

    #[test]
    fn mpv_config_policy_parser_is_fail_closed_and_does_not_need_global_env() {
        assert!(mpv_native_controls_enabled(
            PlaybackTarget::MPV_NATIVE_WINDOW
        ));
        assert!(!mpv_native_controls_enabled(PlaybackTarget::MPV_INTEGRATED));
        assert_eq!(
            parse_mpv_config_policy(None),
            Ok(MpvConfigPolicy::Deterministic)
        );
        assert_eq!(
            parse_mpv_config_policy(Some(OsStr::new("deterministic"))),
            Ok(MpvConfigPolicy::Deterministic)
        );
        assert_eq!(
            parse_mpv_config_policy(Some(OsStr::new("trusted-user"))),
            Ok(MpvConfigPolicy::TrustedUser)
        );
        assert_eq!(parse_mpv_config_policy(Some(OsStr::new("yes"))), Err(()));

        let deterministic = mpv_configuration_diagnostics(
            MpvConfigPolicy::Deterministic,
            MpvLoggingPolicy::default(),
            true,
            true,
        );
        assert_eq!(deterministic.policy, MpvConfigurationPolicy::Deterministic);
        assert!(!deterministic.user_config_enabled);
        assert!(!deterministic.user_scripts_enabled);
        assert!(deterministic.osc_enabled);
        assert!(deterministic.input_bindings_enabled);
        assert!(!deterministic.external_url_resolver_enabled);
        assert_eq!(deterministic.log_verbosity, MpvLogVerbosity::Info);
        assert!(deterministic.startup_verbose_capture);
        assert_eq!(deterministic.active_video_shader_count, None);

        let integrated = mpv_configuration_diagnostics(
            MpvConfigPolicy::Deterministic,
            MpvLoggingPolicy::default(),
            false,
            false,
        );
        assert!(!integrated.osc_enabled);
        assert!(!integrated.input_bindings_enabled);

        let trusted = mpv_configuration_diagnostics(
            MpvConfigPolicy::TrustedUser,
            MpvLoggingPolicy::fixed(MpvLogLevel::Trace),
            true,
            true,
        );
        assert_eq!(trusted.policy, MpvConfigurationPolicy::TrustedUser);
        assert!(trusted.user_config_enabled);
        assert!(trusted.user_scripts_enabled);
        let serialized = serde_json::to_value(trusted).unwrap();
        assert_eq!(serialized["policy"], "trusted_user");
        assert_eq!(serialized["external_url_resolver_enabled"], false);
        assert_eq!(serialized["log_verbosity"], "trace");
        assert_eq!(serialized["startup_verbose_capture"], false);
    }

    #[test]
    fn mpv_log_policy_is_explicit_fail_closed_and_secret_free() {
        assert_eq!(
            parse_mpv_logging_policy(None),
            Ok(MpvLoggingPolicy::default())
        );
        for (name, expected) in [
            ("none", MpvLogLevel::None),
            ("fatal", MpvLogLevel::Fatal),
            ("error", MpvLogLevel::Error),
            ("warn", MpvLogLevel::Warn),
            ("info", MpvLogLevel::Info),
            ("verbose", MpvLogLevel::Verbose),
            ("debug", MpvLogLevel::Debug),
            ("trace", MpvLogLevel::Trace),
        ] {
            let policy =
                parse_mpv_logging_policy(Some(OsStr::new(name))).unwrap();
            assert_eq!(policy, MpvLoggingPolicy::fixed(expected));
            assert!(!policy.startup_verbose_capture);
        }
        assert_eq!(
            parse_mpv_logging_policy(Some(OsStr::new("Bearer private"))),
            Err(())
        );
    }

    #[test]
    fn content_fit_maps_to_deterministic_native_vo_properties() {
        assert_eq!(
            content_fit_properties(PlaybackContentFit::Contain),
            [
                ("keepaspect", MpvNode::Bool(true)),
                ("video-unscaled", MpvNode::String("no".to_string())),
                ("panscan", MpvNode::Double(0.0)),
            ]
        );
        assert_eq!(
            content_fit_properties(PlaybackContentFit::Cover),
            [
                ("keepaspect", MpvNode::Bool(true)),
                ("video-unscaled", MpvNode::String("no".to_string())),
                ("panscan", MpvNode::Double(1.0)),
            ]
        );
        assert_eq!(
            content_fit_properties(PlaybackContentFit::Fill),
            [
                ("keepaspect", MpvNode::Bool(false)),
                ("video-unscaled", MpvNode::String("no".to_string())),
                ("panscan", MpvNode::Double(0.0)),
            ]
        );
        assert_eq!(
            content_fit_properties(PlaybackContentFit::None)[1],
            ("video-unscaled", MpvNode::String("yes".to_string()),)
        );
        assert_eq!(
            content_fit_properties(PlaybackContentFit::ScaleDown)[1],
            (
                "video-unscaled",
                MpvNode::String("downscale-big".to_string()),
            )
        );
    }

    #[test]
    #[ignore = "requires FERREX_MPV_SMOKE_MEDIA or FERREX_MPV_SMOKE_URL and a working desktop VO"]
    fn linked_native_window_load_control_fullscreen_stop_and_close_smoke() {
        let source = smoke_source_from_environment("Ferrex mpv smoke");
        let resume_position = Duration::from_millis(250);
        let mut adapter = MpvPlaybackAdapter::open(
            &source,
            resume_position,
            SessionGeneration::INITIAL,
        )
        .expect("native-window adapter starts");

        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            adapter.poll_events();
            match adapter.snapshot().state {
                PlaybackState::Playing | PlaybackState::Paused => break,
                PlaybackState::Failed => {
                    panic!(
                        "mpv load failed: {:?}",
                        adapter.snapshot().last_error
                    )
                }
                _ if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                state => panic!("mpv did not start before deadline: {state:?}"),
            }
        }

        let metadata_deadline =
            std::time::Instant::now() + Duration::from_secs(3);
        while (adapter.snapshot().duration.is_none()
            || adapter.snapshot().position < resume_position
            || adapter.snapshot().tracks.audio.is_empty()
            || adapter.snapshot().video.is_none()
            || adapter.mapper.mpv_version.is_none()
            || adapter.mapper.ffmpeg_version.is_none()
            || adapter.mapper.current_vo.is_none()
            || !adapter.mapper.vo_configured)
            && std::time::Instant::now() < metadata_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(adapter.snapshot().duration.is_some());
        assert!(adapter.snapshot().position >= resume_position);
        assert!(!adapter.snapshot().tracks.audio.is_empty());
        assert!(adapter.snapshot().video.is_some());
        assert!(adapter.mapper.mpv_version.is_some());
        assert!(adapter.mapper.ffmpeg_version.is_some());
        assert!(adapter.mapper.current_vo.is_some());
        assert!(adapter.mapper.vo_configured);
        let diagnostics = adapter.diagnostics(BackendRequest::Exact(
            PlaybackTarget::MPV_NATIVE_WINDOW,
        ));
        assert!(diagnostics.versions.client_api.is_some());
        assert!(diagnostics.versions.mpv.is_some());
        assert!(diagnostics.versions.ffmpeg.is_some());
        assert!(diagnostics.versions.libplacebo.is_some());
        let configuration = diagnostics
            .mpv_configuration
            .as_ref()
            .expect("mpv configuration policy is diagnostic");
        assert_eq!(
            configuration.policy,
            match adapter.config_policy {
                MpvConfigPolicy::Deterministic => {
                    MpvConfigurationPolicy::Deterministic
                }
                MpvConfigPolicy::TrustedUser => {
                    MpvConfigurationPolicy::TrustedUser
                }
            }
        );
        assert_eq!(
            configuration.user_config_enabled,
            adapter.config_policy.user_config_enabled()
        );
        assert!(!configuration.external_url_resolver_enabled);
        assert_eq!(diagnostics.output.vo_configured, Some(true));
        assert!(diagnostics.output.video_output.is_some());
        assert!(diagnostics.output.gpu_context.is_some());
        assert!(diagnostics.output.output_video.is_some());
        assert!(!adapter.startup_diagnostics_active);

        if let Some(path) =
            std::env::var_os("FERREX_MPV_SMOKE_EXTERNAL_SUBTITLE")
        {
            let path = std::fs::canonicalize(std::path::PathBuf::from(path))
                .expect("external subtitle fixture exists");
            let existing = adapter
                .snapshot()
                .tracks
                .subtitles
                .iter()
                .map(|track| track.id.clone())
                .collect::<Vec<_>>();
            adapter
                .apply_command(PlaybackCommand::AddExternalSubtitle {
                    source: PlaybackFilePath::new(path),
                    select: true,
                })
                .expect("external subtitle request is accepted");
            let subtitle_deadline =
                std::time::Instant::now() + Duration::from_secs(3);
            while !adapter.snapshot().tracks.subtitles.iter().any(|track| {
                track.is_external
                    && !existing.contains(&track.id)
                    && adapter.snapshot().tracks.selected_subtitle.as_ref()
                        == Some(&track.id)
            }) && std::time::Instant::now() < subtitle_deadline
            {
                adapter.poll_events();
                std::thread::sleep(Duration::from_millis(20));
            }
            let external = adapter
                .snapshot()
                .tracks
                .subtitles
                .iter()
                .find(|track| {
                    track.is_external && !existing.contains(&track.id)
                })
                .expect("mpv exposes the added external subtitle");
            assert_eq!(external.kind, SubtitleKind::Text);
            assert_eq!(
                adapter.snapshot().tracks.selected_subtitle.as_ref(),
                Some(&external.id)
            );
        }

        adapter
            .apply_command(PlaybackCommand::SetPaused(true))
            .expect("pause request is accepted");
        let pause_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while adapter.snapshot().state != PlaybackState::Paused
            && std::time::Instant::now() < pause_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(adapter.snapshot().state, PlaybackState::Paused);

        adapter
            .apply_command(PlaybackCommand::SetVolume(0.35))
            .expect("volume request is accepted");
        adapter
            .apply_command(PlaybackCommand::SetMuted(true))
            .expect("mute request is accepted");
        adapter
            .apply_command(PlaybackCommand::SetSpeed(1.25))
            .expect("speed request is accepted");
        adapter
            .apply_command(PlaybackCommand::SetContentFit(
                PlaybackContentFit::Cover,
            ))
            .expect("content-fit request is accepted");
        let controls_deadline =
            std::time::Instant::now() + Duration::from_secs(2);
        while ((adapter.snapshot().volume - 0.35).abs() > 0.001
            || !adapter.snapshot().muted
            || (adapter.snapshot().speed - 1.25).abs() > 0.001)
            && std::time::Instant::now() < controls_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!((adapter.snapshot().volume - 0.35).abs() <= 0.001);
        assert!(adapter.snapshot().muted);
        assert!((adapter.snapshot().speed - 1.25).abs() <= 0.001);
        assert_eq!(adapter.snapshot().content_fit, PlaybackContentFit::Cover);

        // Exercise explicit local extension inputs without relying on user
        // config. The observed shader count proves mpv accepted the runtime
        // list while diagnostics retain only the count, never the path.
        let artifact_stem = format!(
            "ferrex-mpv-smoke-{}-{}",
            std::process::id(),
            adapter.snapshot().generation.get()
        );
        let shader_path =
            std::env::temp_dir().join(format!("{artifact_stem}.hook"));
        let screenshot_path =
            std::env::temp_dir().join(format!("{artifact_stem}.png"));
        let _ = std::fs::remove_file(&shader_path);
        let _ = std::fs::remove_file(&screenshot_path);
        std::fs::write(
            &shader_path,
            "#!HOOK MAIN\n#!BIND HOOKED\nvec4 hook() { return HOOKED_tex(HOOKED_pos); }\n",
        )
        .expect("write temporary identity shader");
        adapter
            .apply_command(PlaybackCommand::SetVideoShaders(vec![
                PlaybackFilePath::new(shader_path.clone()),
            ]))
            .expect("shader passthrough request is accepted");
        let shader_deadline =
            std::time::Instant::now() + Duration::from_secs(3);
        while adapter.mapper.active_video_shader_count != Some(1)
            && std::time::Instant::now() < shader_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(adapter.mapper.active_video_shader_count, Some(1));

        adapter
            .apply_command(PlaybackCommand::CaptureScreenshot {
                output: PlaybackFilePath::new(screenshot_path.clone()),
                mode: PlaybackScreenshotMode::VideoWithSubtitles,
            })
            .expect("screenshot request is accepted");
        let screenshot_deadline =
            std::time::Instant::now() + Duration::from_secs(3);
        while std::fs::metadata(&screenshot_path)
            .map(|metadata| metadata.len() == 0)
            .unwrap_or(true)
            && std::time::Instant::now() < screenshot_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            std::fs::metadata(&screenshot_path)
                .is_ok_and(|metadata| metadata.len() > 0),
            "mpv did not write the requested screenshot"
        );

        adapter
            .apply_command(PlaybackCommand::SetVideoShaders(Vec::new()))
            .expect("shader clear request is accepted");
        let shader_clear_deadline =
            std::time::Instant::now() + Duration::from_secs(3);
        while adapter.mapper.active_video_shader_count != Some(0)
            && std::time::Instant::now() < shader_clear_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(adapter.mapper.active_video_shader_count, Some(0));
        std::fs::remove_file(shader_path).expect("remove temporary shader");
        std::fs::remove_file(screenshot_path)
            .expect("remove temporary screenshot");

        let audio_target = adapter
            .snapshot()
            .tracks
            .audio
            .iter()
            .find(|track| {
                Some(&track.id)
                    != adapter.snapshot().tracks.selected_audio.as_ref()
            })
            .or_else(|| adapter.snapshot().tracks.audio.first())
            .map(|track| track.id.clone())
            .expect("smoke fixture exposes an audio track");
        adapter
            .apply_command(PlaybackCommand::SelectAudio(audio_target.clone()))
            .expect("audio selection request is accepted");
        let audio_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while adapter.snapshot().tracks.selected_audio.as_ref()
            != Some(&audio_target)
            && std::time::Instant::now() < audio_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            adapter.snapshot().tracks.selected_audio.as_ref(),
            Some(&audio_target)
        );

        if let Some(subtitle_target) = adapter
            .snapshot()
            .tracks
            .subtitles
            .first()
            .map(|track| track.id.clone())
        {
            adapter
                .apply_command(PlaybackCommand::SelectSubtitle(Some(
                    subtitle_target.clone(),
                )))
                .expect("subtitle selection request is accepted");
            let subtitle_deadline =
                std::time::Instant::now() + Duration::from_secs(2);
            while adapter.snapshot().tracks.selected_subtitle.as_ref()
                != Some(&subtitle_target)
                && std::time::Instant::now() < subtitle_deadline
            {
                adapter.poll_events();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                adapter.snapshot().tracks.selected_subtitle.as_ref(),
                Some(&subtitle_target)
            );

            adapter
                .apply_command(PlaybackCommand::SelectSubtitle(None))
                .expect("subtitle disable request is accepted");
            let subtitle_off_deadline =
                std::time::Instant::now() + Duration::from_secs(2);
            while adapter.snapshot().tracks.selected_subtitle.is_some()
                && std::time::Instant::now() < subtitle_off_deadline
            {
                adapter.poll_events();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(adapter.snapshot().tracks.selected_subtitle.is_none());
        }

        if let Some(chapter_target) = adapter
            .snapshot()
            .chapters
            .get(1)
            .or_else(|| adapter.snapshot().chapters.first())
            .map(|chapter| chapter.id.clone())
        {
            adapter
                .apply_command(PlaybackCommand::SelectChapter(
                    chapter_target.clone(),
                ))
                .expect("chapter selection request is accepted");
            let chapter_deadline =
                std::time::Instant::now() + Duration::from_secs(2);
            while adapter.snapshot().current_chapter.as_ref()
                != Some(&chapter_target)
                && std::time::Instant::now() < chapter_deadline
            {
                adapter.poll_events();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                adapter.snapshot().current_chapter.as_ref(),
                Some(&chapter_target)
            );
        }

        if let Some(edition_target) = adapter
            .snapshot()
            .editions
            .first()
            .map(|edition| edition.id.clone())
        {
            adapter
                .apply_command(PlaybackCommand::SelectEdition(
                    edition_target.clone(),
                ))
                .expect("edition selection request is accepted");
            let edition_deadline =
                std::time::Instant::now() + Duration::from_secs(2);
            while adapter.snapshot().current_edition.as_ref()
                != Some(&edition_target)
                && std::time::Instant::now() < edition_deadline
            {
                adapter.poll_events();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                adapter.snapshot().current_edition.as_ref(),
                Some(&edition_target)
            );
        }

        for expected_fullscreen in [true, false] {
            adapter
                .apply_command(PlaybackCommand::SetFullscreen(
                    expected_fullscreen,
                ))
                .expect("fullscreen request is accepted");
            let fullscreen_deadline =
                std::time::Instant::now() + Duration::from_secs(3);
            while adapter.snapshot().fullscreen != expected_fullscreen
                && std::time::Instant::now() < fullscreen_deadline
            {
                adapter.poll_events();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                adapter.snapshot().fullscreen,
                expected_fullscreen,
                "mpv did not confirm the requested fullscreen state"
            );
        }

        adapter
            .apply_command(PlaybackCommand::SeekAbsolute(
                Duration::from_millis(500),
            ))
            .expect("absolute seek request is accepted");
        let absolute_seek_deadline =
            std::time::Instant::now() + Duration::from_secs(2);
        while (adapter.snapshot().state == PlaybackState::Seeking
            || adapter.snapshot().position < Duration::from_millis(400))
            && std::time::Instant::now() < absolute_seek_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        let absolute_position = adapter.snapshot().position;
        assert!(absolute_position >= Duration::from_millis(400));

        adapter
            .apply_command(PlaybackCommand::SeekRelative(
                DurationDelta::Forward(Duration::from_millis(250)),
            ))
            .expect("relative seek request is accepted");
        let relative_seek_deadline =
            std::time::Instant::now() + Duration::from_secs(2);
        while (adapter.snapshot().state == PlaybackState::Seeking
            || adapter.snapshot().position
                < absolute_position.saturating_add(Duration::from_millis(100)))
            && std::time::Instant::now() < relative_seek_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            adapter.snapshot().position
                >= absolute_position.saturating_add(Duration::from_millis(100))
        );

        adapter
            .apply_command(PlaybackCommand::SetPaused(false))
            .expect("play request is accepted");
        let play_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while adapter.snapshot().state != PlaybackState::Playing
            && std::time::Instant::now() < play_deadline
        {
            adapter.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(adapter.snapshot().state, PlaybackState::Playing);

        adapter
            .apply_command(PlaybackCommand::Stop)
            .expect("stop request is accepted");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            adapter.poll_events();
            if adapter.snapshot().end_reason == Some(EndReason::Stopped) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "mpv stop did not produce a terminal event: {:?}",
                adapter.snapshot()
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        // Exercise natural EOF independently from explicit stop, then load one
        // final generation to prove that a normal native-window close remains
        // distinguishable from EOF and cannot auto-advance an episode.
        adapter
            .apply_command(PlaybackCommand::Load(source.clone()))
            .expect("replacement load is accepted after stop");
        wait_for_smoke_playback(&mut adapter, "replacement load");
        let duration = adapter
            .snapshot()
            .duration
            .expect("smoke fixture has a finite duration");
        adapter
            .apply_command(PlaybackCommand::SeekAbsolute(
                duration.saturating_sub(Duration::from_millis(300)),
            ))
            .expect("near-EOF seek is accepted");
        adapter
            .apply_command(PlaybackCommand::SetPaused(false))
            .expect("near-EOF playback resumes");
        let eof_deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            adapter.poll_events();
            if adapter.snapshot().end_reason == Some(EndReason::Eof) {
                break;
            }
            assert!(
                std::time::Instant::now() < eof_deadline,
                "mpv did not produce natural EOF: {:?}",
                adapter.snapshot()
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        adapter
            .apply_command(PlaybackCommand::Load(source.clone()))
            .expect("post-EOF load is accepted");
        wait_for_smoke_playback(&mut adapter, "post-EOF load");

        adapter
            .worker()
            .expect("native owner remains available")
            .command_async(["quit"])
            .expect("native close-equivalent quit is accepted");
        let close_deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            adapter.poll_events();
            if adapter.snapshot().end_reason == Some(EndReason::Closed) {
                break;
            }
            assert!(
                std::time::Instant::now() < close_deadline,
                "mpv close did not produce an orderly terminal event: {:?}",
                adapter.snapshot()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    #[ignore = "requires FERREX_MPV_SMOKE_MEDIA or FERREX_MPV_SMOKE_URL and a working desktop VO"]
    fn linked_native_window_load_stop_lifecycle_stress() {
        let source =
            smoke_source_from_environment("Ferrex mpv lifecycle stress");
        let cycles = std::env::var("FERREX_MPV_STRESS_CYCLES")
            .map(|value| {
                value
                    .parse::<u64>()
                    .expect("FERREX_MPV_STRESS_CYCLES is an integer")
            })
            .unwrap_or(100);
        assert!(
            (1..=1_000).contains(&cycles),
            "FERREX_MPV_STRESS_CYCLES must be between 1 and 1000"
        );

        let started = std::time::Instant::now();
        let mut baseline_resources = None;
        let mut final_resources = None;
        let mut peak_resident_kib = 0;
        let mut peak_open_fds = 0;
        for cycle in 1..=cycles {
            let mut adapter = MpvPlaybackAdapter::open(
                &source,
                Duration::ZERO,
                SessionGeneration::new(cycle),
            )
            .unwrap_or_else(|error| {
                panic!("native-window cycle {cycle}/{cycles} failed to start: {error}")
            });
            wait_for_smoke_playback(
                &mut adapter,
                &format!("lifecycle cycle {cycle}/{cycles}"),
            );
            assert!(
                adapter.mapper.vo_configured,
                "cycle {cycle}/{cycles} did not configure a native VO"
            );

            adapter.apply_command(PlaybackCommand::Stop).unwrap_or_else(
                |error| panic!("cycle {cycle}/{cycles} rejected stop: {error}"),
            );
            let stop_deadline =
                std::time::Instant::now() + Duration::from_secs(5);
            loop {
                adapter.poll_events();
                if adapter.snapshot().end_reason == Some(EndReason::Stopped) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < stop_deadline,
                    "cycle {cycle}/{cycles} did not stop cleanly: {:?}",
                    adapter.snapshot()
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(adapter);

            let resources = process_resource_sample();
            if let Some(resources) = resources {
                baseline_resources.get_or_insert(resources);
                final_resources = Some(resources);
                peak_resident_kib =
                    peak_resident_kib.max(resources.resident_kib);
                peak_open_fds = peak_open_fds.max(resources.open_fds);
            }
            if cycle == cycles || cycle % 10 == 0 {
                if let Some(resources) = resources {
                    eprintln!(
                        "native-mpv lifecycle stress: {cycle}/{cycles} cycles complete; rss={} KiB, fds={}",
                        resources.resident_kib, resources.open_fds
                    );
                } else {
                    eprintln!(
                        "native-mpv lifecycle stress: {cycle}/{cycles} cycles complete"
                    );
                }
            }
        }
        eprintln!(
            "native-mpv lifecycle stress completed {cycles} fresh load/stop/window cycles in {:.2?}",
            started.elapsed()
        );

        if let (Some(baseline), Some(final_sample)) =
            (baseline_resources, final_resources)
        {
            let resident_growth_kib = final_sample
                .resident_kib
                .saturating_sub(baseline.resident_kib);
            let fd_growth =
                final_sample.open_fds.saturating_sub(baseline.open_fds);
            eprintln!(
                "native-mpv lifecycle resources: baseline_rss={} KiB, final_rss={} KiB, peak_rss={} KiB, rss_growth={} KiB, baseline_fds={}, final_fds={}, peak_fds={}, fd_growth={}",
                baseline.resident_kib,
                final_sample.resident_kib,
                peak_resident_kib,
                resident_growth_kib,
                baseline.open_fds,
                final_sample.open_fds,
                peak_open_fds,
                fd_growth
            );

            if let Some(limit_mib) =
                optional_stress_limit("FERREX_MPV_STRESS_MAX_RSS_GROWTH_MIB")
            {
                assert!(
                    resident_growth_kib <= limit_mib.saturating_mul(1_024),
                    "native-mpv lifecycle RSS grew by {resident_growth_kib} KiB, above the configured {limit_mib} MiB limit"
                );
            }
            if let Some(limit) =
                optional_stress_limit("FERREX_MPV_STRESS_MAX_FD_GROWTH")
            {
                let limit = usize::try_from(limit).unwrap_or(usize::MAX);
                assert!(
                    fd_growth <= limit,
                    "native-mpv lifecycle file descriptors grew by {fd_growth}, above the configured limit {limit}"
                );
            }
        }
    }

    #[test]
    fn duration_delta_retains_signed_seconds_for_mpv_seek() {
        assert_eq!(
            DurationDelta::Backward(Duration::from_millis(1_500))
                .as_seconds_f64(),
            -1.5
        );
        assert_eq!(PlaybackContentFit::Contain, PlaybackContentFit::Contain);
    }
}
