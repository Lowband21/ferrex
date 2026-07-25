//! Subwave/GStreamer adapter behind Ferrex-owned playback models.
//!
//! The adapter is the only module allowed to mention `SubwaveVideo` or
//! Subwave track DTOs. It keeps legacy presentation available while the player
//! update loop moves from direct method calls to the command/event contract.

use std::{collections::HashMap, time::Duration};

#[cfg(feature = "ui")]
use iced::Element;
use subwave_core::video::{
    types::{
        AudioTrack as SubwaveAudioTrack, SubtitleTrack as SubwaveSubtitleTrack,
    },
    video_trait::Video as SubwaveVideoTrait,
};
use subwave_unified::video::{BackendPreference, OpenOptions, SubwaveVideo};

use crate::contract::{
    AudioTrack, BackendKind, BackendRequest, DurationDelta, EndReason,
    EventSequence, FallbackReason, PlaybackCapabilities, PlaybackCommand,
    PlaybackError, PlaybackErrorKind, PlaybackEvent, PlaybackEventEnvelope,
    PlaybackSnapshot, PlaybackSource, PlaybackState, PlaybackTarget,
    SessionGeneration, SubtitleKind, SubtitleTrack, TrackCatalog, TrackId,
    reduce_event,
};
use crate::diagnostics::{PlaybackDiagnosticSnapshot, redact_playback_url};

/// Legacy Subwave provider adapted to Ferrex-owned commands and models.
pub struct SubwavePlaybackAdapter {
    video: SubwaveVideo,
    snapshot: PlaybackSnapshot,
    next_sequence: EventSequence,
    audio_indices: HashMap<TrackId, i32>,
    subtitle_indices: HashMap<TrackId, i32>,
}

impl std::fmt::Debug for SubwavePlaybackAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SubwavePlaybackAdapter")
            .field("snapshot", &self.snapshot)
            .finish_non_exhaustive()
    }
}

/// Narrow command surface between the Ferrex adapter and Subwave.
///
/// Keeping this seam independent of `SubwaveVideo` lets command translation be
/// verified without initializing GStreamer, a display server, or test media.
trait SubwaveCommandTarget {
    fn position(&self) -> Duration;
    fn set_paused(&mut self, paused: bool);
    fn seek(&mut self, position: Duration) -> Result<(), String>;
    fn set_volume(&mut self, volume: f64);
    fn set_muted(&mut self, muted: bool);
    fn set_speed(&mut self, speed: f64) -> Result<(), String>;
    fn select_audio_track(&mut self, index: i32) -> Result<(), String>;
    fn select_subtitle_track(
        &mut self,
        index: Option<i32>,
    ) -> Result<(), String>;
}

impl SubwaveCommandTarget for SubwaveVideo {
    fn position(&self) -> Duration {
        SubwaveVideo::position(self)
    }

    fn set_paused(&mut self, paused: bool) {
        SubwaveVideo::set_paused(self, paused);
    }

    fn seek(&mut self, position: Duration) -> Result<(), String> {
        SubwaveVideo::seek(self, position, false)
            .map_err(|error| error.to_string())
    }

    fn set_volume(&mut self, volume: f64) {
        SubwaveVideo::set_volume(self, volume);
    }

    fn set_muted(&mut self, muted: bool) {
        SubwaveVideo::set_muted(self, muted);
    }

    fn set_speed(&mut self, speed: f64) -> Result<(), String> {
        SubwaveVideo::set_speed(self, speed).map_err(|error| error.to_string())
    }

    fn select_audio_track(&mut self, index: i32) -> Result<(), String> {
        SubwaveVideo::select_audio_track(self, index)
            .map_err(|error| error.to_string())
    }

