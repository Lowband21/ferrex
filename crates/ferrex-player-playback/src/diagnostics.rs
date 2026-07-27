//! Serializable playback diagnostics and helpers that avoid leaking secrets.

use serde::Serialize;

use crate::contract::{
    BackendRequest, FallbackReason, PlaybackCapabilities, PlaybackError,
    PlaybackSnapshot, PlaybackState, PlaybackTarget, PresenterState,
    SurfaceGeometry, VideoParameters,
};

const ACCESS_TOKEN_PARAM: &str = "access_token=";
const REDACTED_TOKEN: &str = "<redacted>";

/// Version of the stable diagnostic JSON shape.
pub const PLAYBACK_DIAGNOSTIC_SCHEMA_VERSION: u16 = 6;

/// Backend-owner lifecycle, separate from media playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackBackendLifecycle {
    Initializing,
    Ready,
    Running,
    Stopping,
    Terminated,
    Failed,
}

/// libmpv client ABI compatibility details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MpvClientApiDiagnostics {
    pub bindings: String,
    pub runtime: String,
    pub minimum: String,
    pub compatible: bool,
}

/// mpv user-configuration trust policy selected for this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MpvConfigurationPolicy {
    /// Ferrex-owned options only; no user config or script discovery.
    Deterministic,
    /// Standard mpv user config and scripts are loaded as trusted code.
    TrustedUser,
}

/// Effective libmpv message verbosity for this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MpvLogVerbosity {
    None,
    Fatal,
    Error,
    Warn,
    Info,
    Verbose,
    Debug,
    Trace,
}

/// Effective high-level mpv configuration switches for issue reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MpvConfigurationDiagnostics {
    pub policy: MpvConfigurationPolicy,
    pub user_config_enabled: bool,
    pub user_scripts_enabled: bool,
    pub osc_enabled: bool,
    pub input_bindings_enabled: bool,
    pub external_url_resolver_enabled: bool,
    /// Steady-state native message filter. This reports only the selected
    /// level; log contents are never retained in the diagnostic snapshot.
    pub log_verbosity: MpvLogVerbosity,
    /// The default concise policy briefly raises native logging during file
    /// initialization so version/VO/GPU capability lines can be observed.
    pub startup_verbose_capture: bool,
    /// Number of explicit native-VO shaders currently observed by mpv. Paths
    /// are deliberately excluded from diagnostics.
    pub active_video_shader_count: Option<usize>,
}

/// Runtime versions and compiled mpv features observed in-process.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct PlaybackVersionDiagnostics {
    pub client_api: Option<MpvClientApiDiagnostics>,
    pub mpv: Option<String>,
    pub ffmpeg: Option<String>,
    pub libplacebo: Option<String>,
    pub compiled_features: Vec<String>,
}

/// Native-VO frame timing counters reported by mpv.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PlaybackFrameDiagnostics {
    pub decoder_dropped: Option<u64>,
    pub output_dropped: Option<u64>,
    pub mistimed: Option<u64>,
    pub delayed: Option<u64>,
    pub av_sync_seconds: Option<f64>,
}

/// Video-output details. `None` means the backend did not expose an observation.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PlaybackOutputDiagnostics {
    pub vo_configured: Option<bool>,
    pub video_output: Option<String>,
    pub gpu_api: Option<String>,
    pub gpu_context: Option<String>,
    pub gpu_adapter: Option<String>,
    pub hardware_decoder: Option<String>,
    pub hardware_decoder_interop: Option<String>,
    pub input_video: Option<VideoParameters>,
    pub output_video: Option<VideoParameters>,
    pub frames: PlaybackFrameDiagnostics,
}

/// Redacted, serializable snapshot used by diagnostics/settings and issue reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackDiagnosticSnapshot {
    pub schema_version: u16,
    pub generation: u64,
    pub requested_backend: BackendRequest,
    pub selected_target: PlaybackTarget,
    pub backend_lifecycle: PlaybackBackendLifecycle,
    pub playback_state: PlaybackState,
    pub presenter_state: PresenterState,
    pub presenter_geometry: Option<SurfaceGeometry>,
    pub capabilities: PlaybackCapabilities,
    pub position_millis: u64,
    pub duration_millis: Option<u64>,
    pub versions: PlaybackVersionDiagnostics,
    pub mpv_configuration: Option<MpvConfigurationDiagnostics>,
    pub output: PlaybackOutputDiagnostics,
    pub fallback_chain: Vec<FallbackReason>,
    pub last_fallback: Option<FallbackReason>,
    pub last_error: Option<PlaybackError>,
}

