//! Video backend wiring and stream URL handling.
//!
//! This module coordinates the unified video backend, stream URL redaction,
//! loading flags, and playback state transitions around media file playback.

use crate::{
    PlayerMessage,
    contract::{
        BackendKind, BackendRequest, FallbackReason, FallbackReasonCode,
        PlaybackCommand, PlaybackContentFit, PlaybackError, PlaybackSource,
        SessionGeneration,
    },
    session::{PlaybackSession, PlaybackShutdownBarrier},
    state::PlayerDomainState,
    subwave_adapter::SubwavePlaybackAdapter,
    update::{PlaybackUiShell, PlaybackUpdatePort},
};
#[cfg(feature = "mpv")]
use crate::{contract::PlaybackErrorKind, mpv_adapter::MpvPlaybackAdapter};

use iced::Task;
use std::time::Duration;

// Helper functions
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub(crate) fn close_video(
    state: &mut PlayerDomainState,
) -> Option<PlaybackShutdownBarrier> {
    let mut shutdown_barrier = None;
    let retired_request = state
        .session_playback_request
        .or(state.integrated_playback_request);
    if let Some(mut video) = state.video_opt.take() {
        log::info!("Closing video");
        let _ = video.apply_command(PlaybackCommand::Stop);
        shutdown_barrier = match video.begin_shutdown_barrier() {
            Ok(barrier) => barrier,
            Err(error) => {
                log::error!(
                    "Playback shutdown could not establish its completion barrier: {error}"
                );
                Some(PlaybackShutdownBarrier::failed(error.to_string()))
            }
        };
        if shutdown_barrier.is_some() {
            state.root_shutdown_in_progress = true;
            state.root_shutdown_failed = false;
            state.root_shutdown_retired_request = retired_request;
            state.root_shutdown_exit_destination = None;
        }
        drop(video);
    }
    state.session_playback_request = None;
    state.session_media_id = None;
    state.last_valid_position = 0.0;
    state.last_valid_duration = 0.0;
    state.dragging = false;
    state.last_seek_position = None;
    state.seeking = false;
    shutdown_barrier
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn load_video<P>(
    state: &mut PlayerDomainState,
    ui: &mut dyn PlaybackUiShell,
) -> Task<P::AppMessage>
where
    P: PlaybackUpdatePort + 'static,
{
    if state.root_shutdown_blocks_launch() {
        if state.root_shutdown_failed {
            ui.set_video_error(
                "Playback is blocked because native window teardown could not be proven complete"
                    .to_string(),
            );
        }
        log::warn!(
            "Playback backend launch deferred behind native shutdown proof"
        );
        return Task::none();
    }

    // Check if video is already loaded or loading
    if state.video_opt.is_some() {
        log::warn!("Video already loaded, skipping duplicate load");
        return Task::none();
    }

    // Check if we're already in the process of loading
    if state.is_loading_video {
        log::warn!("Video is already being loaded, skipping duplicate load");
        return Task::none();
    }

    // Video loading is now handled directly, while transcoding cases are handled by streaming domain

    // Mark that we're loading
    state.is_loading_video = true;

    // Preserve any resume/duration hints before closing the current provider
    let pending_resume_hint = state.pending_resume_position;
    let duration_hint_before_close = state.last_valid_duration;

    // Close existing video if any (should not happen due to guard above)
    let _ = close_video(state);

    // Restore playback hints immediately so UI elements reflect intended progress
    if let Some(resume) = pending_resume_hint {
        state.last_valid_position = resume as f64;
        state.pending_resume_position = Some(resume);
    } else {
        state.last_valid_position = 0.0;
        state.pending_resume_position = None;
    }

    if duration_hint_before_close > 0.0 {
        state.last_valid_duration = duration_hint_before_close;
    }

    let source = match state.current_source.clone().or_else(|| {
        state
            .current_url
            .clone()
            .map(crate::contract::PlaybackSource::new)
    }) {
        Some(source) => source,
        None => {
            ui.set_video_error("No playback source provided".to_string());
            state.is_loading_video = false;
            return Task::none();
        }
    };
    let request = match state
        .resolved_playback_request
        .filter(|request| state.is_active_playback_request(*request))
    {
        Some(request) => request,
        None => {
            let Some(request) = state.begin_playback_request() else {
                ui.set_video_error(
                    "Playback request identity exhausted".to_string(),
                );
                state.is_loading_video = false;
                return Task::none();
            };
            let accepted = state.resolve_playback_request(request);
            debug_assert!(accepted);
            request
        }
    };
    let url = source.uri().clone();

    log::info!("=== VIDEO LOADING DEBUG ===");
    log::info!("Loading playback source: {source:?}");

    // Seed duration from server metadata. Backend/presenter selection must not
    // use a filename as HDR evidence; native output capability is confirmed by
    // the selected backend's observed snapshot/diagnostics.
    if let Some(current_media) = &state.current_media
        && let Some(metadata) = &current_media.media_file_metadata
        && let Some(duration) = metadata.duration
    {
        state.last_valid_duration = duration;
    }

    // Validate URL is valid UTF-8 before using
    let url_string = url.as_str();
    if !url_string.is_ascii() {
        let non_ascii_bytes =
            url_string.bytes().filter(|byte| !byte.is_ascii()).count();
        log::warn!(
            "Playback source URI contains {non_ascii_bytes} non-ASCII byte(s)"
        );
    }

    log::info!(
        "Creating backend-neutral playback provider (source: {source:?})"
    );

    ui.set_player_view();

    let res_pos: f64 = state.pending_resume_position.unwrap_or(0.0).into();

    let Some(generation) = state.playback_generation.next() else {
        log::error!("Playback session generation exhausted");
        ui.set_video_error("Playback session generation exhausted".to_string());
        state.is_loading_video = false;
        return Task::done(P::playback_message(PlayerMessage::VideoLoaded {
            request,
            success: false,
        }));
    };
    state.playback_generation = generation;

    let source = source.with_title(
        state
            .current_media
            .as_ref()
            .map(|media| media.filename.as_str())
            .unwrap_or("Ferrex playback"),
    );
    let start = Duration::try_from_secs_f64(res_pos).unwrap_or_default();

    // Create the selected adapter synchronously and update state immediately.
    // Auto deliberately remains Subwave during the staged rollout; only an
    // exact mpv request enters the in-process native-window path.
    match open_playback_session(
        &source,
        start,
        generation,
        state.backend_request,
    ) {
        Ok(mut video) => {
            if matches!(
                state.backend_request,
                BackendRequest::Exact(target) if target.backend == BackendKind::Mpv
            ) && video.snapshot().target.backend != BackendKind::Mpv
            {
                state.backend_request = BackendRequest::Auto;
            }

            let _ =
                video.apply_command(PlaybackCommand::SetVolume(state.volume));
            let _ =
                video.apply_command(PlaybackCommand::SetMuted(state.is_muted));
            let _ = video
                .apply_command(PlaybackCommand::SetSpeed(state.playback_speed));
            let _ = video.apply_command(PlaybackCommand::SetContentFit(
                playback_content_fit(state.content_fit),
            ));
            if state.is_fullscreen {
                let _ =
                    video.apply_command(PlaybackCommand::SetFullscreen(true));
            }
            video.synchronize_snapshot();

            if let Some(duration) = video.snapshot().duration {
                let duration = duration.as_secs_f64();
                if duration > 0.0 {
                    state.last_valid_duration = duration;
                }
            }

            state.terminal_generation_handled = None;
            state.video_opt = Some(video);
            state.session_playback_request = Some(request);
            state.session_media_id = state.current_media_id;
            state.is_loading_video = false;
            ui.set_player_view();

            Task::done(P::playback_message(PlayerMessage::VideoLoaded {
                request,
                success: true,
            }))
        }
        Err(e) => {
            log::error!("Failed to create video: {}", e);
            ui.set_video_error(format!("{}", e));
            state.is_loading_video = false;
            state.session_playback_request = None;
            state.session_media_id = None;
            Task::done(P::playback_message(PlayerMessage::VideoLoaded {
                request,
                success: false,
            }))
        }
    }
}

/// Open a backend-neutral playback session for an already resolved source.
///
/// Callers must keep authentication in [`PlaybackSource`] headers or cookies.
/// Exact backend requests still follow Ferrex's deterministic fallback policy,
/// which is reflected by the returned session snapshot and diagnostics.
pub fn open_playback_session(
    source: &PlaybackSource,
    start: Duration,
    generation: SessionGeneration,
    request: BackendRequest,
) -> Result<PlaybackSession, PlaybackError> {
    let requested_mpv_target = match request {
        BackendRequest::Exact(target) if target.backend == BackendKind::Mpv => {
            Some(target)
        }
        BackendRequest::Auto | BackendRequest::Exact(_) => None,
    };
    let mut fallback = None;

    #[cfg(feature = "mpv")]
    if let Some(requested_target) = requested_mpv_target {
        if let Some(detail) = packaged_mpv_platform_unavailability() {
            log::warn!(
                "playback_fallback code=unsupported_platform from={} to=gstreamer-auto detail={detail}",
                playback_target_label(requested_target),
            );
            fallback = Some((
                FallbackReasonCode::UnsupportedPlatform,
                detail.to_string(),
            ));
        } else {
            let presentation = PlaybackSession::preflight_mpv_presentation(
                requested_target,
                generation,
            );
            let effective_target = presentation.target();
            match MpvPlaybackAdapter::open_for_target(
                source,
                start,
                generation,
                effective_target,
            ) {
                Ok(adapter) => {
                    let report = adapter.compatibility_report();
                    log::info!(
                        "Using in-process mpv backend target={} (client API {}, bindings {})",
                        playback_target_label(effective_target),
                        report.runtime,
                        report.bindings
                    );
                    if effective_target != requested_target {
                        let detail = "integrated presenter preflight selected native-window compatibility mode";
                        log::warn!(
                            "playback_fallback code=missing_capability from={} to={} detail={detail}",
                            playback_target_label(requested_target),
                            playback_target_label(effective_target),
                        );
                    }
                    return Ok(PlaybackSession::from_mpv(
                        adapter,
                        request,
                        presentation,
                    ));
                }
                Err(error) => {
                    let code = mpv_initialization_fallback_code(&error);
                    log::warn!(
                        "playback_fallback code={} from={} to=gstreamer-auto detail={error}",
                        fallback_reason_code_label(code),
                        playback_target_label(requested_target),
                    );
                    fallback = Some((code, error.to_string()));
                }
            }
        }
    }

    #[cfg(not(feature = "mpv"))]
    if let Some(requested_target) = requested_mpv_target {
        let detail = "in-process mpv support is disabled in this build";
        log::warn!(
            "playback_fallback code=backend_disabled from={} to=gstreamer-auto detail={detail}",
            playback_target_label(requested_target),
        );
        fallback = Some((FallbackReasonCode::BackendDisabled, detail.into()));
    }

    let mut adapter = SubwavePlaybackAdapter::open(source, start, generation)?;
    if let Some((code, detail)) = fallback {
        adapter.record_fallback(FallbackReason {
            code,
            from: requested_mpv_target,
            to: adapter.snapshot().target,
            detail,
        });
    }
    Ok(PlaybackSession::from_subwave(adapter, request))
}

#[cfg(all(feature = "mpv", target_os = "linux"))]
fn packaged_mpv_platform_unavailability() -> Option<&'static str> {
    mpv_linux_platform_unavailability(
        option_env!("FERREX_MPV_X11"),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("WAYLAND_SOCKET").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    )
}

