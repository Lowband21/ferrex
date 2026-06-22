---
title: "Android playback QA matrix"
description: "LOW-353 Android and Android TV playback, watch progress, ticket expiry, track selection, and manual hardware QA matrix."
sidebar:
  order: 6
---

> **Evidence status:** This page preserves a dated QA packet. Treat rows marked blocked, pending hardware QA, or release-readiness follow-up as stale evidence until they are rerun on current devices and builds.

This matrix is the final Android/TV gate for the ticketed Media3 playback, watch progress, and track-selection stack. Automated fake coverage should pass before any hardware run; phone and TV manual results must be updated from real devices before promoting the Android lane beyond `dev`.

## Automated gate coverage

- `PlaybackFoundationTest` covers ticket fetch/stream URL redaction, first play preparation, server-resume vs start-over routing, progress writes for pause/exit/end, session expiry, ticket auth retry, server-unreachable retry, missing-file/offline-library recovery, and diagnostic redaction.
- `PlaybackTrackOptionsTest` covers no-track, multiple-audio, unsupported-audio, subtitle off/on, labels, capability warnings, and redacted track diagnostics.
- `TvPlaybackOverlayReducerTest` covers D-pad show controls, TV Back/exit sequencing, picker dismissal/selection, playback-stop recovery focus, and auto-hide behavior without Compose hardware dependencies.

## Manual phone/TV matrix

| Scenario | Automated/fake evidence | Phone manual result | TV manual result | Expected recovery/pass criteria |
| --- | --- | --- | --- | --- |
| First play | Ticket fetch and `Ready` state in `PlaybackFoundationTest` | Pending hardware QA | Pending hardware QA | Playback opens from detail/cache entry without exposing session tokens and starts rendering. |
| Resume | Server resume lookup in `PlaybackFoundationTest` | Pending hardware QA | Pending hardware QA | Resume starts at server watch position when no explicit start was chosen. |
| Start-over | Start-over skip-resume test in `PlaybackFoundationTest` | Pending hardware QA | Pending hardware QA | Playback starts at 0s and does not reuse stale watch position. |
| Pause | Periodic/progress write path in `PlaybackFoundationTest` | Pending hardware QA | Pending hardware QA | Pause keeps screen recoverable and commits current watch progress. |
| Back/exit | Exit progress write plus TV reducer Back tests | Pending hardware QA | Pending hardware QA | Back returns to detail/home without requiring app-data wipe and saves progress when duration is known. |
| Ended/completed | `onPlaybackEnded` progress test | Pending hardware QA | Pending hardware QA | End of media commits completion position and returns to a usable details/home state. |
| Watched/unwatched refresh | Progress commit callback exercised in `PlaybackFoundationTest` | Pending hardware QA | Pending hardware QA | Detail/home watch badges refresh after progress/completion changes. |
| Session expiry | Ticket/resume/progress 401/403 invalidation tests | Pending hardware QA | Pending hardware QA | Expired sessions leave playback and show sign-in/change-server recovery, not a stuck player. |
| Ticket expiry/retry | Stream auth retry-with-fresh-ticket test | Pending hardware QA | Pending hardware QA | Expired playback ticket retries with a fresh ticket, then invalidates session after retry limit. |
| Server unreachable | Ticket network retry-limit test | Pending hardware QA | Pending hardware QA | Shows retry/change-server/sign-out recovery after bounded retry attempts. |
| Missing file/offline library | 404/503 recovery test | Pending hardware QA | Pending hardware QA | Shows retry/change-server/sign-out recovery copy without clearing app data. |
| No tracks | Empty audio/text option tests | Pending hardware QA | Pending hardware QA | Playback stays usable; TV reports no audio/subtitle tracks and subtitles still offer Off. |
| Multiple audio | Multi-group audio option test | Pending hardware QA | Pending hardware QA | TV picker lists each audio stream with language/codec/channel details. |
| Unsupported audio | Capability-warning audio option test | Pending hardware QA | Pending hardware QA | Unsupported streams are disabled or warned; exceeds-capability streams remain selectable for device fallback. |
| Subtitles off/on | Subtitle Off/selected text tests | Pending hardware QA | Pending hardware QA | Off disables text tracks and selectable subtitle tracks can be re-enabled. |
| TV focus/Back behavior | TV overlay reducer tests | Not applicable | Pending hardware QA | Hidden controls restore safe focus before exit; pickers close before Back exits playback. |

## Known gaps and follow-up recommendations

- Physical/manual phone and Android TV playback were not executed in this workspace because no usable attached device or TV remote target was available. Recommended follow-up: run this matrix on a physical phone and Android TV against a seeded Ferrex server before promoting Android builds out of `dev`.
- TV currently exposes a minimal "Play first cached item" entry point rather than the full phone detail route flow. Recommended follow-up: add TV browse/detail playback launch coverage once the TV product surface expands.
- Unsupported-audio behavior is fake-covered through Media3 track support snapshots; hardware verification still needs media fixtures with unsupported and exceeds-capability codecs on representative devices.