/// Evidence-oriented labels suitable for settings and issue-report UI.
///
/// Content signaling, native-output evidence, configured decoder policy, and
/// the observed decoder remain separate so the UI never turns a backend choice
/// into an unsupported HDR, hardware-decoding, or zero-copy claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackDiagnosticSummary {
    pub requested_backend: String,
    pub selected_backend: String,
    pub presentation_mode: String,
    pub integrated_presentation: String,
    pub hdr_content_evidence: String,
    pub native_hdr_evidence: String,
    pub hardware_decode_expectation: String,
    pub observed_hardware_decoder: String,
    pub fallback_reason: Option<String>,
}

impl PlaybackDiagnosticSnapshot {
    pub(crate) fn from_snapshot(
        snapshot: &PlaybackSnapshot,
        requested_backend: BackendRequest,
    ) -> Self {
        let backend_lifecycle = match snapshot.state {
            PlaybackState::Idle | PlaybackState::Ended => {
                PlaybackBackendLifecycle::Ready
            }
            PlaybackState::Stopping => PlaybackBackendLifecycle::Stopping,
            PlaybackState::Terminated => PlaybackBackendLifecycle::Terminated,
            PlaybackState::Failed => PlaybackBackendLifecycle::Failed,
            PlaybackState::Loading
            | PlaybackState::Playing
            | PlaybackState::Paused
            | PlaybackState::Buffering
            | PlaybackState::Seeking => PlaybackBackendLifecycle::Running,
        };
        let input_video = snapshot.video.clone();
        let hardware_decoder = input_video
            .as_ref()
            .and_then(|video| video.hardware_decoder.clone());

        Self {
            schema_version: PLAYBACK_DIAGNOSTIC_SCHEMA_VERSION,
            generation: snapshot.generation.get(),
            requested_backend,
            selected_target: snapshot.target,
            backend_lifecycle,
            playback_state: snapshot.state,
            presenter_state: snapshot.presenter,
            presenter_geometry: snapshot.presenter_geometry,
            capabilities: snapshot.capabilities.clone(),
            position_millis: duration_millis(snapshot.position),
            duration_millis: snapshot.duration.map(duration_millis),
            versions: PlaybackVersionDiagnostics::default(),
            mpv_configuration: None,
            output: PlaybackOutputDiagnostics {
                hardware_decoder,
                input_video,
                ..PlaybackOutputDiagnostics::default()
            },
            fallback_chain: snapshot.fallback_chain.clone(),
            last_fallback: snapshot.last_fallback.clone(),
            last_error: snapshot.last_error.clone(),
        }
    }

    /// Project the structured snapshot into concise, evidence-qualified labels
    /// for user-facing diagnostics. The projection contains no source URI,
    /// header, cookie, local path, or configuration path.
    pub fn summary(&self) -> PlaybackDiagnosticSummary {
        let fallback = self
            .last_fallback
            .as_ref()
            .or_else(|| self.fallback_chain.last());
        let integrated_fallback =
            self.fallback_chain.iter().rev().find(|reason| {
                reason.from.is_some_and(|target| {
                    target.presentation
                        == crate::contract::PresentationMode::IntegratedNative
                })
            });
        let integrated_presentation = if self.selected_target.presentation
            == crate::contract::PresentationMode::IntegratedNative
        {
            "Active for this session".to_string()
        } else if self.capabilities.integrated_presentation {
            "Available, but not selected".to_string()
        } else if let Some(reason) = integrated_fallback {
            format!("Unavailable: {}", reason.detail)
        } else {
            "Not active for this session".to_string()
        };

        let input_hdr_observed = self
            .output
            .input_video
            .as_ref()
            .is_some_and(|video| video.hdr_metadata_observed);
        let output_hdr_observed = self
            .output
            .output_video
            .as_ref()
            .is_some_and(|video| video.hdr_metadata_observed);
        let native_hdr_evidence = if output_hdr_observed {
            "Output metadata observed; display signaling not verified"
        } else if self.capabilities.native_hdr {
            "Backend reports support; output is not verified"
        } else {
            "Not verified for this session"
        };

        PlaybackDiagnosticSummary {
            requested_backend: backend_request_label(self.requested_backend),
            selected_backend: backend_label(self.selected_target.backend)
                .to_string(),
            presentation_mode: presentation_label(
                self.selected_target.presentation,
            )
            .to_string(),
            integrated_presentation,
            hdr_content_evidence: if input_hdr_observed {
                "Observed in input metadata"
            } else {
                "Not observed in input metadata"
            }
            .to_string(),
            native_hdr_evidence: native_hdr_evidence.to_string(),
            hardware_decode_expectation: hardware_decode_expectation(
                self.selected_target.backend,
            )
            .to_string(),
            observed_hardware_decoder: self
                .output
                .hardware_decoder
                .as_deref()
                .filter(|decoder| !decoder.trim().is_empty())
                .unwrap_or("Not observed")
                .to_string(),
            fallback_reason: fallback.map(|reason| reason.detail.clone()),
        }
    }
}