#[cfg(all(feature = "mpv", not(target_os = "linux")))]
fn packaged_mpv_platform_unavailability() -> Option<&'static str> {
    None
}

#[cfg(all(feature = "mpv", target_os = "linux"))]
fn mpv_linux_platform_unavailability(
    x11_profile: Option<&str>,
    has_wayland_display: bool,
    has_wayland_socket: bool,
    has_x11_display: bool,
) -> Option<&'static str> {
    if x11_profile == Some("disabled")
        && !has_wayland_display
        && !has_wayland_socket
        && has_x11_display
    {
        Some(
            "the packaged LGPL libmpv profile excludes mpv 0.41's GPL-only X11 video output",
        )
    } else {
        None
    }
}

#[cfg(feature = "mpv")]
fn fallback_reason_code_label(code: FallbackReasonCode) -> &'static str {
    match code {
        FallbackReasonCode::RequestedUnavailable => "requested_unavailable",
        FallbackReasonCode::MissingCapability => "missing_capability",
        FallbackReasonCode::BackendDisabled => "backend_disabled",
        FallbackReasonCode::RuntimeIncompatible => "runtime_incompatible",
        FallbackReasonCode::InitializationFailed => "initialization_failed",
        FallbackReasonCode::PresenterFailed => "presenter_failed",
        FallbackReasonCode::UnsupportedPlatform => "unsupported_platform",
        FallbackReasonCode::Policy => "policy",
    }
}

