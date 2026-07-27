use super::{
    PlaybackEvent, PlaybackEventEnvelope, PlaybackSnapshot, PlaybackState,
    PresenterEvent, PresenterState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reduction {
    Applied,
    IgnoredStaleGeneration,
    IgnoredDuplicateOrOutOfOrder,
}

/// Apply one ordered, generation-scoped backend event to the player snapshot.
///
/// Adapters may emit duplicate events, and late callbacks may survive session
/// replacement. This reducer rejects both without mutating the snapshot.
pub fn reduce_event(
    snapshot: &mut PlaybackSnapshot,
    envelope: PlaybackEventEnvelope,
) -> Reduction {
    if envelope.generation != snapshot.generation {
        return Reduction::IgnoredStaleGeneration;
    }

    if snapshot
        .last_sequence
        .is_some_and(|last| envelope.sequence <= last)
    {
        return Reduction::IgnoredDuplicateOrOutOfOrder;
    }

    snapshot.last_sequence = Some(envelope.sequence);

    match envelope.event {
        PlaybackEvent::StateChanged(state) => {
            snapshot.state = state;
            if !matches!(state, PlaybackState::Ended) {
                snapshot.end_reason = None;
            }
            if !matches!(state, PlaybackState::Failed) {
                snapshot.last_error = None;
            }
        }
        PlaybackEvent::PositionChanged(position) => {
            snapshot.position = position;
        }
        PlaybackEvent::DurationChanged(duration) => {
            snapshot.duration = duration;
        }
        PlaybackEvent::BufferChanged(buffer) => {
            snapshot.buffer = buffer;
        }
        PlaybackEvent::TracksChanged(mut tracks) => {
            tracks.normalize_selections();
            snapshot.tracks = tracks;
        }
        PlaybackEvent::ChaptersChanged(chapters) => {
            snapshot.chapters = chapters;
            if snapshot.current_chapter.as_ref().is_some_and(|selected| {
                !snapshot
                    .chapters
                    .iter()
                    .any(|chapter| &chapter.id == selected)
            }) {
                snapshot.current_chapter = None;
            }
        }
        PlaybackEvent::ChapterChanged(chapter) => {
            snapshot.current_chapter = chapter.filter(|selected| {
                snapshot
                    .chapters
                    .iter()
                    .any(|chapter| &chapter.id == selected)
            });
        }
        PlaybackEvent::EditionsChanged(editions) => {
            snapshot.editions = editions;
            if snapshot.current_edition.as_ref().is_some_and(|selected| {
                !snapshot
                    .editions
                    .iter()
                    .any(|edition| &edition.id == selected)
            }) {
                snapshot.current_edition = None;
            }
        }
        PlaybackEvent::EditionChanged(edition) => {
            snapshot.current_edition = edition.filter(|selected| {
                snapshot
                    .editions
                    .iter()
                    .any(|edition| &edition.id == selected)
            });
        }
        PlaybackEvent::VideoParametersChanged(video) => {
            snapshot.video = video;
        }
        PlaybackEvent::CapabilitiesChanged(capabilities) => {
            snapshot.capabilities = capabilities;
        }
        PlaybackEvent::VolumeChanged(volume) => {
            snapshot.volume = volume.clamp(0.0, 1.0);
        }
        PlaybackEvent::MutedChanged(muted) => {
            snapshot.muted = muted;
        }
        PlaybackEvent::SpeedChanged(speed) => {
            if speed.is_finite() && speed > 0.0 {
                snapshot.speed = speed;
            }
        }
        PlaybackEvent::ContentFitChanged(content_fit) => {
            snapshot.content_fit = content_fit;
        }
        PlaybackEvent::FullscreenChanged(fullscreen) => {
            snapshot.fullscreen = fullscreen;
        }
        PlaybackEvent::Ended(reason) => {
            snapshot.state = PlaybackState::Ended;
            snapshot.end_reason = Some(reason);
        }
        PlaybackEvent::Error(error) => {
            snapshot.state = PlaybackState::Failed;
            snapshot.last_error = Some(error);
        }
        PlaybackEvent::Presenter(event) => match event {
            PresenterEvent::StateChanged(state) => {
                snapshot.presenter = state;
            }
            PresenterEvent::GeometryChanged(geometry) => {
                snapshot.presenter_geometry = geometry;
            }
            PresenterEvent::FullscreenChanged(fullscreen) => {
                snapshot.fullscreen = fullscreen;
            }
            PresenterEvent::Failure(error) => {
                snapshot.presenter = PresenterState::Failed;
                snapshot.last_error = Some(error);
            }
            PresenterEvent::FallbackRequested(reason) => {
                record_fallback(snapshot, reason, false);
            }
        },
        PlaybackEvent::Fallback(reason) => {
            record_fallback(snapshot, reason, true);
        }
    }

    Reduction::Applied
}

fn record_fallback(
    snapshot: &mut PlaybackSnapshot,
    reason: super::FallbackReason,
    update_target: bool,
) {
    if update_target {
        snapshot.target = reason.to;
    }
    snapshot.last_fallback = Some(reason.clone());
    if snapshot.fallback_chain.last() != Some(&reason) {
        snapshot.fallback_chain.push(reason);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::contract::{
        AudioTrack, Chapter, ChapterId, Edition, EditionId, EventSequence,
        FallbackReason, FallbackReasonCode, GeometryRevision, LogicalRect,
        PlaybackCapabilities, PlaybackEvent, PlaybackEventEnvelope,
        PlaybackTarget, PresenterEvent, SessionGeneration, SubtitleKind,
        SubtitleTrack, SurfaceGeometry, TrackCatalog, TrackId, VideoParameters,
    };

    fn snapshot() -> PlaybackSnapshot {
        PlaybackSnapshot::new(
            SessionGeneration::new(7),
            PlaybackTarget::GSTREAMER_EMBEDDED,
            PlaybackCapabilities::default(),
        )
    }

    fn event(sequence: u64, event: PlaybackEvent) -> PlaybackEventEnvelope {
        PlaybackEventEnvelope {
            generation: SessionGeneration::new(7),
            sequence: EventSequence::new(sequence),
            event,
        }
    }

    #[test]
    fn duplicate_and_out_of_order_events_are_ignored() {
        let mut snapshot = snapshot();

        assert_eq!(
            reduce_event(
                &mut snapshot,
                event(
                    3,
                    PlaybackEvent::PositionChanged(Duration::from_secs(30))
                )
            ),
            Reduction::Applied
        );
        assert_eq!(
            reduce_event(
                &mut snapshot,
                event(
                    3,
                    PlaybackEvent::PositionChanged(Duration::from_secs(99))
                )
            ),
            Reduction::IgnoredDuplicateOrOutOfOrder
        );
        assert_eq!(
            reduce_event(
                &mut snapshot,
                event(
                    2,
                    PlaybackEvent::PositionChanged(Duration::from_secs(10))
                )
            ),
            Reduction::IgnoredDuplicateOrOutOfOrder
        );

        assert_eq!(snapshot.position, Duration::from_secs(30));
        assert_eq!(snapshot.last_sequence, Some(EventSequence::new(3)));
    }

    #[test]
    fn events_from_replaced_sessions_are_ignored() {
        let mut snapshot = snapshot();
        let stale = PlaybackEventEnvelope {
            generation: SessionGeneration::new(6),
            sequence: EventSequence::FIRST,
            event: PlaybackEvent::StateChanged(PlaybackState::Playing),
        };

        assert_eq!(
            reduce_event(&mut snapshot, stale),
            Reduction::IgnoredStaleGeneration
        );
        assert_eq!(snapshot.state, PlaybackState::Idle);
        assert_eq!(snapshot.last_sequence, None);
    }

    #[test]
    fn missing_optional_values_clear_stale_snapshot_data() {
        let mut snapshot = snapshot();
        snapshot.duration = Some(Duration::from_secs(100));
        snapshot.video = Some(VideoParameters {
            width: Some(1920),
            height: Some(1080),
            ..VideoParameters::default()
        });

        reduce_event(
            &mut snapshot,
            event(1, PlaybackEvent::DurationChanged(None)),
        );
        reduce_event(
            &mut snapshot,
            event(2, PlaybackEvent::VideoParametersChanged(None)),
        );

        assert_eq!(snapshot.duration, None);
        assert_eq!(snapshot.video, None);
    }

    #[test]
    fn track_selection_is_identity_based_and_normalized_on_reload() {
        let mut snapshot = snapshot();
        let audio_id = TrackId::new("audio:eng:main");
        let subtitle_id = TrackId::new("subtitle:eng:forced");
        let missing_id = TrackId::new("subtitle:stale-index");
        let catalog = TrackCatalog {
            audio: vec![AudioTrack {
                id: audio_id.clone(),
                title: Some("Main".into()),
                language: Some("eng".into()),
                codec: Some("aac".into()),
                channels: Some(2),
                sample_rate: Some(48_000),
                is_default: true,
                is_forced: false,
            }],
            subtitles: vec![SubtitleTrack {
                id: subtitle_id,
                title: Some("Forced".into()),
                language: Some("eng".into()),
                codec: Some("ass".into()),
                kind: SubtitleKind::Text,
                is_default: false,
                is_forced: true,
                is_external: false,
            }],
            selected_audio: Some(audio_id.clone()),
            selected_subtitle: Some(missing_id),
        };

        reduce_event(
            &mut snapshot,
            event(1, PlaybackEvent::TracksChanged(catalog)),
        );

        assert_eq!(snapshot.tracks.selected_audio, Some(audio_id));
        assert_eq!(snapshot.tracks.selected_subtitle, None);
    }

    #[test]
    fn chapter_and_edition_selection_is_normalized_against_owned_catalogs() {
        let mut snapshot = snapshot();
        let chapter_id = ChapterId::new("chapter:opening");
        let edition_id = EditionId::new("edition:extended");

        reduce_event(
            &mut snapshot,
            event(
                1,
                PlaybackEvent::ChaptersChanged(vec![Chapter {
                    id: chapter_id.clone(),
                    title: Some("Opening".to_string()),
                    start: Duration::ZERO,
                    end: None,
                }]),
            ),
        );
        reduce_event(
            &mut snapshot,
            event(
                2,
                PlaybackEvent::EditionsChanged(vec![Edition {
                    id: edition_id.clone(),
                    title: Some("Extended".to_string()),
                    is_default: true,
                }]),
            ),
        );
        reduce_event(
            &mut snapshot,
            event(3, PlaybackEvent::ChapterChanged(Some(chapter_id.clone()))),
        );
        reduce_event(
            &mut snapshot,
            event(4, PlaybackEvent::EditionChanged(Some(edition_id.clone()))),
        );

        assert_eq!(snapshot.current_chapter, Some(chapter_id));
        assert_eq!(snapshot.current_edition, Some(edition_id));

        reduce_event(
            &mut snapshot,
            event(5, PlaybackEvent::ChaptersChanged(Vec::new())),
        );
        reduce_event(
            &mut snapshot,
            event(6, PlaybackEvent::EditionsChanged(Vec::new())),
        );
        assert_eq!(snapshot.current_chapter, None);
        assert_eq!(snapshot.current_edition, None);
    }

    #[test]
    fn presenter_geometry_and_fallback_chain_remain_backend_neutral() {
        let mut snapshot = snapshot();
        snapshot.target = PlaybackTarget::MPV_INTEGRATED;
        let geometry = SurfaceGeometry::new(
            GeometryRevision::new(4),
            LogicalRect::new(10.0, 20.0, 1280.0, 720.0),
            Some(LogicalRect::new(10.0, 20.0, 1280.0, 700.0)),
            1.5,
        );
        let presenter_fallback = FallbackReason {
            code: FallbackReasonCode::PresenterFailed,
            from: Some(PlaybackTarget::MPV_INTEGRATED),
            to: PlaybackTarget::MPV_NATIVE_WINDOW,
            detail: "overlay attachment failed".to_string(),
        };
        let backend_fallback = FallbackReason {
            code: FallbackReasonCode::InitializationFailed,
            from: Some(PlaybackTarget::MPV_NATIVE_WINDOW),
            to: PlaybackTarget::GSTREAMER_EMBEDDED,
            detail: "native window failed to initialize".to_string(),
        };

        reduce_event(
            &mut snapshot,
            event(
                1,
                PlaybackEvent::Presenter(PresenterEvent::GeometryChanged(
                    Some(geometry),
                )),
            ),
        );
        reduce_event(
            &mut snapshot,
            event(
                2,
                PlaybackEvent::Presenter(PresenterEvent::FallbackRequested(
                    presenter_fallback.clone(),
                )),
            ),
        );
        assert_eq!(snapshot.target, PlaybackTarget::MPV_INTEGRATED);

        // Selection confirmation updates the target without duplicating the
        // already recorded presenter transition.
        reduce_event(
            &mut snapshot,
            event(3, PlaybackEvent::Fallback(presenter_fallback.clone())),
        );
        reduce_event(
            &mut snapshot,
            event(4, PlaybackEvent::Fallback(backend_fallback.clone())),
        );

        assert_eq!(snapshot.presenter_geometry, Some(geometry));
        assert_eq!(snapshot.target, PlaybackTarget::GSTREAMER_EMBEDDED);
        assert_eq!(
            snapshot.fallback_chain,
            vec![presenter_fallback, backend_fallback.clone()]
        );
        assert_eq!(snapshot.last_fallback, Some(backend_fallback));

        reduce_event(
            &mut snapshot,
            event(
                5,
                PlaybackEvent::Presenter(PresenterEvent::GeometryChanged(None)),
            ),
        );
        assert_eq!(snapshot.presenter_geometry, None);
    }
}