fn backend_request_label(request: BackendRequest) -> String {
    match request {
        BackendRequest::Auto => "Auto".to_string(),
        BackendRequest::Exact(target) => format!(
            "Exact: {} / {}",
            backend_label(target.backend),
            presentation_label(target.presentation)
        ),
    }
}

fn backend_label(backend: crate::contract::BackendKind) -> &'static str {
    match backend {
        crate::contract::BackendKind::GStreamer => "GStreamer",
        crate::contract::BackendKind::Mpv => "mpv (in process)",
        crate::contract::BackendKind::ExternalMpv => "mpv (external process)",
    }
}

fn presentation_label(
    presentation: crate::contract::PresentationMode,
) -> &'static str {
    match presentation {
        crate::contract::PresentationMode::IntegratedNative => {
            "Integrated native surface"
        }
        crate::contract::PresentationMode::EmbeddedFrames => {
            "Embedded frame upload"
        }
        crate::contract::PresentationMode::NativeWindow => "Native window",
        crate::contract::PresentationMode::ExternalWindow => "External window",
    }
}

fn hardware_decode_expectation(
    backend: crate::contract::BackendKind,
) -> &'static str {
    match backend {
        crate::contract::BackendKind::Mpv => "auto-safe requested",
        crate::contract::BackendKind::GStreamer => "Backend-managed policy",
        crate::contract::BackendKind::ExternalMpv => {
            "External player-owned policy"
        }
    }
}

fn duration_millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Redact access-token query values from playback URLs or log lines that contain them.
pub fn redact_playback_url(input: &str) -> String {
    redact_query_value(input, ACCESS_TOKEN_PARAM, REDACTED_TOKEN)
}

/// Returns true when the string contains an access-token query parameter.
pub(crate) fn contains_access_token(input: &str) -> bool {
    input.contains(ACCESS_TOKEN_PARAM)
}