fn playback_target_label(
    target: crate::contract::PlaybackTarget,
) -> &'static str {
    match target {
        crate::contract::PlaybackTarget::GSTREAMER_INTEGRATED => {
            "gstreamer-integrated"
        }
        crate::contract::PlaybackTarget::GSTREAMER_EMBEDDED => {
            "gstreamer-embedded"
        }
        crate::contract::PlaybackTarget::MPV_INTEGRATED => "mpv-integrated",
        crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW => {
            "mpv-native-window"
        }
        crate::contract::PlaybackTarget::EXTERNAL_MPV => "external-mpv",
        _ => "custom-target",
    }
}

#[cfg(feature = "mpv")]
fn mpv_initialization_fallback_code(
    error: &PlaybackError,
) -> FallbackReasonCode {
    match error.kind {
        PlaybackErrorKind::BackendUnavailable
            if error.message.contains("incompatible libmpv") =>
        {
            FallbackReasonCode::RuntimeIncompatible
        }
        PlaybackErrorKind::BackendUnavailable => {
            FallbackReasonCode::UnsupportedPlatform
        }
        PlaybackErrorKind::BackendInitialization => {
            FallbackReasonCode::InitializationFailed
        }
        _ => FallbackReasonCode::InitializationFailed,
    }
}

