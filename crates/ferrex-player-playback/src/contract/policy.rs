use std::collections::HashSet;

use serde::Serialize;

use super::{
    FallbackReason, FallbackReasonCode, PlaybackError, PlaybackErrorKind,
    PlaybackTarget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendRequest {
    Auto,
    Exact(PlaybackTarget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaybackRequirements {
    /// Reject presentation paths that cannot preserve native HDR signaling.
    pub native_hdr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCandidate {
    pub target: PlaybackTarget,
    pub available: bool,
    pub native_hdr: bool,
    pub unavailable_reason: Option<FallbackReasonCode>,
}

impl BackendCandidate {
    pub const fn available(target: PlaybackTarget, native_hdr: bool) -> Self {
        Self {
            target,
            available: true,
            native_hdr,
            unavailable_reason: None,
        }
    }

    pub const fn unavailable(
        target: PlaybackTarget,
        reason: FallbackReasonCode,
    ) -> Self {
        Self {
            target,
            available: false,
            native_hdr: false,
            unavailable_reason: Some(reason),
        }
    }
}

/// Ordered rollout and fallback policy. Availability is supplied separately so
/// policy remains deterministic and straightforward to test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackPolicy {
    pub auto_order: Vec<PlaybackTarget>,
    pub fallback_order: Vec<PlaybackTarget>,
    pub allow_explicit_fallback: bool,
}

impl FallbackPolicy {
    /// Current migration policy: keep GStreamer ahead of every mpv mode.
    pub fn migration_default() -> Self {
        Self {
            auto_order: vec![
                PlaybackTarget::GSTREAMER_INTEGRATED,
                PlaybackTarget::GSTREAMER_EMBEDDED,
            ],
            fallback_order: vec![
                PlaybackTarget::MPV_NATIVE_WINDOW,
                PlaybackTarget::GSTREAMER_INTEGRATED,
                PlaybackTarget::GSTREAMER_EMBEDDED,
                PlaybackTarget::EXTERNAL_MPV,
            ],
            allow_explicit_fallback: true,
        }
    }

    /// Require the exact target requested by the caller.
    pub fn strict() -> Self {
        Self {
            auto_order: Vec::new(),
            fallback_order: Vec::new(),
            allow_explicit_fallback: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionDecision {
    pub selected: PlaybackTarget,
    pub fallback: Option<FallbackReason>,
}

/// Select a backend without probing global process state or mutating defaults.
///
/// For HDR-required content, candidates lacking native HDR signaling are
/// rejected rather than silently selecting an SDR frame-upload path.
pub fn select_backend(
    request: BackendRequest,
    requirements: PlaybackRequirements,
    policy: &FallbackPolicy,
    candidates: &[BackendCandidate],
) -> Result<SelectionDecision, PlaybackError> {
    let mut ordered = match request {
        BackendRequest::Auto => policy.auto_order.clone(),
        BackendRequest::Exact(target) => vec![target],
    };

    if matches!(request, BackendRequest::Auto) || policy.allow_explicit_fallback
    {
        ordered.extend(policy.fallback_order.iter().copied());
    }

    let mut seen = HashSet::new();
    ordered.retain(|target| seen.insert(*target));

    let requested_target = ordered.first().copied();
    let mut first_rejection = None;

    for target in ordered {
        let rejection = match candidates
            .iter()
            .find(|candidate| candidate.target == target)
        {
            None => Some((
                FallbackReasonCode::RequestedUnavailable,
                "target was not reported by runtime capability discovery",
            )),
            Some(candidate) if !candidate.available => Some((
                candidate
                    .unavailable_reason
                    .unwrap_or(FallbackReasonCode::RequestedUnavailable),
                "target is unavailable in this runtime",
            )),
            Some(candidate)
                if requirements.native_hdr && !candidate.native_hdr =>
            {
                Some((
                    FallbackReasonCode::MissingCapability,
                    "target cannot preserve required native HDR signaling",
                ))
            }
            Some(_) => None,
        };

        if let Some(rejection) = rejection {
            first_rejection.get_or_insert(rejection);
            continue;
        }

        let fallback = requested_target
            .filter(|requested| *requested != target)
            .map(|requested| {
                let (code, detail) = first_rejection.unwrap_or((
                    FallbackReasonCode::Policy,
                    "selected by fallback policy",
                ));
                FallbackReason {
                    code,
                    from: Some(requested),
                    to: target,
                    detail: detail.to_string(),
                }
            });

        return Ok(SelectionDecision {
            selected: target,
            fallback,
        });
    }

    let mut error = PlaybackError::new(
        PlaybackErrorKind::BackendUnavailable,
        if requirements.native_hdr {
            "no available playback target satisfies native HDR requirements"
        } else {
            "no playback target is available"
        },
    );
    error.recoverable = true;
    error.backend = requested_target.map(|target| target.backend);
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_auto_keeps_gstreamer_as_default() {
        let candidates = [
            BackendCandidate::available(
                PlaybackTarget::GSTREAMER_EMBEDDED,
                false,
            ),
            BackendCandidate::available(
                PlaybackTarget::MPV_NATIVE_WINDOW,
                true,
            ),
        ];

        let decision = select_backend(
            BackendRequest::Auto,
            PlaybackRequirements::default(),
            &FallbackPolicy::migration_default(),
            &candidates,
        )
        .unwrap();

        assert_eq!(decision.selected, PlaybackTarget::GSTREAMER_EMBEDDED);
        assert!(decision.fallback.is_some());
        assert_eq!(
            decision.fallback.unwrap().from,
            Some(PlaybackTarget::GSTREAMER_INTEGRATED)
        );
    }

    #[test]
    fn integrated_mpv_failure_falls_back_to_native_window() {
        let candidates = [
            BackendCandidate::unavailable(
                PlaybackTarget::MPV_INTEGRATED,
                FallbackReasonCode::PresenterFailed,
            ),
            BackendCandidate::available(
                PlaybackTarget::MPV_NATIVE_WINDOW,
                true,
            ),
        ];

        let decision = select_backend(
            BackendRequest::Exact(PlaybackTarget::MPV_INTEGRATED),
            PlaybackRequirements::default(),
            &FallbackPolicy::migration_default(),
            &candidates,
        )
        .unwrap();

        assert_eq!(decision.selected, PlaybackTarget::MPV_NATIVE_WINDOW);
        assert_eq!(
            decision.fallback.unwrap().code,
            FallbackReasonCode::PresenterFailed
        );
    }

    #[test]
    fn hdr_requirement_never_selects_sdr_frame_upload_fallback() {
        let candidates = [
            BackendCandidate::available(
                PlaybackTarget::GSTREAMER_EMBEDDED,
                false,
            ),
            BackendCandidate::available(
                PlaybackTarget::MPV_NATIVE_WINDOW,
                true,
            ),
        ];

        let decision = select_backend(
            BackendRequest::Exact(PlaybackTarget::GSTREAMER_EMBEDDED),
            PlaybackRequirements { native_hdr: true },
            &FallbackPolicy::migration_default(),
            &candidates,
        )
        .unwrap();

        assert_eq!(decision.selected, PlaybackTarget::MPV_NATIVE_WINDOW);
        assert_eq!(
            decision.fallback.unwrap().code,
            FallbackReasonCode::MissingCapability
        );
    }

    #[test]
    fn strict_request_reports_unavailable_instead_of_falling_back() {
        let candidates = [BackendCandidate::unavailable(
            PlaybackTarget::MPV_INTEGRATED,
            FallbackReasonCode::UnsupportedPlatform,
        )];

        let error = select_backend(
            BackendRequest::Exact(PlaybackTarget::MPV_INTEGRATED),
            PlaybackRequirements::default(),
            &FallbackPolicy::strict(),
            &candidates,
        )
        .unwrap_err();

        assert_eq!(error.kind, PlaybackErrorKind::BackendUnavailable);
        assert_eq!(error.backend, Some(super::super::BackendKind::Mpv));
    }
}
