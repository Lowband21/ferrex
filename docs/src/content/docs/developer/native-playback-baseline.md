---
title: "Native playback migration baseline"
description: "Code inventory and behavior baseline captured before the libmpv migration."
sidebar:
  order: 9
---

This inventory anchors P0 of the [native mpv integration specification](https://github.com/Lowband21/ferrex/blob/dev/docs/specs/native-mpv-playback.md) and its [migration plan](https://github.com/Lowband21/ferrex/blob/dev/docs/plans/native-mpv-playback-migration.md). It describes the code at `dev` commit `23c5715b0c73234bc4d2708d7f2068685730dde7` before the backend-neutral contract is wired into the existing player.

## Ownership boundary

The current desktop playback implementation is already extracted from the UI shell:

- `crates/ferrex-player-playback` owns playback state, update policy, Subwave loading, controls, the external-mpv process, and the desktop player view.
- `crates/ferrex-player-ui` adapts the playback ports, contributes the 10-foot player overlay, chooses transparent window theming, and starts playback-related subscriptions.
- `crates/ferrex-player-playback/src/contract/` is the selected initial boundary for Ferrex-owned backend-neutral commands, events, snapshots, track models, channels, and fallback policy. A further crate split is not needed until a second client consumes this contract.

## Concrete backend coupling inventory

### `SubwaveVideo`

| Location | Direct coupling |
|---|---|
| `ferrex-player-playback/src/state.rs` | Imports and stores `Option<SubwaveVideo>` as `video_opt`; derives playing/video presence by polling it; pauses and drops it during teardown. |
| `ferrex-player-playback/src/video.rs` | Constructs `SubwaveVideo::open_at_seconds`, queries initial duration, and pauses/drops the old instance. |
| `ferrex-player-playback/src/update.rs` | Polls position, duration, and paused state; directly issues pause, seek, volume, mute, speed, track, and diagnostic backend-switch operations. |
| `ferrex-player-playback/src/track_selection.rs` | Copies Subwave track DTOs into state and selects tracks by Subwave integer index. |
| `ferrex-player-playback/src/view.rs` | Accepts `&SubwaveVideo` and calls `video.widget(...)`; frame callbacks also drive snapshot-like polling. |
| `ferrex-player-playback/src/controls.rs` | Reads `video.backend()` to label the Wayland/AppSink diagnostic toggle. |
| `ferrex-player-ui/src/domains/ui/views/tenfoot/player_overlay.rs` | Calls `video.widget(...)` directly for the 10-foot player and branches on `video_opt`. |
| `ferrex-player-ui/src/domains/ui/theme.rs` | Polls the Subwave backend preference to decide whether the main Iced window is transparent. |
| UI streaming/media/player subscriptions | Treat `video_opt.is_some()` as the internal-session and playback-ready signal. |

The streaming domain also uses `video_opt` presence to decide whether HLS may start, whether transcoding status polling should continue, and whether a discovered source duration can be copied into player state. These are session-policy checks rather than presentation concerns and must move to backend-neutral state.

### External mpv process

`external_mpv_active` is read by the playback view, playback and media subscriptions, episode navigation, the 10-foot overlay, and keyboard gating. `external_mpv_handle` is owned by `PlayerDomainState` and is directly polled or cleared by `update.rs`. The two fields jointly represent backend selection, liveness, progress, fullscreen restoration, and process ownership; there is no single external-player snapshot.

Both playback and media subscription composers currently add an external-mpv poll when active. Root subscription composition must be checked for duplicate one-second polls during migration.

## Message-to-behavior map

| `PlayerMessage` | Current backend/state effect |
|---|---|
| `PlayMedia` | Synthesizes a random movie ID and delegates to `PlayMediaWithId`; watch tracking therefore has a placeholder identity. |
| `PlayMediaWithId` | Stores media/ID, consumes pending resume data, seeds duration/HDR heuristics, clears stale URLs, and asynchronously requests a playback ticket. |
| `SetStreamUrl` | Parses and stores the ticketed URL, closes an existing Subwave provider during episode replacement, and calls `load_video`. |
| `StreamUrlResolutionFailed` | Clears URL/loading flags and enters the video-error view; it does not persist terminal progress. |
| `VideoReadyToPlay` | Calls the internal Subwave loader. |
| `VideoLoaded(true)` | Copies Subwave audio/subtitle state and enters the player view. |
| `VideoLoaded(false)` | Enters the video-error view. |
| `Play` / `Pause` | Calls `set_paused`, then immediately sends progress using direct position/duration polling. Both paths unwrap `current_media_id` when a video exists. |
| `PlayPause` | Polls `paused`, toggles it, sends progress, and reveals controls. It also unwraps `current_media_id`. |
| `Stop` | Sends final progress from `last_valid_position`/`last_valid_duration`, then queues reset and back navigation. It does not query the backend at stop time. |
| `ResetAfterStop` | Clears media, URL, Subwave handle, progress cache, tracks, and transient playback state. External-mpv fields are not reset by `PlayerDomainState::reset`; callers clear them separately. |
| `NavigateBack` / `NavigateHome` | Polls internal position/duration when possible, sends final progress if a media ID exists, then queues reset and navigation. |
| `Seek` | Updates drag/UI position only. |
| `SeekBarPressed` | Starts a drag only when the last mouse-derived seek position is valid. |
| `MouseMoved` | Computes seek position, updates UI immediately, and sends a direct seek at most every 100 ms while dragging; one pending value is retained. |
| `SeekRelease` | Sends the pending/final absolute seek, marks seeking, clears drag throttling fields, and persists the UI-side position. |
| `SeekDone` | Polls backend position, clears seeking, and persists progress; no current producer was found in the repository. |
| `SeekRelative` | Polls current position, clamps against source/known duration, issues an absolute Subwave seek, and updates UI optimistically. |
| `SeekTo` | Converts to seconds and delegates to `Seek`, so it changes drag/UI state rather than immediately seeking. |
| `SetVolume` | Interprets `1.1`/`0.9` as keyboard increments, clamps 0–1, stores state, and calls Subwave. |
| `ToggleMute` | Optimistically toggles state and calls Subwave. |
| `SetPlaybackSpeed` | Stores speed and calls Subwave; backend errors are discarded. |
| `SetContentFit` | Stores Iced `ContentFit`; the Subwave widget consumes it during view construction. External mpv is unaffected. |
| `ToggleFullscreen` | Optimistically flips `is_fullscreen` and emits an Iced window-mode event. |
| `DisableFullscreen` | Emits windowed mode if the boolean is true but does not clear the boolean locally. |
| `VideoClicked` | Single-click toggles play; a second click within 300 ms toggles fullscreen. |
| `VideoDoubleClicked` | Toggles fullscreen directly. |
| `ShowControls` / `CheckControlsVisibility` | Reveals controls or hides them after three seconds; the timer also expires track notifications. |
| Settings/menu toggles | Mutate only overlay visibility and mutually close selected menus. |
| `AudioTrackSelected` | Selects a Subwave integer index and updates a toast. |
| `SubtitleTrackSelected` | Selects an optional Subwave integer index, updates enabled state, and closes the menu. |
| `ToggleSubtitles` | Selects the current/first index when enabling, or `None` when disabling. |
| `CycleAudioTrack` | Increments the integer index modulo track count. |
| `CycleSubtitleTrack` | Cycles `None -> 0..N-1 -> None`. |
| `CycleSubtitleSimple` | Implements the existing off/first/last-used behavior with integer indices. |
| `TracksLoaded` | Only advances notification timeout; no current producer was found. |
| `ToggleAppsinkBackend` | Wayland diagnostic that switches Subwave between forced AppSink and forced Wayland; non-Wayland forces AppSink. |
| `ToggleShuffle` / `ToggleRepeat` | Toggle UI booleans only; no backend playlist command is issued. |
| `NextEpisode` | Persists current progress, resolves the next ordered episode, and preserves internal-vs-external mode. |
| `PreviousEpisode` | At or after 5% seeks/restarts the current episode; before 5% persists progress and opens the prior episode, preserving mode. |
| `EndOfStream` | Persists direct backend progress, auto-opens the next episode when present, otherwise resets and navigates back. No current producer was found in the repository. |
| `NewFrame` | Polls duration/position, clears a one-second seek timeout, lazily refreshes tracks, and updates notification state. |
| `ProgressHeartbeat` | Every ten seconds while internally playing, polls valid position/duration and sends watch progress. |
| `Reload` | No-op in the playback reducer; its comment refers to obsolete main-level handling. |
| `PlayExternal` | Waits for URL resolution, captures internal resume position, stops Subwave, and starts external mpv. Launch failure falls back to `load_video`. |
| `ExternalPlaybackStarted` | Log-only acknowledgement. |
| `PollExternalMpv` | Polls process liveness and JSON IPC state; emits update/end handling and fullscreen restoration. |
| `ExternalPlaybackUpdate` | Copies position/duration into state and advances `last_progress_sent`. |
| `ExternalPlaybackEnded` | Captures final position/fullscreen, persists progress, auto-advances episodes in external mode, or resets/navigates/restores the app window. |

## Lifecycle and persistence baseline

- Internal loading is synchronous on the UI thread once the ticket URL resolves.
- `video_opt.is_some()` is overloaded as session existence, rendering readiness, and streaming-start gating.
- Position and duration are copied into `last_valid_*` primarily from `NewFrame`; values at exactly `0.0` are generally treated as unavailable.
- The normal heartbeat interval is ten seconds. Frame callbacks and an additional ten-second `NewFrame` media subscription also poll backend state.
- Final progress is attempted on stop, back/home navigation, EOF, external process exit, and episode transitions. Internal load/auth errors do not have a common terminal-progress path.
- Internal teardown pauses and drops `SubwaveVideo`; there is no generation token, explicit event-channel close, or stale-callback rejection.
- External teardown depends on process-handle polling and per-branch field clearing.

## Track identity baseline

Subwave `AudioTrack` and `SubtitleTrack` values escape into `PlayerDomainState` and controls. Selection identity is an `i32` index. The same number is used both as a vector offset and as the backend selection argument. Reloads replace the vectors without preserving a Ferrex-owned stable identity; a prior subtitle index is retained separately for the simple toggle behavior.

## Content fit and presentation baseline

- The UI exposes `Contain`, `Cover`, `Fill`, `None`, and `ScaleDown` through Iced `ContentFit`.
- Fit is passed to the Subwave widget and is not represented as a backend capability.
- Wayland transparency is inferred from process environment plus Subwave backend preference.
- Desktop and 10-foot views each call `SubwaveVideo::widget`, so both must migrate to one presentation boundary.
- Fullscreen belongs to the Iced window in internal mode and to mpv in external mode. Internal state is optimistic; only external process teardown reports a final native fullscreen value for restoration.

## Dependency and packaging pins

| Input | Baseline |
|---|---|
| Iced fork | `Lowband21/iced-ferrex` commit `577abb7fa132ecd160adb5c8dfaf5c187b4f888d` |
| iced_aw fork | `Lowband21/iced_aw_ferrex` commit `6ebb6e587d2312bef9ca8c7f8acdf4e0f6384148` |
| Subwave | `Lowband21/subwave` `main` commit `4de8fd485a8077d17fd0f25e7b426988ac0da116` |
| gstreamer-rs | `main` commit `7922e962b267bdb645443615a5ae84239c71f19c` (`0.26.0-alpha`) |
| Nix GStreamer | Source overlay `1.28.4`; Rust toolchain `1.92.0` |
| Flatpak | Freedesktop `24.08`, Rust `1.92.0`, GStreamer core/base/good/bad/ugly/libav `1.28.4` |
| Windows CI | Official MSVC GStreamer `1.28.4` |
| Windows dist | Official MSVC GStreamer `1.28.4` |
| macOS handoff | Homebrew GStreamer exact gate `1.28.5`; pinned custom FFmpeg commit `38b88335f99e76ed89ff3c93f877fdefce736c13`; macOS `15.0` floor |
| Nix inputs | nixpkgs `9ae611a455b90cf061d8f332b977e387bda8e1ca`; rust-overlay `06f25b8e40805beb2121a4dae4cc37d6f981800f`; crane `59a82a1222dd3b2080b5cc52a1a2e8d5f1b77f37` |

Nix wraps the player with the pinned plugin paths and Linux graphics libraries.
Flatpak builds the media stack from source. Windows distribution starts from
the hash-pinned official SDK but stages only a reviewed plugin/PE/GIO/TLS
closure; OpenH264 and Media Foundation avoid a second FFmpeg ABI. macOS builds
FFmpeg and the mpv dependency core from exact sources, while Homebrew
GStreamer/build-support inputs are version/hash recorded and fail closed on
profile drift. Those rolling Homebrew/MSYS2/Rust inputs make the handoff paths
canonical and provenance-recorded, not bit-for-bit reproducible release
inputs.

## Baseline test gaps retained as P0 work

The reproducible synthetic media, authenticated range/HLS transport, initial platform inventory, Wayland operation matrix, and ignored results location are defined in [Native playback fixtures and test matrix](/developer/native-playback-fixtures/). The generator validates codecs, color signaling, HDR side data, subtitles, tracks, chapters, attachments, and malformed inputs without committing generated media.

Startup, seek, CPU/GPU, memory-cycle, hardware-decoder, compositor/HDR, and protocol-trace measurements still require runs on the physical environments in that matrix. EOF and seek-completion producer wiring also needs an explicit reproduction test before behavior is frozen.