fn redact_query_value(input: &str, key: &str, replacement: &str) -> String {
    let mut redacted = String::with_capacity(input.len());
    let mut remainder = input;

    while let Some(index) = remainder.find(key) {
        let (prefix, suffix) = remainder.split_at(index);
        redacted.push_str(prefix);
        redacted.push_str(key);
        redacted.push_str(replacement);

        let value_start = key.len();
        let value = &suffix[value_start..];
        let value_end = value
            .find(|ch: char| {
                matches!(
                    ch,
                    '&' | '#'
                        | ' '
                        | '\t'
                        | '\r'
                        | '\n'
                        | '"'
                        | '\''
                        | ')'
                        | ']'
                        | '}'
                )
            })
            .unwrap_or(value.len());
        remainder = &value[value_end..];
    }

    redacted.push_str(remainder);
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_access_token_in_playback_url() {
        let url = "https://ferrex.example/api/v1/stream/file?access_token=secret-token&quality=best";

        let redacted = redact_playback_url(url);

        assert_eq!(
            redacted,
            "https://ferrex.example/api/v1/stream/file?access_token=<redacted>&quality=best"
        );
        assert!(!redacted.contains("secret-token"));
    }

    #[test]
    fn redacts_access_token_inside_mpv_log_line() {
        let line = "Playing: https://ferrex.example/api/v1/stream/file?access_token=raw-secret)";

        let redacted = redact_playback_url(line);

        assert_eq!(
            redacted,
            "Playing: https://ferrex.example/api/v1/stream/file?access_token=<redacted>)"
        );
        assert!(!redacted.contains("raw-secret"));
    }

    #[test]
    fn leaves_urls_without_access_token_unchanged() {
        let url = "https://ferrex.example/api/v1/stream/file?ticket=public";

        assert_eq!(redact_playback_url(url), url);
        assert!(!contains_access_token(url));
    }

    #[test]
    fn diagnostic_summary_separates_selection_from_observed_evidence() {
        let mut snapshot = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::new(6),
            PlaybackTarget::MPV_NATIVE_WINDOW,
            PlaybackCapabilities::default(),
        );
        snapshot.video = Some(VideoParameters {
            hardware_decoder: Some("vaapi".to_string()),
            hdr_metadata_observed: true,
            ..VideoParameters::default()
        });
        snapshot.fallback_chain.push(FallbackReason {
            code: crate::contract::FallbackReasonCode::MissingCapability,
            from: Some(PlaybackTarget::MPV_INTEGRATED),
            to: PlaybackTarget::MPV_NATIVE_WINDOW,
            detail: "integrated presenter unavailable".to_string(),
        });
        snapshot.last_fallback = snapshot.fallback_chain.last().cloned();

        let mut diagnostics = PlaybackDiagnosticSnapshot::from_snapshot(
            &snapshot,
            BackendRequest::Exact(PlaybackTarget::MPV_INTEGRATED),
        );
        diagnostics.output.output_video = Some(VideoParameters {
            hdr_metadata_observed: true,
            ..VideoParameters::default()
        });

        let summary = diagnostics.summary();

        assert_eq!(summary.selected_backend, "mpv (in process)");
        assert_eq!(summary.presentation_mode, "Native window");
        assert_eq!(
            summary.requested_backend,
            "Exact: mpv (in process) / Integrated native surface"
        );
        assert_eq!(
            summary.integrated_presentation,
            "Unavailable: integrated presenter unavailable"
        );
        assert_eq!(summary.hdr_content_evidence, "Observed in input metadata");
        assert_eq!(
            summary.native_hdr_evidence,
            "Output metadata observed; display signaling not verified"
        );
        assert_eq!(summary.hardware_decode_expectation, "auto-safe requested");
        assert_eq!(summary.observed_hardware_decoder, "vaapi");
        assert_eq!(
            summary.fallback_reason.as_deref(),
            Some("integrated presenter unavailable")
        );

        let rendered = format!("{summary:?}").to_ascii_lowercase();
        assert!(!rendered.contains("zero-copy"));
        assert!(!rendered.contains("zero copy"));
    }

    #[test]
    fn diagnostic_snapshot_has_a_stable_serializable_shape() {
        let mut snapshot = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::new(7),
            PlaybackTarget::MPV_NATIVE_WINDOW,
            PlaybackCapabilities {
                seek: true,
                external_subtitle_loading: true,
                chapter_selection: true,
                edition_selection: true,
                screenshot: true,
                video_shader_passthrough: true,
                native_window_fallback: true,
                ..PlaybackCapabilities::default()
            },
        );
        snapshot.state = PlaybackState::Playing;
        snapshot.position = std::time::Duration::from_millis(1_250);
        snapshot.duration = Some(std::time::Duration::from_secs(90));
        snapshot.presenter = PresenterState::Attached;
        snapshot.presenter_geometry = Some(SurfaceGeometry::new(
            crate::contract::GeometryRevision::new(3),
            crate::contract::LogicalRect::new(0.0, 0.0, 1920.0, 1080.0),
            Some(crate::contract::LogicalRect::new(0.0, 0.0, 1920.0, 1040.0)),
            1.5,
        ));
        snapshot.fallback_chain.push(FallbackReason {
            code: crate::contract::FallbackReasonCode::MissingCapability,
            from: Some(PlaybackTarget::MPV_INTEGRATED),
            to: PlaybackTarget::MPV_NATIVE_WINDOW,
            detail: "integrated presenter unavailable".to_string(),
        });
        snapshot.last_fallback = snapshot.fallback_chain.last().cloned();

        let diagnostics = PlaybackDiagnosticSnapshot::from_snapshot(
            &snapshot,
            BackendRequest::Exact(PlaybackTarget::MPV_NATIVE_WINDOW),
        );
        let json = serde_json::to_value(diagnostics).unwrap();

        assert_eq!(json["schema_version"], 6);
        assert_eq!(json["generation"], 7);
        assert_eq!(json["playback_state"], "playing");
        assert_eq!(json["backend_lifecycle"], "running");
        assert_eq!(json["position_millis"], 1_250);
        assert_eq!(json["duration_millis"], 90_000);
        assert_eq!(json["selected_target"]["backend"], "mpv");
        assert_eq!(json["selected_target"]["presentation"], "native_window");
        assert_eq!(json["capabilities"]["external_subtitle_loading"], true);
        assert_eq!(json["capabilities"]["chapter_selection"], true);
        assert_eq!(json["capabilities"]["edition_selection"], true);
        assert_eq!(json["capabilities"]["screenshot"], true);
        assert_eq!(json["capabilities"]["video_shader_passthrough"], true);
        assert_eq!(json["capabilities"]["video_profile_passthrough"], false);
        assert_eq!(json["presenter_state"], "attached");
        assert_eq!(json["presenter_geometry"]["revision"], 3);
        assert_eq!(json["presenter_geometry"]["scale_factor"], 1.5);
        assert!(json["mpv_configuration"].is_null());
        assert_eq!(json["fallback_chain"].as_array().unwrap().len(), 1);
        assert_eq!(json["fallback_chain"][0]["code"], "missing_capability");
    }
}