    fn select_subtitle_track(
        &mut self,
        index: Option<i32>,
    ) -> Result<(), String> {
        SubwaveVideo::select_subtitle_track(self, index)
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SubwaveAdapterCommand {
    SetPaused(bool),
    SeekAbsolute(Duration),
    SeekRelative(DurationDelta),
    SetVolume(f64),
    SetMuted(bool),
    SetSpeed(f64),
    SelectAudio(i32),
    SelectSubtitle(Option<i32>),
    Stop,
    Shutdown,
}

fn dispatch_subwave_command(
    target: &mut impl SubwaveCommandTarget,
    command: SubwaveAdapterCommand,
) -> Result<Vec<PlaybackEvent>, PlaybackError> {
    let events = match command {
        SubwaveAdapterCommand::SetPaused(paused) => {
            target.set_paused(paused);
            vec![PlaybackEvent::StateChanged(if paused {
                PlaybackState::Paused
            } else {
                PlaybackState::Playing
            })]
        }
        SubwaveAdapterCommand::SeekAbsolute(position) => {
            target.seek(position).map_err(|error| {
                backend_error(
                    PlaybackErrorKind::Command,
                    "Subwave absolute seek failed",
                    error,
                )
            })?;
            vec![PlaybackEvent::PositionChanged(position)]
        }
        SubwaveAdapterCommand::SeekRelative(delta) => {
            let current = target.position();
            let position = match delta {
                DurationDelta::Forward(amount) => {
                    current.saturating_add(amount)
                }
                DurationDelta::Backward(amount) => {
                    current.checked_sub(amount).unwrap_or(Duration::ZERO)
                }
            };
            target.seek(position).map_err(|error| {
                backend_error(
                    PlaybackErrorKind::Command,
                    "Subwave relative seek failed",
                    error,
                )
            })?;
            vec![PlaybackEvent::PositionChanged(position)]
        }
        SubwaveAdapterCommand::SetVolume(volume) => {
            if !volume.is_finite() {
                return Err(PlaybackError::new(
                    PlaybackErrorKind::Command,
                    "volume must be finite",
                ));
            }
            let volume = volume.clamp(0.0, 1.0);
            target.set_volume(volume);
            vec![PlaybackEvent::VolumeChanged(volume)]
        }
        SubwaveAdapterCommand::SetMuted(muted) => {
            target.set_muted(muted);
            vec![PlaybackEvent::MutedChanged(muted)]
        }
        SubwaveAdapterCommand::SetSpeed(speed) => {
            if !speed.is_finite() || speed <= 0.0 {
                return Err(PlaybackError::new(
                    PlaybackErrorKind::Command,
                    "playback speed must be finite and positive",
                ));
            }
            target.set_speed(speed).map_err(|error| {
                backend_error(
                    PlaybackErrorKind::Command,
                    "Subwave speed change failed",
                    error,
                )
            })?;
            vec![PlaybackEvent::SpeedChanged(speed)]
        }
        SubwaveAdapterCommand::SelectAudio(index) => {
            target.select_audio_track(index).map_err(|error| {
                backend_error(
                    PlaybackErrorKind::Command,
                    "Subwave audio-track selection failed",
                    error,
                )
            })?;
            Vec::new()
        }
        SubwaveAdapterCommand::SelectSubtitle(index) => {
            target.select_subtitle_track(index).map_err(|error| {
                backend_error(
                    PlaybackErrorKind::Command,
                    "Subwave subtitle-track selection failed",
                    error,
                )
            })?;
            Vec::new()
        }
        SubwaveAdapterCommand::Stop => {
            target.set_paused(true);
            vec![
                PlaybackEvent::StateChanged(PlaybackState::Stopping),
                PlaybackEvent::Ended(EndReason::Stopped),
            ]
        }
        SubwaveAdapterCommand::Shutdown => {
            target.set_paused(true);
            vec![PlaybackEvent::StateChanged(PlaybackState::Terminated)]
        }
    };

    Ok(events)
}

impl SubwavePlaybackAdapter {
    /// Open a Subwave provider without exposing source credentials in errors or
    /// diagnostics. HTTP headers and cookies are passed in-process.
    pub fn open(
        source: &PlaybackSource,
        start: Duration,
        generation: SessionGeneration,
    ) -> Result<Self, PlaybackError> {
        let mut headers: Vec<(String, String)> = source
            .headers()
            .iter()
            .map(|header| {
                (
                    header.name.clone(),
                    header.value.expose_secret().to_string(),
                )
            })
            .collect();

        if !source.cookies().is_empty() {
            let cookie = source
                .cookies()
                .iter()
                .map(|cookie| {
                    format!("{}={}", cookie.name, cookie.value.expose_secret())
                })
                .collect::<Vec<_>>()
                .join("; ");
            headers.push(("Cookie".to_string(), cookie));
        }

        let mut options = OpenOptions::new().start_seconds(start.as_secs_f64());
        if !headers.is_empty() {
            options = options.headers(&headers);
        }

        let video =
            SubwaveVideo::open(source.uri(), options).map_err(|error| {
                source_backend_error(
                    source,
                    "Subwave failed to initialize the media source",
                    error,
                )
            })?;
        let target = target_for_backend(video.backend());
        let capabilities = capabilities_for_target(target);
        let snapshot = PlaybackSnapshot::new(generation, target, capabilities);
        let mut adapter = Self {
            video,
            snapshot,
            next_sequence: EventSequence::FIRST,
            audio_indices: HashMap::new(),
            subtitle_indices: HashMap::new(),
        };
        adapter.synchronize_core_properties();
        adapter.refresh_tracks();
        Ok(adapter)
    }

    pub fn snapshot(&self) -> &PlaybackSnapshot {
        &self.snapshot
    }

    pub(crate) fn diagnostics(
        &self,
        requested_backend: BackendRequest,
    ) -> PlaybackDiagnosticSnapshot {
        PlaybackDiagnosticSnapshot::from_snapshot(
            &self.snapshot,
            requested_backend,
        )
    }

    pub(crate) fn record_fallback(&mut self, reason: FallbackReason) {
        self.record(PlaybackEvent::Fallback(reason));
    }

    /// Apply a Ferrex command and reduce resulting copied state into the
    /// adapter snapshot. Presentation-only fullscreen remains owned by Iced in
    /// this legacy backend, so the event records requested state only.
    pub fn apply_command(
        &mut self,
        command: PlaybackCommand,
    ) -> Result<(), PlaybackError> {
        let command = match command {
            PlaybackCommand::Load(_) => {
                return Err(PlaybackError::new(
                    PlaybackErrorKind::Command,
                    "load requires creating a new Subwave adapter generation",
                ));
            }
            PlaybackCommand::SelectAudio(track_id) => {
                return self.select_audio_track(&track_id);
            }
            PlaybackCommand::SelectSubtitle(track_id) => {
                return self.select_subtitle_track(track_id.as_ref());
            }
            PlaybackCommand::AddExternalSubtitle { .. } => {
                return Err(unsupported_extension("external subtitle loading"));
            }
            PlaybackCommand::SelectChapter(_) => {
                return Err(PlaybackError::new(
                    PlaybackErrorKind::Command,
                    "chapter selection is unavailable for the Subwave backend",
                ));
            }
            PlaybackCommand::SelectEdition(_) => {
                return Err(PlaybackError::new(
                    PlaybackErrorKind::Command,
                    "edition selection is unavailable for the Subwave backend",
                ));
            }
            PlaybackCommand::SetContentFit(content_fit) => {
                self.record(PlaybackEvent::ContentFitChanged(content_fit));
                return Ok(());
            }
            PlaybackCommand::SetFullscreen(fullscreen) => {
                self.record(PlaybackEvent::FullscreenChanged(fullscreen));
                return Ok(());
            }
            PlaybackCommand::ApplyVideoProfile(_) => {
                return Err(unsupported_extension("video profile passthrough"));
            }
            PlaybackCommand::SetVideoShaders(_) => {
                return Err(unsupported_extension("video shader passthrough"));
            }
            PlaybackCommand::CaptureScreenshot { .. } => {
                return Err(unsupported_extension("native video screenshots"));
            }
            PlaybackCommand::SetPaused(paused) => {
                SubwaveAdapterCommand::SetPaused(paused)
            }
            PlaybackCommand::SeekAbsolute(position) => {
                SubwaveAdapterCommand::SeekAbsolute(position)
            }
            PlaybackCommand::SeekRelative(delta) => {
                SubwaveAdapterCommand::SeekRelative(delta)
            }
            PlaybackCommand::SetVolume(volume) => {
                SubwaveAdapterCommand::SetVolume(volume)
            }
            PlaybackCommand::SetMuted(muted) => {
                SubwaveAdapterCommand::SetMuted(muted)
            }
            PlaybackCommand::SetSpeed(speed) => {
                SubwaveAdapterCommand::SetSpeed(speed)
            }
            PlaybackCommand::Stop => SubwaveAdapterCommand::Stop,
            PlaybackCommand::Shutdown => SubwaveAdapterCommand::Shutdown,
        };

        for event in dispatch_subwave_command(&mut self.video, command)? {
            self.record(event);
        }
        Ok(())
    }

    pub fn synchronize_core_properties(&mut self) {
        let eos = subwave_eos(&self.video);
        if eos
            && self.snapshot.state == PlaybackState::Ended
            && self.snapshot.end_reason == Some(EndReason::Eof)
        {
            return;
        }

        for event in subwave_core_events(
            self.video.position(),
            self.video.duration(),
            self.video.paused(),
            eos,
        ) {
            self.record(event);
        }
    }

    pub fn refresh_tracks(&mut self) -> TrackCatalog {
        let selected_audio_index = self.video.current_audio_track();
        let selected_subtitle_index = self.video.current_subtitle_track();
        let subtitles_enabled = self.video.subtitles_enabled();
        let raw_audio = self.video.audio_tracks();
        let raw_subtitles = self.video.subtitle_tracks();

        let (audio, audio_indices) = convert_audio_tracks(raw_audio);
        let (subtitles, subtitle_indices) =
            convert_subtitle_tracks(raw_subtitles);
        self.audio_indices = audio_indices;
        self.subtitle_indices = subtitle_indices;

        let selected_audio =
            self.audio_indices.iter().find_map(|(id, index)| {
                (*index == selected_audio_index).then(|| id.clone())
            });
        let selected_subtitle = subtitles_enabled
            .then(|| {
                self.subtitle_indices.iter().find_map(|(id, index)| {
                    (Some(*index) == selected_subtitle_index)
                        .then(|| id.clone())
                })
            })
            .flatten();

        let catalog = TrackCatalog {
            audio,
            subtitles,
            selected_audio,
            selected_subtitle,
        };
        self.record(PlaybackEvent::TracksChanged(catalog.clone()));
        catalog
    }

    pub fn select_audio_track(
        &mut self,
        track_id: &TrackId,
    ) -> Result<(), PlaybackError> {
        let index = self.lookup_audio_index(track_id)?;
        dispatch_subwave_command(
            &mut self.video,
            SubwaveAdapterCommand::SelectAudio(index),
        )?;
        let mut catalog = self.snapshot.tracks.clone();
        catalog.selected_audio = Some(track_id.clone());
        self.record(PlaybackEvent::TracksChanged(catalog));
        Ok(())
    }

    pub fn select_subtitle_track(
        &mut self,
        track_id: Option<&TrackId>,
    ) -> Result<(), PlaybackError> {
        let index = track_id
            .map(|track_id| self.lookup_subtitle_index(track_id))
            .transpose()?;
        dispatch_subwave_command(
            &mut self.video,
            SubwaveAdapterCommand::SelectSubtitle(index),
        )?;
        let mut catalog = self.snapshot.tracks.clone();
        catalog.selected_subtitle = track_id.cloned();
        self.record(PlaybackEvent::TracksChanged(catalog));
        Ok(())
    }

    pub fn set_subtitles_enabled(&mut self, enabled: bool) {
        self.video.set_subtitles_enabled(enabled);
        if !enabled {
            let mut catalog = self.snapshot.tracks.clone();
            catalog.selected_subtitle = None;
            self.record(PlaybackEvent::TracksChanged(catalog));
        }
    }

    pub fn subtitles_enabled(&self) -> bool {
        self.snapshot.tracks.selected_subtitle.is_some()
            || self.video.subtitles_enabled()
    }

    pub fn has_video(&self) -> bool {
        self.video.has_video()
    }

    pub fn is_appsink(&self) -> bool {
        matches!(self.video.backend(), BackendPreference::ForceAppsink)
    }

    pub fn uses_wayland_surface(&self) -> bool {
        matches!(self.video.backend(), BackendPreference::ForceWayland)
    }

    pub fn toggle_diagnostic_backend(&mut self) -> Result<(), PlaybackError> {
        let target = if self.is_appsink() {
            BackendPreference::ForceWayland
        } else {
            BackendPreference::ForceAppsink
        };
        self.set_diagnostic_backend(target)
    }

    pub fn force_appsink(&mut self) -> Result<(), PlaybackError> {
        self.set_diagnostic_backend(BackendPreference::ForceAppsink)
    }

    #[cfg(feature = "ui")]
    pub fn widget<'a, Message, Theme>(
        &'a self,
        content_fit: iced::ContentFit,
    ) -> Element<'a, Message, Theme, iced_wgpu::Renderer>
    where
        Message: Clone + 'a,
        Theme: 'a,
    {
        // Presentation widgets may redraw decoded frames as required, but UI
        // state synchronization is deliberately timer/event-driven instead of
        // publishing one application message per decoded frame.
        self.video.widget(content_fit, None)
    }

    fn set_diagnostic_backend(
        &mut self,
        target: BackendPreference,
    ) -> Result<(), PlaybackError> {
        self.video.set_preference(target).map_err(|error| {
            backend_error(
                PlaybackErrorKind::Command,
                "Subwave diagnostic backend switch failed",
                error,
            )
        })?;
        let target = target_for_backend(self.video.backend());
        self.snapshot.target = target;
        self.snapshot.capabilities = capabilities_for_target(target);
        self.synchronize_core_properties();
        self.refresh_tracks();
        Ok(())
    }

    fn lookup_audio_index(
        &mut self,
        track_id: &TrackId,
    ) -> Result<i32, PlaybackError> {
        if let Some(index) = self.audio_indices.get(track_id) {
            return Ok(*index);
        }
        self.refresh_tracks();
        self.audio_indices.get(track_id).copied().ok_or_else(|| {
            PlaybackError::new(
                PlaybackErrorKind::Command,
                format!("unknown audio track identity: {track_id}"),
            )
        })
    }

    fn lookup_subtitle_index(
        &mut self,
        track_id: &TrackId,
    ) -> Result<i32, PlaybackError> {
        if let Some(index) = self.subtitle_indices.get(track_id) {
            return Ok(*index);
        }
        self.refresh_tracks();
        self.subtitle_indices.get(track_id).copied().ok_or_else(|| {
            PlaybackError::new(
                PlaybackErrorKind::Command,
                format!("unknown subtitle track identity: {track_id}"),
            )
        })
    }

    fn record(&mut self, event: PlaybackEvent) {
        let sequence = self.next_sequence;
        let Some(next_sequence) = sequence.next() else {
            let mut error = PlaybackError::new(
                PlaybackErrorKind::Unknown,
                "Subwave event sequence exhausted",
            );
            error.backend = Some(BackendKind::GStreamer);
            self.snapshot.state = PlaybackState::Failed;
            self.snapshot.last_error = Some(error);
            return;
        };
        self.next_sequence = next_sequence;
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

fn subwave_eos(video: &SubwaveVideo) -> bool {
    match video {
        SubwaveVideo::Appsink { inner, .. } => {
            SubwaveVideoTrait::eos(inner.as_ref())
        }
        #[cfg(target_os = "linux")]
        SubwaveVideo::Wayland { handle, .. } => handle
            .try_borrow()
            .ok()
            .and_then(|video| video.as_deref().map(SubwaveVideoTrait::eos))
            .unwrap_or(false),
    }
}

fn subwave_core_events(
    position: Duration,
    duration: Duration,
    paused: bool,
    eos: bool,
) -> Vec<PlaybackEvent> {
    let mut events = vec![
        PlaybackEvent::PositionChanged(position),
        PlaybackEvent::DurationChanged(
            (duration > Duration::ZERO).then_some(duration),
        ),
    ];
    if eos {
        events.push(PlaybackEvent::Ended(EndReason::Eof));
    } else {
        events.push(PlaybackEvent::StateChanged(if paused {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        }));
    }
    events
}

fn capabilities_for_target(target: PlaybackTarget) -> PlaybackCapabilities {
    PlaybackCapabilities {
        seek: true,
        audio_track_selection: true,
        subtitle_track_selection: true,
        external_subtitle_loading: false,
        chapter_selection: false,
        edition_selection: false,
        speed: true,
        content_fit: true,
        fullscreen: true,
        screenshot: false,
        video_shader_passthrough: false,
        video_profile_passthrough: false,
        integrated_presentation: matches!(
            target.presentation,
            crate::contract::PresentationMode::IntegratedNative
        ),
        native_window_fallback: false,
        // These require observed compositor/output evidence; backend choice
        // alone is not sufficient to claim them.
        native_hdr: false,
        fractional_scaling: false,
    }
}

fn unsupported_extension(operation: &'static str) -> PlaybackError {
    let mut error = PlaybackError::new(
        PlaybackErrorKind::UnsupportedOperation,
        format!("{operation} is unavailable for the Subwave backend"),
    );
    error.backend = Some(BackendKind::GStreamer);
    error
}

fn target_for_backend(backend: BackendPreference) -> PlaybackTarget {
    match backend {
        BackendPreference::ForceWayland => PlaybackTarget::GSTREAMER_INTEGRATED,
        BackendPreference::Auto | BackendPreference::ForceAppsink => {
            PlaybackTarget::GSTREAMER_EMBEDDED
        }
    }
}

fn source_backend_error(
    source: &PlaybackSource,
    context: &str,
    error: impl std::fmt::Display,
) -> PlaybackError {
    let mut detail = error
        .to_string()
        .replace(source.uri().as_str(), "<redacted playback source>");
    for secret in source
        .headers()
        .iter()
        .map(|header| header.value.expose_secret())
        .chain(
            source
                .cookies()
                .iter()
                .map(|cookie| cookie.value.expose_secret()),
        )
        .filter(|secret| !secret.is_empty())
    {
        detail = detail.replace(secret, "<redacted>");
    }
    backend_error(PlaybackErrorKind::BackendInitialization, context, detail)
}

fn backend_error(
    kind: PlaybackErrorKind,
    context: &str,
    error: impl std::fmt::Display,
) -> PlaybackError {
    let detail = redact_playback_url(&error.to_string());
    let mut playback_error =
        PlaybackError::new(kind, format!("{context}: {detail}"));
    playback_error.backend = Some(BackendKind::GStreamer);
    playback_error
}

fn convert_audio_tracks(
    tracks: Vec<SubwaveAudioTrack>,
) -> (Vec<AudioTrack>, HashMap<TrackId, i32>) {
    let mut occurrences = HashMap::<String, usize>::new();
    let mut indices = HashMap::new();
    let tracks = tracks
        .into_iter()
        .map(|track| {
            let base = audio_identity_base(&track);
            let occurrence = occurrences.entry(base.clone()).or_default();
            let id = TrackId::new(format!("{base}#{occurrence}"));
            *occurrence += 1;
            indices.insert(id.clone(), track.index);
            AudioTrack {
                id,
                title: track.title,
                language: track.language,
                codec: track.codec,
                channels: track
                    .channels
                    .and_then(|channels| u16::try_from(channels).ok()),
                sample_rate: track
                    .sample_rate
                    .and_then(|rate| u32::try_from(rate).ok()),
                is_default: false,
                is_forced: false,
            }
        })
        .collect();
    (tracks, indices)
}

fn convert_subtitle_tracks(
    tracks: Vec<SubwaveSubtitleTrack>,
) -> (Vec<SubtitleTrack>, HashMap<TrackId, i32>) {
    let mut occurrences = HashMap::<String, usize>::new();
    let mut indices = HashMap::new();
    let tracks = tracks
        .into_iter()
        .map(|track| {
            let kind = if track.is_text_based() {
                SubtitleKind::Text
            } else {
                SubtitleKind::Bitmap
            };
            let base = subtitle_identity_base(&track);
            let occurrence = occurrences.entry(base.clone()).or_default();
            let id = TrackId::new(format!("{base}#{occurrence}"));
            *occurrence += 1;
            indices.insert(id.clone(), track.index);
            SubtitleTrack {
                id,
                title: track.title,
                language: track.language,
                codec: track.codec,
                kind,
                is_default: false,
                is_forced: false,
                is_external: false,
            }
        })
        .collect();
    (tracks, indices)
}

fn audio_identity_base(track: &SubwaveAudioTrack) -> String {
    format!(
        "subwave:audio:{}:{}:{}:{}:{}",
        identity_component(track.language.as_deref()),
        identity_component(track.title.as_deref()),
        identity_component(track.codec.as_deref()),
        track
            .channels
            .map_or_else(|| "_".to_string(), |value| value.to_string()),
        track
            .sample_rate
            .map_or_else(|| "_".to_string(), |value| value.to_string()),
    )
}

fn subtitle_identity_base(track: &SubwaveSubtitleTrack) -> String {
    format!(
        "subwave:subtitle:{}:{}:{}",
        identity_component(track.language.as_deref()),
        identity_component(track.title.as_deref()),
        identity_component(track.codec.as_deref()),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum FakeCall {
        SetPaused(bool),
        Seek(Duration),
        SetVolume(f64),
        SetMuted(bool),
        SetSpeed(f64),
        SelectAudio(i32),
        SelectSubtitle(Option<i32>),
    }

    #[derive(Debug, Default)]
    struct FakeCommandTarget {
        position: Duration,
        calls: Vec<FakeCall>,
        fail_seek: bool,
    }

    impl SubwaveCommandTarget for FakeCommandTarget {
        fn position(&self) -> Duration {
            self.position
        }

        fn set_paused(&mut self, paused: bool) {
            self.calls.push(FakeCall::SetPaused(paused));
        }

        fn seek(&mut self, position: Duration) -> Result<(), String> {
            self.calls.push(FakeCall::Seek(position));
            if self.fail_seek {
                Err("fake seek failure".to_string())
            } else {
                self.position = position;
                Ok(())
            }
        }

        fn set_volume(&mut self, volume: f64) {
            self.calls.push(FakeCall::SetVolume(volume));
        }

        fn set_muted(&mut self, muted: bool) {
            self.calls.push(FakeCall::SetMuted(muted));
        }

        fn set_speed(&mut self, speed: f64) -> Result<(), String> {
            self.calls.push(FakeCall::SetSpeed(speed));
            Ok(())
        }

        fn select_audio_track(&mut self, index: i32) -> Result<(), String> {
            self.calls.push(FakeCall::SelectAudio(index));
            Ok(())
        }

        fn select_subtitle_track(
            &mut self,
            index: Option<i32>,
        ) -> Result<(), String> {
            self.calls.push(FakeCall::SelectSubtitle(index));
            Ok(())
        }
    }

    #[test]
    fn ferrex_commands_dispatch_to_subwave_and_return_snapshot_events() {
        let mut target = FakeCommandTarget::default();

        assert_eq!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SetPaused(false),
            )
            .unwrap(),
            vec![PlaybackEvent::StateChanged(PlaybackState::Playing)]
        );
        assert_eq!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SeekAbsolute(Duration::from_secs(42)),
            )
            .unwrap(),
            vec![PlaybackEvent::PositionChanged(Duration::from_secs(42))]
        );
        assert_eq!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SeekRelative(DurationDelta::Forward(
                    Duration::from_secs(8),
                )),
            )
            .unwrap(),
            vec![PlaybackEvent::PositionChanged(Duration::from_secs(50))]
        );
        assert_eq!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SeekRelative(DurationDelta::Backward(
                    Duration::from_secs(100),
                )),
            )
            .unwrap(),
            vec![PlaybackEvent::PositionChanged(Duration::ZERO)]
        );
        assert_eq!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SetVolume(1.5),
            )
            .unwrap(),
            vec![PlaybackEvent::VolumeChanged(1.0)]
        );
        assert_eq!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SetMuted(true),
            )
            .unwrap(),
            vec![PlaybackEvent::MutedChanged(true)]
        );
        assert_eq!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SetSpeed(1.25),
            )
            .unwrap(),
            vec![PlaybackEvent::SpeedChanged(1.25)]
        );
        assert!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SelectAudio(4),
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::SelectSubtitle(None),
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            dispatch_subwave_command(&mut target, SubwaveAdapterCommand::Stop,)
                .unwrap(),
            vec![
                PlaybackEvent::StateChanged(PlaybackState::Stopping),
                PlaybackEvent::Ended(EndReason::Stopped),
            ]
        );
        assert_eq!(
            dispatch_subwave_command(
                &mut target,
                SubwaveAdapterCommand::Shutdown,
            )
            .unwrap(),
            vec![PlaybackEvent::StateChanged(PlaybackState::Terminated)]
        );

        assert_eq!(
            target.calls,
            vec![
                FakeCall::SetPaused(false),
                FakeCall::Seek(Duration::from_secs(42)),
                FakeCall::Seek(Duration::from_secs(50)),
                FakeCall::Seek(Duration::ZERO),
                FakeCall::SetVolume(1.0),
                FakeCall::SetMuted(true),
                FakeCall::SetSpeed(1.25),
                FakeCall::SelectAudio(4),
                FakeCall::SelectSubtitle(None),
                FakeCall::SetPaused(true),
                FakeCall::SetPaused(true),
            ]
        );
    }

    #[test]
    fn core_observation_emits_eof_instead_of_overwriting_it_with_pause() {
        assert_eq!(
            subwave_core_events(
                Duration::from_millis(9_900),
                Duration::from_secs(10),
                true,
                true,
            ),
            vec![
                PlaybackEvent::PositionChanged(Duration::from_millis(9_900)),
                PlaybackEvent::DurationChanged(Some(Duration::from_secs(10))),
                PlaybackEvent::Ended(EndReason::Eof),
            ]
        );
        assert_eq!(
            subwave_core_events(
                Duration::from_secs(4),
                Duration::from_secs(10),
                true,
                false,
            )
            .last(),
            Some(&PlaybackEvent::StateChanged(PlaybackState::Paused))
        );
    }

    #[test]
    fn mpv_native_extensions_fail_as_explicit_subwave_capabilities() {
        let error = unsupported_extension("native video screenshots");

        assert_eq!(error.kind, PlaybackErrorKind::UnsupportedOperation);
        assert_eq!(error.backend, Some(BackendKind::GStreamer));
        assert!(error.message.contains("unavailable"));
        let capabilities =
            capabilities_for_target(PlaybackTarget::GSTREAMER_EMBEDDED);
        assert!(!capabilities.external_subtitle_loading);
        assert!(!capabilities.screenshot);
        assert!(!capabilities.video_shader_passthrough);
        assert!(!capabilities.video_profile_passthrough);
    }

    #[test]
    fn invalid_values_and_backend_failures_are_structured() {
        let mut target = FakeCommandTarget::default();

        let invalid_volume = dispatch_subwave_command(
            &mut target,
            SubwaveAdapterCommand::SetVolume(f64::NAN),
        )
        .unwrap_err();
        let invalid_speed = dispatch_subwave_command(
            &mut target,
            SubwaveAdapterCommand::SetSpeed(0.0),
        )
        .unwrap_err();
        assert_eq!(invalid_volume.kind, PlaybackErrorKind::Command);
        assert_eq!(invalid_speed.kind, PlaybackErrorKind::Command);
        assert!(target.calls.is_empty());

        target.fail_seek = true;
        let seek_error = dispatch_subwave_command(
            &mut target,
            SubwaveAdapterCommand::SeekAbsolute(Duration::from_secs(1)),
        )
        .unwrap_err();
        assert_eq!(seek_error.kind, PlaybackErrorKind::Command);
        assert_eq!(seek_error.backend, Some(BackendKind::GStreamer));
        assert!(seek_error.message.contains("fake seek failure"));
    }

    fn audio(index: i32, language: &str, title: &str) -> SubwaveAudioTrack {
        SubwaveAudioTrack {
            index,
            language: Some(language.to_string()),
            title: Some(title.to_string()),
            codec: Some("aac".to_string()),
            channels: Some(2),
            sample_rate: Some(48_000),
        }
    }

    #[test]
    fn audio_identity_survives_backend_index_reordering() {
        let (first, _) = convert_audio_tracks(vec![
            audio(0, "eng", "Main"),
            audio(1, "jpn", "Main"),
        ]);
        let (reloaded, _) = convert_audio_tracks(vec![
            audio(0, "jpn", "Main"),
            audio(1, "eng", "Main"),
        ]);

        let english = first
            .iter()
            .find(|track| track.language.as_deref() == Some("eng"))
            .unwrap();
        let reloaded_english = reloaded
            .iter()
            .find(|track| track.language.as_deref() == Some("eng"))
            .unwrap();
        assert_eq!(english.id, reloaded_english.id);
    }

    #[test]
    fn source_initialization_errors_redact_url_headers_and_cookies() {
        let source = PlaybackSource::new(
            "https://user:password@example.test/private?access_token=query-secret"
                .parse()
                .unwrap(),
        )
        .with_header("Authorization", "header-secret")
        .with_cookie("session", "cookie-secret");
        let raw_error = format!(
            "failed {} Authorization=header-secret Cookie=cookie-secret",
            source.uri()
        );

        let error = source_backend_error(&source, "open failed", raw_error);
        let debug = format!("{error:?}");

        for secret in [
            "password",
            "private",
            "query-secret",
            "header-secret",
            "cookie-secret",
        ] {
            assert!(!debug.contains(secret), "error leaked {secret}");
        }
    }

    #[test]
    fn duplicate_metadata_still_produces_distinct_track_ids() {
        let (tracks, indices) = convert_audio_tracks(vec![
            audio(4, "eng", "Commentary"),
            audio(9, "eng", "Commentary"),
        ]);

        assert_ne!(tracks[0].id, tracks[1].id);
        assert_eq!(indices[&tracks[0].id], 4);
        assert_eq!(indices[&tracks[1].id], 9);
    }
}