pub(crate) fn playback_content_fit(
    fit: iced::ContentFit,
) -> PlaybackContentFit {
    match fit {
        iced::ContentFit::Contain => PlaybackContentFit::Contain,
        iced::ContentFit::Cover => PlaybackContentFit::Cover,
        iced::ContentFit::Fill => PlaybackContentFit::Fill,
        iced::ContentFit::None => PlaybackContentFit::None,
        iced::ContentFit::ScaleDown => PlaybackContentFit::ScaleDown,
    }
}

/// Treat server/decoder metadata as content evidence without using parsed
/// filenames or assuming that the selected output can signal HDR.
pub(crate) fn media_file_metadata_indicates_hdr(
    metadata: &ferrex_player_api::api_types::MediaFileMetadata,
) -> bool {
    let transfer_is_hdr = metadata.color_transfer.as_deref().is_some_and(|v| {
        let value = v.to_ascii_lowercase();
        value.contains("smpte2084")
            || value.contains("arib-std-b67")
            || value.contains("smpte2086")
            || value.contains("pq")
            || value.contains("hlg")
    });
    let primaries_are_wide = metadata
        .color_primaries
        .as_deref()
        .is_some_and(|v| v.to_ascii_lowercase().contains("bt2020"));

    metadata.bit_depth.is_some_and(|depth| depth > 8)
        || transfer_is_hdr
        || primaries_are_wide
}

#[cfg(all(test, feature = "mpv", target_os = "linux"))]
mod tests {
    use super::{
        media_file_metadata_indicates_hdr, mpv_linux_platform_unavailability,
    };
    use ferrex_player_api::api_types::MediaFileMetadata;

    fn metadata() -> MediaFileMetadata {
        MediaFileMetadata {
            duration: None,
            width: None,
            height: None,
            video_codec: None,
            audio_codec: None,
            bitrate: None,
            framerate: None,
            file_size: 0,
            color_primaries: None,
            color_transfer: None,
            color_space: None,
            bit_depth: None,
            parsed_info: None,
        }
    }

    #[test]
    fn hdr_content_evidence_uses_metadata_not_parsed_filename() {
        let mut sdr = metadata();
        sdr.bit_depth = Some(8);
        assert!(!media_file_metadata_indicates_hdr(&sdr));

        let mut pq = metadata();
        pq.bit_depth = Some(8);
        pq.color_transfer = Some("SMPTE2084".into());
        assert!(media_file_metadata_indicates_hdr(&pq));

        let mut hlg = metadata();
        hlg.color_transfer = Some("arib-std-b67".into());
        assert!(media_file_metadata_indicates_hdr(&hlg));

        let mut wide = metadata();
        wide.color_primaries = Some("BT2020".into());
        assert!(media_file_metadata_indicates_hdr(&wide));

        let mut ten_bit = metadata();
        ten_bit.bit_depth = Some(10);
        assert!(media_file_metadata_indicates_hdr(&ten_bit));
    }

    #[test]
    fn lgpl_package_rejects_x11_only_session_before_starting_mpv() {
        assert!(
            mpv_linux_platform_unavailability(
                Some("disabled"),
                false,
                false,
                true,
            )
            .is_some()
        );

        for supported in [
            (Some("enabled"), false, false, true),
            (None, false, false, true),
            (Some("disabled"), true, false, true),
            (Some("disabled"), false, true, true),
            (Some("disabled"), false, false, false),
        ] {
            assert_eq!(
                mpv_linux_platform_unavailability(
                    supported.0,
                    supported.1,
                    supported.2,
                    supported.3,
                ),
                None
            );
        }
    }
}
