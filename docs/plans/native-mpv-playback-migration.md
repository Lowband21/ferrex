# Native mpv Playback Migration Plan

- **Status:** In progress
- **Branch:** `feat/mpv-integration`
- **Worktree:** `~/dev/ferrex/mpv`
- **Specification:** [Native mpv Playback Integration](../specs/native-mpv-playback.md)
- **Last updated:** 2026-07-24

## 1. How to Use This Plan

This document tracks implementation and migration. The specification is the
source of truth for architecture and acceptance criteria; this plan owns task
order, status, dependencies, decisions, and rollout checkpoints.

Update this file in the same change that:

- completes or materially changes a milestone;
- resolves an open design spike;
- changes fallback or rollout policy;
- adds a newly discovered blocker or risk; or
- changes a platform's default backend.

### Status legend

- `[ ]` Not started
- `[~]` In progress (use only one owner/status note beneath the item)
- `[x]` Complete and verified
- `[!]` Blocked; link or describe the blocker
- `[-]` Deliberately deferred or rejected with rationale

Checklist syntax cannot encode `[~]` or `[!]` as interactive Markdown boxes, so
those markers are textual status labels and must not be treated as completed.

## 2. Delivery Rules

1. Every phase must leave the player buildable and keep a working playback
   fallback.
2. Refactoring the player contract must land before mpv-specific branches
   spread through domain/view code.
3. The current default remains unchanged until the relevant platform gate
   passes.
4. Native-window mpv is implemented before integrated presentation; it is the
   compatibility and debugging baseline.
5. Release packaging is part of backend completion, not a follow-up.
6. Wayland is a gated feasibility project. Failure results in a documented
   hybrid backend, not unsafe framework coupling or silent HDR regression.
7. No mpv-specific Iced change is proposed upstream.
8. Each platform presenter must have a deterministic fallback and teardown
   test before it can be selected by Auto.
9. No decoded video frame may enter wgpu in the native-VO milestone.
10. Any private mpv patch requires an explicit specification amendment and a
    maintenance/exit strategy before adoption.

## 3. Milestone Overview

| ID | Milestone | Depends on | Status | Exit result |
|---|---|---|---|---|
| P0 | Baseline, fixtures, and design records | — | In progress | Reproducible current behavior and test matrix |
| P1 | Backend-neutral player contract | P0 | In progress | Subwave runs through Ferrex-owned commands/events |
| P2 | libmpv FFI and packaging foundation | P0 | In progress | Versioned libmpv loads/builds on target CI |
| P3 | mpv control plane and native-window vertical slice | P1, P2 | In progress | End-to-end playback without render API |
| P4 | Native presenter and Iced surface lifecycle | P1, P3 | In progress | Fake presenter and host lifecycle are stable |
| P5 | Windows presenter and X11 platform decision | P4 | Handoff ready; hardware gate open | Compile-gated owned-overlay spike plus deterministic fallback |
| P6 | macOS integrated presenter | P4 | Handoff ready; hardware gate open | Compile-gated AppKit child-window spike plus deterministic fallback |
| P7 | Wayland protocol bridge | P3, P4 | Complete (HYBRID) | GStreamer integrated; mpv native-window until a safe bridge exists |
| P8 | Playback feature parity | P3, platform presenter | In progress | Current player controls and tracks work through mpv |
| P9 | Hardening, performance, and release packaging | P5–P8 | In progress | Platform acceptance matrix passes |
| P10 | Staged default rollout | P9 | Not started | mpv selected by Auto per approved platform |
| P11 | Legacy cleanup and optional upstream work | P10 | Not started | Obsolete playback code removed after rollback window |

P2 packaging work and P5/P6 platform work may proceed in parallel after their
listed interfaces are stable. The recorded Wayland HYBRID decision does not
reduce the Windows or macOS scope: both remain active targets for fully
integrated native-VO mpv presentation inside the Ferrex player experience.
D-023 separately records X11 as HYBRID because mpv 0.41 excludes its X11 VO
from the LGPL-only build required by D-005.

## 4. P0 — Baseline, Fixtures, and Design Records

**Objective:** make regressions measurable before changing the player model.

### Documentation

- [x] Create the target architecture specification.
- [x] Create this migration tracking plan.
- [x] Link the specification, plan, and baseline from the canonical Starlight
  architecture page when implementation begins.
- [x] Record the exact current Iced fork revisions, Subwave revision, GStreamer
  version, and release packaging inputs in
  [`native-playback-baseline.md`](../src/content/docs/developer/native-playback-baseline.md).

### Current behavior inventory

- [x] Inventory every direct use of `SubwaveVideo` in the extracted playback
  and UI crates.
- [x] Inventory every branch on `external_mpv_active` and
  `external_mpv_handle`.
- [x] Map each `PlayerMessage` to current backend calls and resulting state
  changes.
- [x] Record current behavior for stop, EOF, error, next episode, previous
  episode, and navigation while playing.
- [x] Record current progress heartbeat and final progress persistence behavior.
- [x] Record current track identity/index semantics for audio and subtitles.
- [x] Record current content-fit behavior for contain, cover, and fill.
- [x] Record current fullscreen ownership and window restoration behavior.

The inventory above is maintained in
[`native-playback-baseline.md`](../src/content/docs/developer/native-playback-baseline.md)
and is anchored to the pre-contract `dev` commit recorded there.

### Test media and environments

- [x] Define a redistributable or locally generated media fixture set covering:
  - [x] H.264 SDR 8-bit;
  - [x] HEVC Main10 SDR;
  - [x] HDR10/PQ metadata;
  - [x] HLG;
  - [x] VP9 and AV1;
  - [x] ASS with fonts and animation;
  - [x] PGS bitmap subtitles;
  - [x] multiple audio/subtitle tracks;
  - [x] chapters and attachments; and
  - [x] malformed/unsupported input.
- [x] Document commands to generate synthetic fixtures when redistribution is
  not permitted.
- [x] Define authenticated HTTP range and transcoded-stream fixtures against a
  local Ferrex server.
- [x] Create the initial platform/GPU/compositor test inventory.

The schema-versioned generator and validator are
[`scripts/qa/native_playback_fixtures.py`](../../scripts/qa/native_playback_fixtures.py);
all generated media and checksummed manifests live under the ignored
`target/native-playback-fixtures/` directory. It creates nine primary files,
including locally constructed PGS packets, plus malformed inputs and segmented
HLS output. The loopback-only
[`native_playback_fixture_server.py`](../../scripts/qa/native_playback_fixture_server.py)
reads its token outside argv and supports bearer/query authentication and real
single-range `206` responses. Generation and validation passed with FFmpeg 8.1
on 2026-07-12; Linux CI now regenerates and verifies the same matrix. A
transport smoke separately verified `401`, bounded `206`, and
header-authenticated HLS segment reads without retained-token disclosure. On
2026-07-13 a second display-backed acceptance passed through a real
network-bound Ferrex router: it rewrote only the generated HLS fixture's local
segment references to credential-free protected stream routes, required the
same playback-scoped bearer ticket for the manifest and all four MPEG-TS
segments, verified HLS MIME types and unauthenticated rejection, and completed
the native-mpv control/screenshot lifecycle. This proves server/router HLS
transport and header propagation, independently of Ferrex-side transcode
generation. A third display-backed run on 2026-07-13 submitted the real `360p`
profile, waited for bounded FFmpeg generation and atomic cache publication,
verified ticket enforcement on the generated manifest and every segment,
confirmed cached reuse, and completed the same native-mpv
control/shader/screenshot/stop lifecycle.
[The fixture procedure and initial platform/Wayland matrix](../src/content/docs/developer/native-playback-fixtures.md)
also define local Ferrex import/direct/transcode acceptance. The manual player
quality-picker run and the UI episode run remain P8 gates.

### Baseline measurements

- [ ] Capture current startup-to-first-frame time.
- [ ] Capture CPU/GPU usage for SDR and HDR reference playback.
- [ ] Capture seek latency and resize/fullscreen behavior.
- [ ] Verify and record current hardware-decoder selection.
- [ ] Capture Wayland protocol traces for the known-good GStreamer HDR path.
- [ ] Run repeated load/stop cycles and record native/GPU memory behavior.

### Exit criteria

- [ ] Current player behavior is represented by a written mapping and tests
  where practical.
- [ ] Test fixtures and manual HDR procedure are reproducible.
- [ ] Baseline measurements are stored under an appropriate ignored or
  documented results location.

## 5. P1 — Backend-neutral Player Contract

**Objective:** remove backend objects from domain/view behavior before adding a
second in-process backend.

### Contract design

- [x] Keep the initial Ferrex-owned contract in the already extracted
  `crates/ferrex-player-playback/src/contract/` boundary; split it again only
  when another client needs a dependency-lighter crate.
- [x] Define `PlaybackCommand`.
- [x] Define `PlaybackEvent`.
- [x] Define `PlaybackSnapshot` and `PlaybackState`.
- [x] Define Ferrex-owned `TrackId`, `AudioTrack`, `SubtitleTrack`, chapter, and
  video-parameter models.
- [x] Define `PlaybackCapabilities`, `BackendKind`, and presentation capability
  models.
- [x] Define structured `PlaybackError` and `FallbackReason` types.
- [x] Define source/authentication data without embedding access tokens in log
  output.
- [x] Define controller/event channel ownership and shutdown semantics.

### Subwave adapter

- [x] Wrap `SubwaveVideo` behind the new command/event contract.
- [x] Convert Subwave audio/subtitle models into Ferrex-owned models.
- [x] Move direct `SubwaveVideo` polling out of player `view` code.
- [x] Reduce adapter events into one `PlaybackSnapshot`.
- [x] Preserve current progress, seek timeout, controls, and track behavior.
- [x] Preserve current backend toggle only as a Subwave diagnostic during the
  migration; do not add it to the generic contract unless it represents a real
  user capability.

### Player domain migration

- [x] Replace the concrete value in `PlayerDomainState::video_opt` with a
  backend-neutral `PlaybackSession` handle and reduced snapshot (the temporary
  field name is retained for compatibility during the migration).
- [x] Stop storing Subwave track types in `PlayerDomainState`.
- [x] Move concrete position/duration polling into adapter snapshot event
  reduction; legacy `last_valid_*` mirrors remain only for seek-preview and
  persistence compatibility until P11 cleanup.
- [x] Consolidate internal and external playback branches where behavior is
  backend-independent.
- [x] Keep `external_mpv.rs` operational through an adapter or explicit legacy
  path.
- [x] Ensure `view.rs` reads only snapshot and presenter state.
- [x] Ensure player commands do not unwrap `current_media_id` on paths where it
  may legitimately be absent.

The external process remains an explicit legacy path rather than a generic
backend session. It now starts mpv idle, observes state over its private IPC
socket, and submits the media URL with `loadfile` over IPC so playback tickets
do not appear in the child argument vector. Its Unix socket lives under an
RAII-owned `0700` temporary directory. On 2026-07-12 the ignored Linux real-mpv
smoke passed against a generated Matroska file behind a local query-ticket HTTP
range server, verifying IPC load, observed media state, and
`/proc/<pid>/cmdline` non-disclosure while preserving the existing
process-liveness fallback. The bounded legacy snapshot synchronization now
samples Subwave's owned EOS flag inside the adapter and reduces it to
`Ended(Eof)` instead of allowing the backend's terminal pause to overwrite EOF.
The same generation-scoped terminal handler is used by legacy ticks, the
progress heartbeat, and event-driven mpv wakeups, so EOF/error dispatch and
final progress happen once. Event-driven snapshots now clear seeking only from
mpv's confirmed state, while Subwave retains its one-second timeout; snapshot
projection also preserves pending resume hints and the last available subtitle
selection. Pure regression tests cover EOS-vs-pause ordering, confirmed seek
completion, one-shot terminal dispatch/error progress, resume projection, and
subtitle restoration. The retained external process now has one reduced
`PlaybackSnapshot`; its native handle owns only process/IPC resources. Desktop
and 10-foot views select presentation through that snapshot and obtain the
backend-owned widget through a state presentation boundary, while progress
heartbeats, episode mode, and navigation use one neutral progress projection.
The obsolete `external_mpv_active` flag and all direct view reads of the process
handle/session field are removed. Final IPC values are reduced before the
process handle is dropped, fixing external episode advancement and avoiding the
old duplicate terminal progress/navigation dispatch. State/update regression
tests cover external loading/playing/terminal reduction, invalid observations,
reset cleanup, heartbeat persistence, and terminal episode transition after
handle teardown.

### Tests

- [x] Command-to-Subwave adapter tests.
- [x] Snapshot reducer tests for duplicate, out-of-order, and missing values.
- [x] Track identity/selection tests across reload.
- [x] Stop/EOF/error progress persistence tests.
- [x] Episode transition tests independent of backend.
- [x] Fallback-policy unit tests.

### Exit criteria

- [ ] Existing GStreamer playback works with no intended UI behavior change.
- [x] `SubwaveVideo` is not referenced by player view or domain policy code.
- [x] Backend-specific track types do not escape the adapter.
- [x] All new contract/reducer tests pass.

## 6. P2 — libmpv FFI and Packaging Foundation

**Objective:** select and ship a libmpv foundation capable of exposing the full
control API on every desktop target.

### FFI decision spike

Evaluate at least:

- a thin Ferrex wrapper over maintained raw `libmpv` bindings;
- `libmpv2` plus direct raw access for missing APIs; and
- generated/local bindings with dynamic symbol loading.

For each option record:

- [x] supported client API and mpv versions;
- [x] command/property/node/event coverage;
- [x] wakeup callback and async reply support;
- [x] raw escape-hatch feasibility;
- [x] Windows and macOS linking behavior;
- [x] maintenance activity and licensing;
- [x] cross-compilation behavior; and
- [x] ability to test with a fake function table.

Spike result (crate releases and repository activity checked 2026-07-11):

| Option | Coverage and version | Build/test characteristics | Decision |
|---|---|---|---|
| `libmpv2` 6.0.0 plus raw access | Maintained; high-level crate declares API 2.2 while `libmpv2-sys` 4.0.1 ships current API 2.5 declarations. High-level events omit node payloads and it exposes no public async command/property, node-command, hook, or log-request methods. | LGPL-2.1; linked through `libmpv2-sys`; public raw context permits escape hatches but couples two ownership layers and is not naturally fakeable. | Rejected as the ownership layer; useful only as prior art. |
| Thin Ferrex wrapper over `libmpv2-sys` 4.0.1 | Complete API 2.5 client/node/event/wakeup/async declarations from mpv 0.40+ headers, including APIs required by P3. | Maintained, LGPL-2.1, pregenerated bindings avoid target bindgen/Clang, and the build script links `mpv`. Ferrex's own function table makes it fakeable and keeps unsafe ownership local. | **Selected.** |
| Generated/local bindings with dynamic symbol loading | Can provide complete coverage and graceful runtime absence. | Adds header/generated-code drift, a library-lifetime loader, per-platform search policy, and a second binding-maintenance surface before packaging is proven. | Deferred; reconsider only if linked development/release layouts cannot meet diagnostics or rollback requirements. |

`mpv-client-dyn` 0.5.0 was also rejected: it is GPL-3.0, hard-codes
`mpv.exe`, and omits required version, terminate, wakeup, node/async-property,
and log APIs. `mpv-client-cross-sys` 4.0.0 is current but GPL-3.0 and its
dynamic-symbol path is designed for C plugins hosted by mpv, not an embedding
application.

Decision:

- [x] Write `D-004` in the decision log with the selected FFI foundation.
- [x] Set the minimum supported client API version to 2.2 (mpv 0.37.0),
  which contains every P3 client symbol; release packaging targets mpv 0.41.0
  / API 2.5.
- [x] Decide release bundling versus development dynamic loading: use normal
  shared-library linking, keep it behind the `linked` Cargo feature, use an
  explicitly LGPL-only mpv/FFmpeg build, and bundle/reference that exact shared
  library in release artifacts. Do not add a bespoke runtime `dlopen` layer
  unless packaging evidence requires it.

### Build integration

- [x] Add the selected Rust dependencies with minimal features in the isolated
  `ferrex-player-mpv` crate; its default feature set does not link libmpv.
- [x] Add compile-time platform gating without compiling Wayland dependencies on
  Windows/macOS (`linked` enables only the raw client bindings).
- [x] Add libmpv version detection and actionable build errors: Unix builds
  require `mpv >= 2.2.0` through pkg-config, Windows names the required
  `LIBMPV_LIB_DIR`, and runtime client API compatibility is checked before
  allocation.
- [x] Add a runtime compatibility report.
- [x] Ensure the player can still build in a configuration where mpv is
  deliberately disabled during the migration.

### Packaging workstream

- [x] Nix development shell provides mpv 0.41.0 headers and runtime library
  built with `gpl=false`, LGPL-only FFmpeg, and GPL-only optional inputs
  disabled.
- [x] Nix package references the selected LGPL libmpv closure and includes its
  library directory in the wrapped runtime path; package and license-profile
  checks pass.
- [x] Linux CI installs `libmpv-dev` at the API 2.2 compatibility floor and
  runs the linked handle smoke test; Nix/release packaging remains pinned to
  mpv 0.41.0.
- [x] Flatpak manifest builds/bundles libmpv with required VO/protocol features.
- [~] Windows CI/source builder, MSVC import-library generation, hashed DLL
  closure staging, and package audit are implemented; the first target CI and
  clean-VM artifact run remain representative-system gates.
- [~] macOS CI/source builder and complete dylib closure rewrite/audit are
  implemented for Apple Silicon and Intel; the first target CI and clean-app
  launch remain representative-system gates.
- [~] Windows/macOS package audits reject loader dependencies and runtime search
  paths into developer Nix/Homebrew prefixes; execute them against the produced
  target artifacts before closing this item. These checks do not claim to scan
  arbitrary resource strings for unrelated build-host paths.
- [~] The target builders emit exact mpv, FFmpeg, libplacebo, libass, and
  target Lua runtime profiles/notices/hashes (LuaJIT on Windows/Flatpak, static
  Lua 5.2.4 on macOS); final license review remains open until the target
  artifacts are produced.

The Flatpak manifest now builds the player with its `mpv` feature and pins mpv
0.41.0/API 2.5, FFmpeg 8.1.2, libplacebo 7.360.1, libass, and LuaJIT. Configure
and post-install assertions require `gpl=false`, reject FFmpeg GPL/nonfree/
version-3 options, require Vulkan/Wayland/dmabuf support, verify the final Rust
binary directly needs `libmpv.so.2`, and install the component license/build
profile files. `gst-libav` is built against the same bundled FFmpeg ABI so the
process does not load both the runtime and bundled FFmpeg versions. On
2026-07-12 a clean Flatpak builder run, 50 MiB bundle export, user installation,
and installed-runtime loader smoke passed; libmpv, FFmpeg, libplacebo, libass,
and LuaJIT all resolved from `/app/lib` without a Nix/store or host package
path. The Flatpak workflow now installs every produced bundle and repeats the
closure/profile assertions before upload. mpv 0.41 gates X11 VO/`wid` sources
on its GPL option, so D-023 keeps them out of the reviewed bundle and selects
GStreamer integration on X11; the required Wayland native-window
`gpu-next`/Vulkan path remains present.

### Exit criteria

- [ ] A minimal program creates and destroys a libmpv handle on Linux, Windows,
  and macOS CI or documented equivalent builders.
- [x] Version/capability diagnostics are available.
- [x] Release package layout is defined for all targets.
- [ ] The FFI decision is recorded and reviewed.

## 7. P3 — mpv Control Plane and Native-window Vertical Slice

**Objective:** deliver end-to-end in-process playback using mpv's ordinary
native window before attempting embedding.

### Session core

- [x] Add RAII ownership for `mpv_handle`.
- [x] Set deterministic pre-initialization options.
- [x] Initialize without creating `mpv_render_context`.
- [x] Add the serialized command/property owner.
- [x] Install a wakeup callback that only signals the owner/runtime.
- [x] Drain and copy events safely.
- [x] Correlate async command and property replies.
- [x] Forward mpv logs with level mapping and redaction.
- [x] Implement ordered stop and termination.
- [~] AppKit presenter work is main-thread-token-gated, detaches before
  shutdown, and hands the blocking `MpvWorker` drain to a named off-main
  reaper; a real macOS load/quit/fullscreen stress run remains required.

The first P3 control-plane tranche lives entirely in `ferrex-player-mpv`.
`MpvSession` is a thread-affine serialized owner; `MpvWorker` creates it on a
named owner thread, wakes through an atomic/unpark-only callback, forwards only
owned events, and performs a bounded stop/reply/final-event drain before RAII
termination. The local session form remains available for the unresolved macOS
main-loop model.

### Generic mpv API

- [x] Set/get string, flag, integer, double, and node properties.
- [x] Observe and unobserve arbitrary properties.
- [x] Submit arbitrary async commands and node commands.
- [x] Expose hook/client-message support needed by scripts and future features.
- [x] Expose API/FFmpeg/libplacebo version diagnostics.
- [x] Add an explicit raw/unsafe extension boundary.

### Ferrex mapping

- [x] Load authenticated HTTP media without exposing tokens in logs/process
  arguments.
- [x] Observe pause, time, duration, cache, seeking, EOF, and idle state.
- [x] Observe tracks, chapters, editions, video parameters, and hardware decoder.
- [x] Map core events into `PlaybackEvent`.
- [x] Map play/pause, absolute/relative seek, volume, mute, and speed.
- [x] Map audio/subtitle selection.
- [x] Map stop and end reasons.
- [x] Preserve final and heartbeat watch progress.

The Ferrex mapping now lives in the feature-gated
`ferrex-player-playback::mpv_adapter`. Direct-stream ticket resolution creates a
credential-free URI plus a zeroizing `Authorization` header on
`PlaybackSource`; both Subwave and libmpv receive that source in process. Only
the explicit legacy external-player boundary reconstructs a query-ticket URL.
The adapter submits authenticated sources as in-process node commands with
per-file options, validates header/cookie input, redacts source-specific secrets
from copied logs, and reduces the observed mpv property/event surface into the
existing generation-scoped snapshot. The mpv owner emits a coalesced
backend-neutral readiness signal after copied events
are queued, so Iced drains them without a video-frame or periodic polling
redraw loop. Async load failure falls back to Auto/Subwave from the last
observed position. A schema-versioned, serializable diagnostic snapshot now
reports requested/selected backend, backend and presenter lifecycle, client API
compatibility, mpv/FFmpeg/libplacebo versions, compiled features, VO/GPU
context and adapter, hwdec/interop, input/output color parameters, frame timing
counters, presenter geometry/display scale, the ordered deduplicated fallback
chain, and the last structured fallback/error. Geometry and fallback history
introduced diagnostic schema version 2; additive chapter/edition capabilities
advanced it to version 3; effective mpv config/script trust policy advanced it
to version 4. Capability-gated local extensions and the redacted active shader
count advanced it to version 5. The effective native log policy advances the
current schema to version 6. By default, startup-only verbose logging is
reduced to informational filtering after file initialization; the explicit
`FERREX_MPV_LOG_LEVEL` diagnostic switch can instead retain a fixed native
filter without recording log contents or an invalid environment value.

### Native-window vertical slice

- [x] Add an opt-in backend selector for in-process mpv native-window mode.
- [x] Play a local fixture.
- [x] Play an authenticated Ferrex URL.
- [x] Verify server transcoding output. The bounded FFmpeg job provider,
  start/status/assets routes, quality-profile request, authenticated rendition
  source, atomic cache publication, protected reload, and display-backed
  native-mpv run pass.
- [x] Verify next-episode transition through real native-mpv EOF and the
  backend-neutral replacement path.
- [x] Verify ordinary mpv fullscreen and close handling.
- [x] Enable a controlled mpv OSC fallback in native-window mode.
- [x] Keep existing external mpv fallback available.

Evidence: the ignored
`mpv_adapter::tests::linked_native_window_load_control_fullscreen_stop_and_close_smoke`
test loaded a locally generated MPEG-4/AAC Matroska fixture through the real
mpv 0.41.0 `gpu-next` native VO on 2026-07-11, then exercised metadata/track
observation, pause, seek, confirmed fullscreen enter/exit, stop, replacement
load, orderly native-window quit, and teardown. The copied quit event is kept
distinct from EOF, and the domain test verifies that close/core termination
persists final progress and exits rather than auto-advancing an episode. The
transport form of the same smoke path passed again on 2026-07-12 against a
temporary authenticated HTTP range server whose media endpoint required the
current bearer header; it completed load, metadata/track discovery, pause,
fullscreen enter/exit, seek, stop, replacement load, and close. An earlier
variant also covered query-ticket and cookie input. On 2026-07-12 the expanded
smoke also passed against the schema-generated multitrack fixture, confirming
an initial resume offset, observed volume, mute, speed, content-fit,
audio-track selection, subtitle selection/off, chapter selection, edition
catalog/selection, confirmed fullscreen, absolute/relative seek, explicit stop,
natural EOF, post-terminal reload, and native close.
The normal server integration test separately proves that a real Ferrex router
accepts the scoped playback ticket (not a full account session) in the same
`Authorization: Bearer` form and serves a bounded `206` range. The stream
handler now also returns demuxer-appropriate MIME types for protected HLS
manifests, MPEG-TS/AAC segments, and fragmented-MP4 segments. On 2026-07-13
the feature-gated ignored
`playback_ticket_drives_display_backed_native_mpv_through_ferrex_router` test
then combined both ends against an isolated PostgreSQL database and a real
network-bound Ferrex router: it registered a user, seeded the generated H.264
fixture with its actual size, issued the normal playback-scoped ticket, and
opened that protected URL through the backend-neutral exact-mpv session. The
real native VO confirmed resume/metadata, pause, an authenticated range seek,
shader application, a non-empty screenshot, redacted diagnostics, and ordered
stop. The normal feature suite remains display-free; the test is opt-in through
the server's `native-mpv-e2e` feature and its command is documented with the
fixture procedure. A second run the same day loaded the generated
`transcoded-hls/index.m3u8` transport fixture through protected real-router
URLs, requiring one header-carried ticket on the manifest and every segment;
it verified unauthenticated rejection, credential-free manifest URLs,
redacted diagnostics, controls, seek, shader, screenshot, and ordered stop.
That closes router/HLS transport propagation independently. A third ignored
acceptance,
`server_generated_transcode_plays_through_display_backed_native_mpv`, passed on
2026-07-13 with the real bounded FFmpeg provider and generated HLS assets. It
submitted `360p`, polled the authenticated job to completion, verified atomic
publication, unauthenticated rejection and ticket access for the manifest and
every segment, confirmed immediate cached reuse, and completed the same real
native-VO resume/control/seek/shader/screenshot/ordered-stop lifecycle. The
manual quality-picker run and end-to-end UI episode transition remain open.
The ignored
`update::tests::linked_native_window_eof_reloads_next_episode_with_same_backend`
smoke also passed on 2026-07-13. It let the first synthetic episode reach real
native-mpv EOF, required one final-progress plus backend-preserving next-episode
request, then drove the normal `SetStreamSource` close/reopen path and confirmed
a newer mpv session generation playing the second episode. This closes the P3
backend/domain transition; outer repository selection, ticket resolution, and
the visible app-shell transition remain in the P8 manual UI gate.

### Tests

- [x] Fake-FFI tests for copied event lifetimes.
- [x] Node conversion tests including nested maps/arrays and null values.
- [x] Async reply correlation and cancellation tests.
- [x] Wakeup storm/coalescing tests.
- [x] Stop during load/seek/EOF tests.
- [x] Repeated session create/destroy test.

### Exit criteria

- [ ] mpv plays supported fixtures through its native VO with no render context.
- [x] Current basic controls, tracks, EOF, and progress work through the generic
  player contract.
- [x] Hardware-decoder and VO diagnostics are visible.
- [x] Failure returns cleanly to GStreamer/external fallback.

## 8. P4 — Native Presenter and Iced Surface Lifecycle

**Objective:** implement platform-neutral host geometry/lifecycle before native
platform attachment code.

### Presenter state model

- [x] Define host-ready, VO-ready, attach, hidden, suspended, detach, and failure
  transitions.
- [x] Add monotonically increasing session/presenter generations.
- [x] Define presenter commands and events without requiring native resources to
  be `Send`.
- [x] Define logical bounds, visible bounds, scale factor, and geometry revision.
- [x] Define fullscreen ownership and actual-state confirmation.
- [x] Define deterministic fallback requests.

The platform-neutral implementation is in
`ferrex-player-playback::presenter`. `PresenterLifecycle` accepts only
session/presenter-generation-scoped inputs, emits UI-thread-local commands plus
existing playback presenter events, attaches at most once per generation, and
rejects stale generations and geometry revisions. `NativePresenter` uses a
borrowed generic associated host with no `Send` bound, so later Wayland,
AppKit, and window-system resources can remain event-loop-local. Fullscreen
changes reach the playback snapshot only after native confirmation; presenter
failures detach first and request the configured native-window fallback.

### `NativeVideoSlot`

- [x] Implement a renderer-generic custom widget outside Iced.
- [x] Store attachment state in `Tree::State`.
- [x] Acquire host raw handles through current generic Iced APIs.
- [x] Synchronize only on geometry revisions during redraw.
- [x] Handle zero size and full clipping as hidden.
- [x] Detach on tree removal and explicit window-close flow.
- [x] Draw loading/failure fallback without drawing decoded video.
- [x] Remove continuous redraw behavior used only for polling.

`ferrex-player-playback::native_video_slot` now provides a renderer-neutral
layout slot and an explicit `window::run` host-capture task. Raw window/display
handles remain in an event-loop thread-local registry and are exposed only as a
borrow during presenter callbacks, preserving the presenter's non-`Send`
contract. `Tree::State` owns the generation handle, monotonically revisions
changed bounds/clip/scale observations only on redraw, requests host capture at
most once while absent, and performs idempotent detach on replacement, drop,
and close request. Loading/failure plates use only generic renderer quads; the
slot contains no decoded image or wgpu video primitive. Unit tests cover raw
host capture, duplicate suppression, scaling, clipping/zero size, deferred host
capture, and detach-before-drop. Platform callbacks and selection of the slot
remain gated on P5–P7 presenters. Desktop and 10-foot views no longer register
a decoded-frame callback for player-state updates: native backends wake through
the copied-event signal, while the legacy adapter synchronizes only on the
bounded controls timer and the existing low-rate progress heartbeat.

### Dedicated playback overlay window

- [x] Add a player/overlay `WindowKind` to the existing daemon window manager.
- [x] Create transparent overlays hidden before native attachment.
- [x] Render only the player UI for the overlay window.
- [x] Keep the library/main window alive but hidden or suspended during dedicated
  native-root playback.
- [x] Restore geometry/focus after playback.
- [ ] Ensure one visible player/taskbar identity at a time.

The daemon window manager now owns a deterministic
`Closed -> Hidden -> Active -> Closing` player-overlay lifecycle. Allocation is
transparent, undecorated, and invisible. Native attachment/positioning occurs
while hidden; an explicit post-attachment task hides the still-live main
window before a follow-up marks the presenter host visible and focuses it.
That follow-up never reapplies stale main-window geometry. The window manager
also retains a separate live overlay viewport for controls/focus/hit testing,
leaving main geometry untouched for restoration. User close detaches every
registered native slot and
releases the event-loop-local raw-host lease before queuing native destruction,
then restores the retained main geometry, fullscreen mode, and focus. A
separate dismiss path preserves playback during presenter fallback. Every
completed exit now funnels through a backend-neutral `PlaybackExited` window
event; the app shell idempotently dismisses an active overlay, detaches its
host, and restores the retained main geometry/focus after stop, EOF, native
close, Back, or Home. Pure manager/settings/theme tests cover map replacement,
lifecycle ordering, hidden allocation, explicit surface alpha, and exit
dismissal without player mutation; the native-slot test covers multi-slot
detach-before-host-release. Platform presenters must still establish native
ownership/z-order and prove the single taskbar/Alt-Tab identity in P5/P6.

### Transparency

- [x] Make player background alpha explicit.
- [x] Verify the wgpu surface uses a compositing alpha mode where required.
- [x] Verify Iced does not advertise a full opaque region over video.
- [ ] Verify controls and text remain SDR and readable over HDR output.

The pinned Iced revision `577abb7f` selects post-multiplied alpha when
available, then pre-multiplied alpha, and configures every wgpu surface with
the selected mode. The dedicated player window is created with
`Settings::transparent = true`; winit 0.30.13 responds on Wayland by issuing
`wl_surface.set_opaque_region(null)` instead of the full-surface opaque region.
Ferrex unit tests independently require the hidden overlay setting and its root
theme background to remain transparent. Actual compositor support and SDR UI
legibility over HDR remain platform acceptance measurements rather than an
assumption from these code paths.

### Fake presenter tests

- [x] attach occurs once per generation;
- [x] host-before-VO and VO-before-host ordering;
- [x] duplicate geometry suppression;
- [x] clipping/hide/show transitions;
- [x] scale and window recreation;
- [x] stale event rejection;
- [x] explicit close before drop; and
- [x] presenter error to fallback transition.

### Exit criteria

- [x] The presenter/widget contract is stable without mpv or platform-specific
  types in Iced-facing public APIs.
- [x] Fake presenter lifecycle tests pass.
- [x] Player UI no longer needs video-frame redraws to update progress.
- [x] No Iced fork change has been added for native presentation.

## 9. P5 — Windows and X11 Presenters

Windows may proceed after P4 and remains a target for fully integrated
native-VO mpv presentation; the Wayland HYBRID decision does not defer or
weaken its production presenter gate. X11 is now a separate licensing-gated
HYBRID under D-023: the reviewed LGPL libmpv profile has no X11 VO or `wid`
implementation, so the checklist is retained only as re-entry criteria.

### Windows

- [x] Observe/query mpv `window-id` as a full pointer-width `i64`, reject zero
  or out-of-range values, and validate with `IsWindow` before attach.
- [x] Choose and record an mpv-root/owned-Iced-overlay relationship.
- [x] Allocate the Iced overlay hidden and reveal it only after attachment.
- [x] Synchronize the mpv client rectangle and per-monitor DPI; the active
  spike re-queries the native root independently of Iced layout revisions.
- [x] Hide the retained main window before presenter-driven reveal/focus and
  keep live overlay viewport geometry independent from restoration geometry.
- [x] Implement owned-window z-order, minimize/restore/visibility, focus
  handoff, task-switcher styles, and idempotent restoration.
- [x] Delegate fullscreen to mpv and update state only from its observed
  confirmation.
- [x] Route integrated controls/input through Iced and disable mpv OSC/default
  input for the integrated request.
- [x] Detach and restore the overlay before either HWND is destroyed.
- [-] `wid` inline mode is not retained for the full-player experience; the
  native-root owned-overlay path preserves mpv's modern VO and the ordinary
  native window is the deterministic fallback.
- [~] One taskbar entry and correct Alt-Tab behavior are ready for the
  representative Windows matrix; target observation remains open.
- [~] SDR/HDR overlay-visible/hidden behavior is ready for representative
  display testing; native HDR capability remains false until recorded.
- [~] D3D11 `gpu-next` and D3D11VA/DXVA2 diagnostics are packaged and exposed;
  actual hardware evidence remains open.
- [~] The generic lower-level stress harness exists; the Win32
  owned-overlay-specific 100-cycle run remains open.

**Windows exit decision:**

- [~] The compile-gated owned-overlay implementation is ready for
  representative-system handoff; production/Auto approval remains open.
- [x] Native-window mode is the explicit structured fallback and render API
  integration is not forced.

### X11

**Status:** Deferred under D-023; retained as X11 re-entry criteria.

- [-] Detect X11 backend and compositing-manager presence.
- [-] Obtain mpv and Iced XIDs and verify display/screen compatibility.
- [-] Create/attach an ARGB overlay above the mpv window.
- [-] Synchronize configure, map/unmap, stack, focus, and scale behavior.
- [-] Define input shape/region behavior.
- [-] Delegate and confirm fullscreen state.
- [-] Detach/destroy in protocol-safe order.
- [-] Implement/test `wid` inline mode.
- [-] Test with and without a compositing manager.
- [-] Verify one taskbar entry and window-manager compatibility.
- [-] Stress 100 window/session cycles.

**X11 exit decision:**

- [-] Integrated mpv and `wid` are not built from mpv 0.41's GPL-only X11
  sources under D-005's LGPL release policy.
- [ ] Verify packaged X11 GStreamer fallback and the optional external-process
  handoff before rollout; do not advertise in-process mpv native-window mode.

## 10. P6 — macOS Presenter

**Objective:** preserve mpv's native modern macOS VO while delivering a fully
integrated Ferrex player and Iced controls where AppKit permits. The Wayland
HYBRID decision does not change this target.

### AppKit spike

- [x] Confirm mpv 0.41 returns its live `NSWindow` pointer through the
  read-only `window-id` property; no unsupported macOS `wid` input is used.
- [x] Resolve and retain the mpv `NSWindow` and Iced host `NSView`/`NSWindow`
  only with an AppKit main-thread marker.
- [x] Implement a transparent Iced child `NSWindow` above mpv's content view.
- [~] Movement, resize, backing-scale, focus, occlusion, close, and app
  visibility synchronization are implemented and fake-tested; target
  observation remains open.
- [~] Fullscreen ownership/confirmation and auxiliary-window behavior are
  implemented; native animation observation remains open.
- [~] Active-Space and hide/unhide visibility refresh is implemented; the
  representative Spaces matrix remains open.
- [~] Apple Silicon and Intel build/package jobs are defined; representative
  hardware execution remains open.
- [~] Child-window composition is the selected handoff strategy; retain the
  native-window fallback until representative testing proves it sufficient.

### Production presenter

- [x] Implement the child-window relationship behind the presenter contract.
- [x] Keep all AppKit object access behind the non-`Send` main-thread window
  system.
- [x] Synchronize the root content-view screen rectangle rather than the outer
  frame.
- [x] Detach the AppKit relationship first, then move blocking libmpv shutdown
  to a named reaper so the main run loop remains serviceable.
- [~] VideoToolbox diagnostics are exposed; representative hardware decoding
  evidence remains open.
- [~] HDR/EDR overlay-visible/hidden validation remains open on a capable
  display, and native HDR capability stays false meanwhile.
- [~] The macOS child-window-specific 100-cycle playback/fullscreen/teardown
  run remains open.

### Exit decision

- [~] Integrated capability is enabled only in an explicit compile-time spike
  for representative-system handoff; Auto/production remains closed.
- [x] Any preflight or attachment failure selects mpv native-window mode and
  dismisses the hidden Iced host.
- [-] Do not substitute a deprecated OpenGL render path solely to claim
  embedding.

The exact target build commands, representative fixture matrix, retained
artifact rules, and production-pass boundary for P5/P6 are documented in
[`native-playback-fixtures.md`](../src/content/docs/developer/native-playback-fixtures.md#windows-and-macos-integrated-presenter-handoff).

## 11. P7 — Wayland Protocol Bridge

**Objective:** determine whether mpv's normal Wayland VO can be safely
virtualized as an Iced subsurface without copying frames or modifying Iced with
platform hacks. W0 found the connection bootstrap unsafe under the current
boundary, so D-022 records HYBRID and defers bridge implementation.

This phase has recorded the **HYBRID** outcome in D-022. GStreamer remains the
integrated Wayland backend and mpv remains available through ordinary
native-window presentation. W1–W5 are retained below as re-entry criteria, but
are deliberately deferred until a safer per-session Wayland connection path or
other maintainable architecture exists. Auto defaults are unchanged.

### W0 — Research fixture and bridge boundary

- [x] Pin the mpv version used by the spike.
- [x] Inventory every Wayland global/protocol used by mpv for `gpu-next` on the
  test environment.
- [x] Evaluate reuse/forking of the `wl-proxy` library used by Jellyfin's
  precedent.
- [x] Define a raw protocol trace fixture for basic map, resize, fullscreen,
  frame presentation, and teardown.
- [x] Define how the bridge identifies the intended mpv VO connection/surface.
- [!] Define how only mpv is directed to the private socket without racing other
  process users of `WAYLAND_DISPLAY`/`WAYLAND_SOCKET`.

The versioned
[`native_playback_wayland_trace.py`](../../scripts/qa/native_playback_wayland_trace.py)
harness pins mpv 0.41.0, runs ordinary `gpu-next`/Vulkan/`waylandvk`, inserts
operation markers for map, pause/seek, resize, fullscreen, stop, VO reload, and
teardown, and writes only redacted mode-private artifacts below the ignored
results directory. Its display-free parser/redaction tests run in Linux CI.
Three `wl-wlroots-amd` runs against the generated SDR, HDR10/PQ, and HLG
fixtures passed on 2026-07-13 UTC with Vulkan hardware decoding and the expected
input color parameters. They used the same protocol-interface set, issued ten
registry requests across mpv/libplacebo/driver activity, and exposed exactly
one `xdg_surface.get_toplevel` VO candidate per run. The exact globals,
interfaces, per-method inventory, surface-identification rule, and evaluation
of permissively licensed `wl-proxy` 0.1.3/Jellyfin precedent are recorded in
[the W0 spike page](../src/content/docs/developer/native-mpv-wayland-spike.md).

Stable libmpv provides no per-context Wayland endpoint, and a temporary
process-environment override cannot cover delayed/internal-thread VO and driver
connections safely. The only race-free candidate found so far is a
process-lifetime startup proxy that routes Iced and mpv into one upstream
namespace and virtualizes only the protocol-identified mpv shell candidate.
That is broader than the specification's private mpv-only socket, so it is not
selected. D-022 records HYBRID and D-008 is deferred. Reopening W1 requires a
compliant redirection mechanism or an explicit specification amendment backed
by a maintainable ownership/teardown design. No environment race or symbol
interposition is accepted.

### W1 — Same-upstream connection proof

**Status:** Deferred under D-022; retained as Wayland re-entry criteria.

- [ ] Obtain Iced's borrowed `wl_display` and parent `wl_surface` safely.
- [ ] Build an upstream client/event queue over the borrowed display without
  taking ownership of it.
- [ ] Start a private downstream Wayland socket for mpv.
- [ ] Forward registry/global binding and core object traffic.
- [ ] Prove the mpv child and Iced parent are objects on the same upstream
  connection.
- [ ] Prove bridge teardown does not disconnect or consume Iced's display.
- [ ] Test concurrent event queues for deadlock/starvation under resize and
  playback.

**Gate W1:**

- [-] Continue only if same-connection forwarding and ownership are reliable;
  deferred because no safe mpv-only connection bootstrap exists.
- [x] Record a hybrid decision: retain GStreamer integrated Wayland and use mpv
  native-window mode as the mpv fallback.

### W2 — Shell-role virtualization

**Status:** Deferred under D-022; retained as Wayland re-entry criteria.

- [ ] Capture the mpv video `wl_surface`.
- [ ] Suppress upstream `xdg_wm_base.get_xdg_surface` for that surface.
- [ ] Suppress its toplevel role and virtualize required downstream objects.
- [ ] Assign upstream `wl_subsurface` under Iced's parent.
- [ ] Set desynchronized child commits.
- [ ] Set position from the surface-slot geometry.
- [ ] Apply an empty native input region or otherwise ensure Iced owns input.
- [ ] Synthesize initial and subsequent configure events with valid serials.
- [ ] Consume/validate downstream ack-configure behavior.
- [ ] Handle surface recreation and VO restart generations.

### W3 — Protocol and WSI preservation

**Status:** Deferred under D-022; retained as Wayland re-entry criteria.

- [ ] Vulkan WSI playback through the bridge.
- [ ] EGL/OpenGL fallback where supported.
- [ ] dmabuf file-descriptor forwarding.
- [ ] explicit synchronization and release behavior.
- [ ] viewporter without creating a competing host viewport.
- [ ] fractional-scale events and mixed-DPI display movement.
- [ ] output enter/leave behavior.
- [ ] presentation-time/frame callbacks.
- [ ] tearing-control/content-type where selected by mpv.
- [ ] idle-inhibit behavior.
- [ ] color-management and color-representation objects owned by mpv.
- [ ] gracefully forward or reject unknown/unsupported optional protocols.

### W4 — Host window semantics

**Status:** Deferred under D-022; retained as Wayland re-entry criteria.

- [ ] Translate fullscreen requests to Iced and synthesize resulting state.
- [ ] Translate close requests.
- [ ] Define minimize/maximize behavior.
- [ ] Define interactive move/resize behavior or explicitly leave it to Iced
  decorations.
- [ ] Preserve Iced keyboard, pointer, touch, IME, clipboard, and drag/drop.
- [ ] Synchronize slot geometry before the relevant parent commit.
- [ ] Measure whether current redraw ordering is sufficient.
- [ ] If insufficient, document a minimal generic Iced use case before any
  upstream discussion; do not restore the persistent `wayland-hack` hook.

### W5 — HDR and robustness gate

**Status:** Deferred under D-022; retained as Wayland re-entry criteria.

- [ ] Verify `gpu-next` and expected hardware decoder.
- [ ] Verify HDR10/PQ and HLG color-description traffic on a capable compositor.
- [ ] Verify SDR Iced controls compose over HDR video without incorrect output
  labeling.
- [ ] Test Hyprland/wlroots, KDE, and GNOME where available.
- [ ] Test Intel, AMD, and NVIDIA proprietary drivers where available.
- [ ] Test pause, seek, resize, fractional scaling, fullscreen, minimize,
  suspend/resume, monitor removal, VO reload, and stop.
- [ ] Stress 100 load/stop and 100 fullscreen cycles.
- [ ] Verify clean fallback when optional protocols are absent.
- [ ] Verify no CPU frame path is used.

### Wayland decision

Recorded outcome:

- [-] **GO:** not selected; integrated mpv is not eligible for Wayland rollout.
- [x] **HYBRID:** GStreamer remains the integrated Wayland backend; mpv is used
  in native-window mode on Wayland and remains the integrated target on Windows
  and macOS.
- [-] **STOP:** not selected; the bridge criteria and research fixture are
  retained for reconsideration when a better path exists.

D-022 is a platform-specific release decision, not an abandonment of embedded
mpv elsewhere. Windows P5 and macOS P6 continue toward fully integrated
native-VO presentation. Reopening Wayland GO requires a new decision backed by
a safe connection bootstrap, W1–W5 evidence, and release packaging; a
single-compositor demonstration remains insufficient.

## 12. P8 — Playback Feature Parity

**Objective:** make the mpv backend replace current player behavior rather than
merely play a file.

### Core controls

- [x] play, pause, and toggle;
- [x] absolute and relative seek;
- [x] seek preview/drag throttling without flooding libmpv;
- [x] volume and mute;
- [x] playback speed;
- [x] contain/cover/fill mapping with documented mpv properties;
- [x] confirmed fullscreen state;
- [x] loading, buffering, seeking, and error UI; and
- [x] controls visibility without frame-driven redraw.

Desktop and 10-foot playback surfaces now derive static loading, buffering
(with bounded percentage), seeking, stopping, presenter-readiness, and
structured failure plates from `PlaybackSnapshot`/`PresenterState`. The
pre-session loading route and terminal error route remain shell-owned, while an
active backend no longer depends on adapter-specific UI state for those
transitions. Pure projection tests cover transient states, integrated presenter
readiness, native-window non-presentation, and structured-error fallback text;
the plates do not request animation or video-frame redraws.

The native-window content-fit implementation maps contain/cover/fill to
`keepaspect`, `video-unscaled`, and `panscan`; native-size and scale-down modes
are mapped at the same boundary. The exact table is now normative in the
specification, and pure mapping tests cover every mode. Seek preview dispatch
is limited to one command per 100 ms at the UI boundary. The mpv adapter also
allows only one absolute seek request in flight and replaces its single queued
position with the newest drag target; stop, replacement load, and shutdown
clear that queue so late replies cannot seek a new lifecycle. Deterministic
tests cover the UI interval, latest-value coalescing, and late-reply rejection.
`SeekTo` now submits an immediate absolute command rather than changing only
the drag preview, and keyboard/episode relative seeks remain signed
`PlaybackCommand::SeekRelative` operations after duration clamping. Explicit
pause intent now takes precedence over buffering and is not inferred from other
non-playing states, so toggle behavior remains correct during load/seek/cache
transitions. The real native-VO smoke confirms play, pause, and both seek forms.
Once an initial track catalog exists, backend-driven audio/subtitle selection changes
are now diffed during snapshot projection and use the same short-lived notice
as Iced-issued selections; initial discovery, duplicate confirmations, and
replacement-file loading remain quiet. A pure test covers simultaneous audio
change/subtitle disable, duplicate suppression, and initial-catalog
suppression.

Chapter and edition catalogs now retain Ferrex-owned stable identities plus the
currently observed selection in `PlaybackSnapshot`. Capability-gated settings
pickers submit backend-neutral `SelectChapter`/`SelectEdition` commands; the mpv
adapter maps those to the standard `chapter` and `edition` properties while
Subwave reports the unsupported capability explicitly. Those serialized
capability flags introduced diagnostic schema version 3; the config trust
policy below advanced it to version 4, local extension capabilities advanced it
to version 5, and effective log policy advances the current schema to version
6. Replacement loads clear old catalogs before
the next demuxer identities arrive. Reducer/parser tests
cover catalog normalization, chronological chapter presentation versus native
indices, selection observations, and mpv's single-default-edition case where
the scalar property is unavailable. On 2026-07-12 the display-backed mpv 0.41
multitrack smoke selected the second generated chapter and the generated default
edition through this path, in addition to its existing track/control lifecycle
checks.

### Tracks and media structure

- [x] stable audio track identities and selection;
- [x] subtitle off/on/selection and previous selection;
- [x] ASS, text, bitmap, and external subtitle coverage;
- [x] chapters;
- [x] editions;
- [ ] attached fonts; and
- [x] track-change notifications.

On 2026-07-12 the expanded display-backed smoke also passed against the
`ass-animation-fonts.mkv` and `pgs-bitmap.mkv` generated fixtures, including
track discovery/selection and the full shader/screenshot/control lifecycle. A
separate run loaded `sources/english.srt` beside the H.264 fixture through the
capability-gated `AddExternalSubtitle` command and confirmed a newly observed,
selected Ferrex-owned track with `is_external=true`. Together with the embedded
SRT tracks in the multitrack smoke, this closes native-VO ASS/text/PGS/external
load and selection coverage. Visual attached-font substitution correctness
remains open.

### Ferrex behavior

- [x] authenticated direct play;
- [x] server transcode URL playback, including protected HLS generation,
  publication, route authentication, source projection, and a display-backed
  native-mpv load;
- [x] quality-profile switch and credential-preserving stream reload;
- [x] resume position;
- [x] progress heartbeat;
- [x] final progress on all terminal paths;
- [x] next/previous/restart episode behavior;
- [x] navigation back/home while playing; and
- [x] restore main window state after playback.

The episode reducer preserves Internal, in-process mpv native-window, or
external-process mode across explicit next/previous and natural EOF
transitions, checkpoints progress before replacement, and applies the exact
five-percent Previous boundary (an unknown duration safely restarts). Final
progress, replacement/reset, and navigation messages now use serialized task
chains instead of parallel batches. Back and Home checkpoint and then enter the
common reset path. `ResetAfterStop` emits one backend-neutral host-exit event
after state reset; the UI window
controller's idempotent dismiss path closes an active dedicated overlay and
restores the retained main size, position, fullscreen mode, and focus. Pure
reducer tests cover all three backend modes, the restart boundary, Back/Home
ordering, and the host-exit event; window-controller tests prove exit dismissal
does not mutate player state.

### mpv compatibility surface

- [x] raw command UI/debug console or documented developer API;
- [x] arbitrary property observations for future features;
- [x] profile/config loading policy;
- [x] optional user scripts and input bindings;
- [x] user shader/profile passthrough;
- [x] screenshot behavior; and
- [x] capability diagnostics for unavailable options.

The Starlight
[`Native mpv extension API`](../src/content/docs/developer/native-mpv-extension-api.md)
now documents the public `MpvSession`/`MpvWorker` ownership models, arbitrary
string/node commands, typed and node properties, stable arbitrary observations,
hooks/client messages/events, reply correlation, redaction rules, and the final
unsafe raw-handle boundary. Feature code must keep user-facing cross-backend
behavior in the neutral contract; a user command console is not required for
this gate. Existing fake-ABI tests exercise every documented value format,
observation registration/removal, command form, cancellation, copied event
lifetime, and teardown.

`ferrex-player-mpv::MpvConfigPolicy` now makes configuration trust explicit.
The default deterministic profile disables standard user config, script
discovery, and external URL resolvers; native-window OSC and controlled input
bindings remain Ferrex-owned options. The developer-only
`FERREX_MPV_CONFIG_POLICY=trusted-user` opt-in enables standard mpv config,
`input.conf`, and scripts as trusted in-process code. Unknown or non-Unicode
values fail closed without being echoed into logs. Diagnostic schema version 4
introduced the effective policy and high-level switches without config contents
or paths; schema version 5 added extension capability booleans and only the
observed active shader count, while schema version 6 reports only the effective
native message level and whether the bounded startup capture is active.
Fake-ABI and pure parser/diagnostic tests verify both profiles, fail-closed
selection, and continued `ytdl=no`; the player README documents the trust
boundary and invocation. A normal settings control remains rollout UX work and
does not change Auto or fallback selection.

`PlaybackSession` now exposes capability-gated Ferrex commands for external
sidecar subtitles, named video profiles, an ordered local shader list, and
explicit-path screenshots. The mpv adapter maps them to argument-separated
`sub-add`, `apply-profile`, the `change-list` command for `glsl-shaders`, and
`screenshot-to-file`; named user profiles require the trusted-user policy.
Subwave reports `UnsupportedOperation` instead of a
no-op. Local paths and profile names have redacted `Debug` forms, invalid inputs
fail without echoing values, and diagnostics never include paths. Pure tests
cover every command/mode, policy-dependent capability reporting, unsupported
fallbacks, and redaction. On 2026-07-12 the display-backed mpv 0.41 smoke passed
again after applying and observing a temporary identity shader, writing a
non-empty screenshot, clearing the shader list, and removing both files.

### External player migration

- [x] Compare in-process native-window fallback with current external mpv
  behavior.
- [x] Decide whether external process mode remains for crash isolation.
- [x] If retained, adapt it to the same playback contract and redaction rules.
- [-] Removal is not selected during the rollback window or while D-023 needs
  an explicit X11 handoff.

D-009 retains external mpv as an explicit, process-isolated compatibility
handoff, never an Auto candidate. In-process native-window mpv provides the
full command/event/track surface and header-authenticated transport but shares
the Ferrex process; external mpv provides crash isolation and user-installed
X11 VO availability, while intentionally advertising only its observed
progress, seek, fullscreen, terminal, and native-window capabilities. D-017
keeps its credential-bearing URL out of argv and sends it through private IPC;
D-019 reduces copied IPC observations into the neutral snapshot used by
progress and episode policy. The same snapshot now also produces the redacted
backend/presentation/evidence summary used by diagnostics. Unit tests cover
snapshot lifecycle, heartbeat/final progress, episode transition after handle
teardown, and diagnostic projection; the real Linux smoke covers private IPC,
process observation, and argv non-disclosure. Reconsider removal in P11 after
the rollback window and only if X11 has another policy-approved handoff.

### Exit criteria

- [ ] Existing player integration tests pass against mpv where backend-neutral.
- [ ] Manual control/track/episode parity checklist passes.
- [ ] Unsupported mpv-native behavior is represented as a capability, not a
  hidden no-op.

## 13. P9 — Hardening, Performance, and Release Packaging

### Automated verification

- [x] Linux unit/integration suite with libmpv enabled.
- [~] Windows mpv/presenter build, focused tests, SDK staging, and closure audit
  are defined in CI; the first target run and display smoke remain open.
- [~] macOS mpv/AppKit build, focused tests, pinned core source builds,
  version/hash-recorded runtime inputs, bundle rewrite/sign/audit, and both
  architectures are defined in CI; the first target run and display smoke
  remain open.
- [x] Backend-disabled build remains valid during rollback window; Linux CI
  and both target distribution workflows check it explicitly.
- [x] Fake presenter and FFI tests run without a display.
- [x] Lifecycle stress test is runnable in CI or a documented compositor job.
- [x] Secret-redaction tests cover URL, cookies, and headers.

Linux CI now runs both the linked `ferrex-player-mpv` handle suite and the
`ferrex-player-playback --features mpv` contract/adapter suite, including fake
FFI/presenter lifecycle and source/log redaction coverage. It also performs an
explicit `ferrex-player --no-default-features` rollback build; the Windows and
macOS distribution workflows repeat that check for their target triples.
Display-backed
native-VO smoke tests remain explicitly ignored and are covered by the manual
fixture gate rather than silently using a software/headless VO in CI. The
ignored `linked_native_window_load_stop_lifecycle_stress` job now defaults to
100 fresh libmpv/native-window load, VO-ready, ordered-stop, and teardown
cycles; the fixture procedure documents the exact compositor command and
results location. On 2026-07-13 a full 100-cycle Wayland run with mpv 0.41.0
passed in 25.81 seconds under explicit 64 MiB RSS-growth and four-FD budgets:
process RSS moved from an 80,680 KiB post-first-cycle baseline to 96,192 KiB
with a 103,976 KiB peak, while open FDs remained four. This closes the generic
Linux native-window harness gate; separate GPU/native-resource review and the
Windows/macOS presenter-specific stress gates remain open.

### Performance

- [ ] Compare startup-to-first-frame against baseline.
- [ ] Compare seek latency.
- [ ] Compare CPU/GPU usage for SDR/HDR.
- [ ] Verify Iced does not redraw at video frame rate when controls are idle.
- [ ] Inspect frame-drop/timing diagnostics under 60/120/144 Hz UI settings.
- [ ] Verify overlay visibility does not cause an unacceptable HDR or latency
  regression.
- [ ] Confirm no readback/upload path in native-VO mode.

### Release artifacts

- [x] Nix package smoke test outside the development shell.
- [x] Flatpak bundle smoke test.
- [ ] Windows packaged install smoke test on a clean VM.
- [ ] macOS signed/bundled app smoke test on a clean machine.
- [ ] License and notices reviewed.
- [x] Upgrade/rollback behavior documented.

On 2026-07-13, `nix build path:.#ferrex-player` produced the wrapped player
outside the development shell. The source filter now explicitly excludes
ignored local Flatpak, target, cache, and direnv roots so working-tree package
smokes cannot ingest unrelated vendored Cargo manifests. A clean-environment
launch with a temporary home completed `ferrex-player screenshot --help`; the
packaged ELF directly requires `libmpv.so.2`, its loader metadata contains no
build/developer path, and no developer home path remains in the binary. Nix
store references are expected and resolve through the pinned LGPL closure.

The Flatpak release smoke is a real bundle install, not only a build-directory
check. It verifies the executable and pinned libmpv/FFmpeg/libplacebo closure,
the LGPL build-profile records, and then removes the test installation. The
same loader/profile smoke now runs in the Flatpak workflow before artifact
upload; display-backed playback remains part of the separate manual fixture
gate.

### Documentation

- [x] Update root README platform table based on measured capabilities.
- [x] Update `ferrex-player/README.md` prerequisites and diagnostics.
- [x] Update `docs/architecture.md` diagram.
- [x] Add mpv configuration and troubleshooting documentation.
- [x] Document backend selector and fallback order.
- [x] Document how to collect mpv and presenter diagnostics safely.

The canonical Starlight architecture now shows the neutral session/reducer and
three concrete adapter/presentation paths. The new Desktop playback backends
guide documents build-time feature selection, current per-platform Auto
policy, deterministic exact-request fallback, trusted config, fixed native log
filters, evidence-qualified in-player diagnostics, authentication/package
troubleshooting, and the GStreamer/external rollback boundary. The player README
and operator configuration page link the same policy and commands; the legacy
`docs/architecture.md` remains a pointer to the canonical page.

### Exit criteria

- [ ] Every platform proposed for rollout passes its specification gate.
- [ ] Release packages work without developer-only paths.
- [ ] Performance does not regress beyond an explicitly accepted budget.

## 14. P10 — Staged Default Rollout

### Stage A — Developer-only

- [ ] Backend available only through an explicit developer setting.
- [ ] Structured diagnostics are collected in issue reports.
- [ ] GStreamer remains default everywhere.

### Stage B — User opt-in

- [ ] Document experimental mpv integrated/native-window choices.
- [ ] Add visible fallback reason when integration fails.
- [ ] Collect a minimum soak period and issue inventory.
- [ ] Retain one-click/config rollback to GStreamer.

### Stage C — Per-platform Auto

For each platform independently:

- [ ] platform acceptance gate signed off;
- [ ] release artifact verified;
- [ ] fallback verified;
- [ ] known limitations documented;
- [ ] Auto switched to mpv in one focused change; and
- [ ] release notes identify rollback setting.

Under D-022, Wayland Auto remains on the integrated GStreamer path and an mpv
selection uses native-window presentation. Under D-023, X11 Auto also remains
integrated GStreamer, while the LGPL-only in-process mpv backend reports X11
presentation unavailable and the external process remains an explicit handoff.
Windows and macOS retain independent per-platform Auto gates for their fully
integrated mpv presenters. Reopening either Linux HYBRID decision requires its
recorded re-entry evidence and a new decision.

### Stage D — Primary backend

- [ ] mpv is Auto on every platform approved by its gate.
- [ ] At least one release cycle retains and exercises GStreamer rollback.
- [ ] Crash/error/fallback reports are reviewed before cleanup.

## 15. P11 — Legacy Cleanup and Optional Upstream Work

### Player cleanup

- [ ] Remove obsolete `video_opt` compatibility fields and duplicated state.
- [ ] Remove obsolete external-mpv messages if external mode is retired.
- [ ] Remove backend-specific UI branches superseded by capabilities.
- [x] Remove filename-based HDR provider selection.
- [x] Remove frame-driven progress polling.

Provider selection no longer constructs an HDR hint from `2160p`, `UHD`,
`HDR`, or `DV` filename fragments. Player content labeling uses only server or
decoder color/bit-depth metadata, while native HDR output remains a separate
observed diagnostic. A pure metadata test covers PQ, HLG, BT.2020, 10-bit, and
8-bit SDR without a filename input. Snapshot synchronization is driven by the
bounded controls timer for Subwave and the coalesced copied-event signal for
mpv; views register no decoded-frame progress callback.

### GStreamer/Subwave cleanup

Only after rollout and rollback criteria:

- [ ] Confirm no server/media-analysis use depends on playback GStreamer
  packages.
- [ ] Remove unused appsink playback path.
- [ ] Remove unused Wayland playback surface code if mpv replaced it.
- [ ] Remove the GStreamer development-version pin from player packaging when
  no remaining feature requires it.
- [ ] Preserve Subwave as a separate backend only if it has a documented,
  tested capability.

### Iced fork cleanup

- [ ] Remove the playback-specific Wayland integration hook when no longer used.
- [ ] Keep batching/performance changes separate from media integration.
- [ ] Re-evaluate whether Ferrex can track upstream Iced more directly.
- [ ] Do not combine Iced cleanup with the mpv default-switch change.

### Optional upstream proposal

- [ ] Collect at least two non-media use cases for foreign parent-window
  support.
- [ ] Open an Iced Discourse design discussion before writing a PR.
- [ ] Keep the proposed change backend-generic and map to winit semantics.
- [ ] Submit only after maintainer alignment.
- [ ] Do not include mpv, Wayland protocol objects, HDR policy, or persistent
  pre-commit callbacks.

### Exit criteria

- [ ] No obsolete playback dependency remains in release packages.
- [ ] Architecture and platform docs match the shipped implementation.
- [ ] Rollback history and removed capability decisions are recorded.

## 16. Cross-cutting Workstreams

### 16.1 Diagnostics

- [x] Define a serializable playback diagnostic snapshot.
- [x] Add backend/presenter lifecycle state.
- [x] Add versions and runtime capability list.
- [x] Add VO, GPU context, adapter, hwdec, and color parameters.
- [x] Add geometry and display scale.
- [x] Add fallback chain and reason.
- [x] Add opt-in verbose mpv and Wayland bridge traces.

`FERREX_MPV_LOG_LEVEL` accepts only the fixed levels `none`, `fatal`, `error`,
`warn`, `info`, `verbose`, `debug`, and `trace`; invalid/non-Unicode values fail
closed to the bounded startup-verbose/steady-info policy and are never echoed.
Copied mpv messages still pass through both wrapper and active-source
redaction, and trace-severity messages remain trace-severity application logs.
Diagnostic schema version 6 reports the effective filter and bounded-startup
flag, never message contents. The deferred Wayland bridge has no runtime to
instrument under D-022; its opt-in W0 protocol harness already writes redacted,
mode-private traces through `native_playback_wayland_trace.py` and remains the
required re-entry diagnostic.

### 16.2 Secret handling

- [x] Introduce a redacted playback source debug representation.
- [x] Prefer HTTP headers/cookies over query-token URLs where server API permits.
  Direct in-process streams and the typed `StreamingPlaybackSource` used by the
  streaming/HLS service now carry a zeroizing bearer header on a
  credential-free URI. Invalid/injectable values fail closed and `Debug`
  redacts both path and authorization. Only the explicit external-process
  compatibility boundary reconstructs a temporary query-ticket URL.
- [x] Ensure mpv logs are filtered before entering normal application logs.
  Provider startup logging also uses `PlaybackSource`'s redacted formatter and
  never emits the raw path, query, userinfo, headers, or cookies.
- [x] Ensure errors and panic diagnostics do not expose authorization data.
- [x] Remove URL-bearing external process arguments from the retained legacy
  mode by submitting its media URL over the private mpv IPC socket.

### 16.3 Capability UX

- [x] Show selected backend and presentation mode in diagnostics/settings.
- [x] Explain why integrated presentation is unavailable.
- [x] Distinguish native HDR support from detected HDR content.
- [x] Distinguish expected/observed hardware decoding.
- [x] Never label a path zero-copy without observed evidence.

The in-player settings panel now projects a redacted, evidence-qualified summary
from `PlaybackDiagnosticSnapshot`: requested and selected backend/presentation,
integrated-presenter status and fallback detail, input HDR metadata separately
from native-output HDR evidence, and configured hardware-decoder policy
separately from the observed decoder. Retained external-process snapshots now
produce the same summary after process exit. Pure projection tests cover the
labels and explicitly reject unobserved zero-copy wording; no URI, header,
cookie, local path, or configuration path enters the summary.

## 17. Risk Register

| ID | Risk | Impact | Mitigation / decision trigger | Status |
|---|---|---|---|---|
| R1 | mpv cannot be safely proxied onto Iced's Wayland connection | Blocks integrated mpv on Wayland | D-022 selects GStreamer integration plus mpv native-window rather than a framework/environment hack; reopen only for a maintainable per-session path | Mitigated by HYBRID; research deferred |
| R2 | Proxy misses evolving Wayland protocols used by mpv/driver | Playback/HDR failures by compositor or version | Retain the pinned trace fixture and W1–W5 re-entry matrix; no bridge ships under D-022 | Deferred under HYBRID |
| R3 | Transparent overlay breaks HDR, independent flip, or latency | Quality/performance regression | Measure overlay shown/hidden; native-window fallback; per-platform rollout | Open |
| R4 | macOS child window fails fullscreen/Spaces behavior | No integrated macOS controls | P6 spike; retain native-window mode; do not force OpenGL | Open |
| R5 | libmpv/AppKit/event-loop threading deadlocks | Application hang on load/exit | Serialized owner, callback rules, lifecycle stress, main-loop-aware teardown | Open |
| R6 | Native resource teardown races host window destruction | Crashes/leaks | Explicit generations, close ordering, 100-cycle tests | Open |
| R7 | Packaging differs from developer environment | Backend absent in releases | P2 packaging workstream and clean-machine smoke tests | Open |
| R8 | mpv config/scripts make behavior nondeterministic or unsafe | Support/security problems | Deterministic profile; explicit trusted-user config/scripts opt-in with diagnostics | Mitigated; settings UX pending |
| R9 | Player abstraction refactor changes progress/episode behavior | User-visible regressions | Adapter-first P1 with behavior tests before mpv | Open |
| R10 | Scope expands into a private graphics/render backend | Long-term maintenance burden | Native-VO invariant; private mpv patch requires spec amendment | Open |
| R11 | User input cannot reveal hidden Iced overlay reliably | Broken controls/focus | Explicit per-platform input policy and tests | Open |
| R12 | mpv API/version churn breaks wrapper | Build/runtime incompatibility | Bundle known version, runtime checks, raw API tests, update policy | Open |
| R13 | A distribution links Ferrex to mpv/FFmpeg built with GPL-only code | Release license incompatibility | Require per-platform LGPL build profiles and notices; Nix and Flatpak assert mpv's resolved `gpl=false` option plus FFmpeg's LGPL/no-`--enable-gpl` configuration | Mitigated on Nix/Flatpak; open elsewhere |
| R14 | mpv 0.41's X11 VO and `wid` implementation require its GPL build profile | Blocks bundled in-process mpv on X11 under D-005 | D-023 keeps X11 on integrated GStreamer and permits the external process boundary; reopen only for compatibly licensed upstream code or an explicit distribution-policy amendment | Mitigated by HYBRID; packaged fallback gate open |
| R15 | Server-side FFmpeg jobs can exhaust resources or expose incomplete/stale renditions | Availability or protected-stream integrity regression | Bound concurrency/timeouts/retention, write into per-job staging, validate output, atomically publish, authenticate ownership and every asset, and keep transport-only fixtures independently tested | Mitigated; live generated-rendition load passes, operational soak open |

## 18. Decision Log

| ID | Decision | State | Date | Notes |
|---|---|---|---|---|
| D-001 | Target libmpv native VO instead of forcing frames through wgpu | Accepted | 2026-07-11 | Maximizes current mpv VO/hwdec/HDR support |
| D-002 | Keep GStreamer as migration/failure fallback | Accepted | 2026-07-11 | Removal requires per-platform gates and rollback release |
| D-003 | Make the first integration without mpv-specific Iced changes | Accepted | 2026-07-11 | Use raw handles, `window::run`, redraw events, and widget tree state; the pinned Iced revision has no `Shell::window` host accessor. |
| D-004 | Use a thin Ferrex wrapper and fakeable function table over `libmpv2-sys` 4.0.1 | Accepted | 2026-07-11 | Full client API 2.5 coverage without adopting `libmpv2` ownership/event limitations. Isolated in `ferrex-player-mpv`; `linked` is opt-in and propagated by the player-level `mpv` feature; LGPL exception is explicit in `deny.toml`. Fake lifecycle/version tests and the Nix-linked create/initialize/destroy smoke test pass. Fallback impact: backend-disabled builds remain valid. |
| D-005 | Dynamically link a known LGPL-only mpv 0.41.0 shared library; minimum client API 2.2 | Accepted | 2026-07-11 | API 2.2 (mpv 0.37) contains every P3 client symbol; production remains pinned to mpv 0.41/API 2.5. The Nix profile uses `gpl=false`, LGPL FFmpeg, disabled GPL-only inputs, and an install-time feature/license assertion; its player package and real-handle smoke test pass. Flatpak/Windows/macOS must reproduce the LGPL profile and notices, not use default GPL builds. A bespoke runtime loader is deferred, not prohibited. Fallback impact: builds without `mpv`/`linked` retain GStreamer/external mpv. |
| D-006 | Non-Wayland native-root plus transparent Iced overlay | Accepted for handoff | 2026-07-13 | Windows and macOS now implement independently compile-gated native-root presenters behind the neutral lifecycle. Representative hardware approval remains per-platform in D-024/D-025; Auto is unchanged. |
| D-007 | Wayland Iced root plus proxied mpv subsurface | Deferred | 2026-07-12 | D-022 selects HYBRID. Retain this architecture only as re-entry criteria if a safe connection bootstrap becomes available. |
| D-008 | Wayland-only mpv connection redirection mechanism | Deferred | 2026-07-12 | W0 confirms stable libmpv has no per-context Wayland endpoint and delayed helper connections make temporary environment overrides unsafe. A process-lifetime startup proxy is the only race-free candidate found, but routing Iced too conflicts with the private mpv-only socket requirement. D-022 selects HYBRID until a better path exists. |
| D-009 | Retain external mpv as an explicit process-isolated compatibility handoff through the rollback window | Accepted | 2026-07-13 | In-process native-window mpv now has control/track/progress parity for the supported matrix, but the external process still supplies crash isolation and the D-023 X11 handoff without linking GPL-only X11 VO code into Ferrex. D-017 sends its credential-bearing source through private IPC rather than argv; D-019 projects copied state into `PlaybackSnapshot`, and the capability-UX tranche adds the same redacted diagnostic summary. Pure tests cover lifecycle/progress/episode/diagnostic behavior; the ignored real-mpv smoke verifies IPC load, observation, private socket cleanup, and `/proc/<pid>/cmdline` non-disclosure. Fallback impact: external mpv remains explicit and is never selected by Auto; GStreamer and in-process native-window selection are unchanged. Revisit removal only in P11 after the rollback window and an approved X11 alternative. |
| D-010 | Optional Iced foreign-parent proposal | Deferred | 2026-07-11 | Discuss only after working external implementation |
| D-011 | Keep the neutral contract in `ferrex-player-playback::contract` | Accepted | 2026-07-11 | `dev` already extracted the playback crate; contract/reducer/channel/fallback and adapter tests live there, with no second crate until another client needs it. Evidence: playback and UI unit suites plus workspace all-target check in the initial implementation change |
| D-012 | Serialize libmpv through a thread-affine `MpvSession`, with an optional owner-thread `MpvWorker` | Accepted | 2026-07-11 | The wakeup callback performs only atomic coalescing and `Thread::unpark`; native pointers stay on the owner, and event payloads are bounded/copied before the next wait. Fake ABI tests cover nodes, replies, cancellation, hooks, logs, wake storms, and 50 teardown cycles; linked tests cover a real property reply and ordered stop. macOS can use the local owner until its AppKit model is proven. Fallback impact: none when the `mpv` feature is disabled. |
| D-013 | Route the existing explicit “Play in MPV” action to in-process native-window libmpv when the `mpv` feature is enabled | Accepted | 2026-07-11 | `mpv_adapter.rs`, `video::open_requested_session`, and a coalesced copied-event readiness subscription provide the vertical slice without frame uploads or periodic event polling. Auto remains Subwave; async mpv load failure resumes through Auto/Subwave, the separate external-process handoff remains available, and backend-disabled builds retain the historical external action. Unit tests cover source/log redaction, property/event/track mapping, close-versus-EOF terminal policy, version/VO/GPU/hwdec/frame diagnostic serialization, and load/seek/EOF stop order; the expanded local real-VO smoke test passes with mpv 0.41.0 and verifies public runtime diagnostics, confirmed fullscreen enter/exit, stop/reload, and native close/quit, while the earlier authenticated HTTP range variant separately proves header/cookie/query-ticket transport. |
| D-014 | Keep native presenter resources UI-thread-local behind a generation-scoped lifecycle reducer | Accepted | 2026-07-11 | `ferrex-player-playback::presenter` owns pure readiness, geometry, visibility, suspension, fullscreen-confirmation, teardown, and failure transitions while `NativePresenter` deliberately has no `Send` bound. Accepted/cleared geometry now crosses the neutral presenter-event boundary into diagnostic schema v2, including logical/visible bounds and display scale; fallback requests form an ordered deduplicated chain. Fake lifecycle/reducer tests cover both readiness orders, one attach per generation, clipping/zero size, duplicate geometry, scale/window recreation, stale rejection, explicit detach-before-drop, deterministic fallback, geometry clearing, and fallback history. Fallback impact: presenter failure emits `PresenterFailed` from integrated mpv to native-window mpv after detach; no platform presenter or Auto default is enabled yet. |
| D-015 | Acquire Iced native hosts with a `window::run` handshake and event-loop-local lease | Accepted | 2026-07-11 | The pinned Iced API does not expose a window through widget `Shell`. `native_video_slot.rs` therefore requests capture once, returns only a pointer-free result through `Task`, and keeps copied raw handles in a thread-local borrow registry used by generation-scoped presenter callbacks. `Tree::State` revisions geometry only on redraw and detaches on replacement/drop/close. Unit tests cover host capture, clipping/scale revisions, capture deferral, and teardown. Fallback impact: none; no integrated presenter or Auto selection is enabled. |
| D-016 | Decouple player snapshot updates from decoded-frame callbacks | Accepted | 2026-07-12 | Desktop and 10-foot views construct the backend presentation widget without `on_new_frame`. Native mpv wakes Iced only through its coalesced copied-event signal; legacy Subwave synchronization reuses the bounded controls timer, and progress persistence keeps its existing ten-second heartbeat. The player/UI feature suite and backend-disabled check pass. Fallback impact: Subwave remains fully available, but its UI position refresh is intentionally bounded instead of frame-rate-driven. |
| D-017 | Carry in-process playback tickets in typed source headers | Accepted | 2026-07-13 | `resolve_playback_stream_source`, `PlayerDomainState::current_source`, and `SetStreamSource` keep direct Ferrex stream URIs credential-free and store the playback-scoped token in a redacted, zeroizing `Authorization` header. The formerly string-only streaming/HLS service now returns a typed `StreamingPlaybackSource` with the same constraints and projects it into `PlaybackSource`; embedded query/userinfo credentials and header injection fail closed. Subwave and libmpv pass source headers in process; the legacy external path creates a temporary zeroizing query URL only at its compatibility boundary and sends it through private mpv IPC rather than the child argument vector. Unit tests verify direct and streaming-service header transport, injection rejection, redacted source/state/error diagnostics, fail-closed ticket errors, legacy conversion, and `0700` IPC-directory cleanup. Real native-mpv smokes pass against a bearer-header range server, a real Ferrex router direct stream, and a router-backed HLS manifest whose four segments require the same ticket; the separate external-mpv smoke verifies query-ticket IPC load plus `/proc/<pid>/cmdline` non-disclosure. Fallback impact: in-process mpv-to-Subwave fallback retains the same authenticated source; only the explicit external compatibility handoff reconstructs a query ticket. |
| D-018 | Allocate the dedicated Iced player overlay hidden and reveal it only after explicit native attachment | Accepted | 2026-07-12 | `WindowKind::PlayerOverlay`, the daemon window controller, player-only root view routing, and the explicit transparent theme implement a generation-independent host window shell without platform objects. Attachment and presenter positioning occur while hidden; an explicit `Activating` state covers the serialized retained-main hide, and `Active` is recorded only after the presenter synchronously reveals the host. Pointer-free transition logs then confirm the delivered overlay-focus event. No stale main resize/move is applied after attachment. A separate live overlay viewport drives controls/focus/hit testing while retained main geometry remains restoration state. Close handling calls `prepare_iced_native_host_close` before `window::close`, detaching all registered slot generations before releasing the raw-host lease; the retained main window is restored after activating/active-overlay teardown. Manager, settings, theme, viewport, and native-slot tests cover hidden/activating/active/closing order and detach-before-release. P5/P6 still own native root relationship, z-order, and taskbar identity validation. Fallback impact: the overlay is dormant until an attachment confirmation explicitly activates it, and presenter fallback can dismiss it without stopping playback; current Subwave, mpv native-window, and external modes are unchanged. |
| D-019 | Project retained external-mpv process observations into the backend-neutral snapshot | Accepted | 2026-07-12 | `PlayerDomainState::external_mpv_snapshot` is reduced from copied private-IPC observations while `ExternalMpvHandle` owns only process resources. Desktop/10-foot views, progress heartbeat, navigation, and episode start-mode policy now consume the same snapshot/progress projection as in-process backends; only process polling and external seek remain explicit compatibility branches. Final observations are captured before handle drop, and tests prove terminal episode advancement and progress persistence without a surviving handle. Fallback impact: external process mode remains available and D-009 remains pending; Subwave and in-process mpv selection are unchanged. |
| D-020 | Keep mpv user config and scripts disabled unless trusted-code policy is explicit | Accepted | 2026-07-12 | `ferrex-player-mpv::MpvConfigPolicy` defaults to Ferrex's deterministic native-window profile. `FERREX_MPV_CONFIG_POLICY=trusted-user` is the developer-only opt-in for standard mpv config, `input.conf`, and scripts; invalid values fail closed and are not logged. Diagnostic schema v4 reports policy and effective high-level switches. Fake session tests verify pre-initialization config/script options and retained external-resolver disablement; playback parser/diagnostic tests and backend-disabled compilation pass. Fallback impact: none—Auto remains Subwave, exact mpv selection is unchanged, and builds without mpv do not read the policy. |
| D-021 | Expose local video extensions as capability-gated Ferrex commands | Accepted | 2026-07-12 | `PlaybackCommand`/`PlaybackSession` model external sidecar subtitle loading, named profile application, ordered local shader replacement, and explicit-path screenshots without exposing the mpv owner. `PlaybackFilePath` and `VideoProfileName` redact debug output; adapter validation and copied-log filtering never echo values. mpv uses argument-separated standard commands, while Subwave returns structured `UnsupportedOperation`; user profiles are available only under D-020's trusted policy. Diagnostic schema v5 adds the four support booleans and only an observed shader count. Pure mapping/redaction/policy tests pass; display-backed mpv 0.41 native-VO smokes passed with a real external SRT track, identity shader, and non-empty screenshot on 2026-07-12. Fallback impact: unsupported backends remain selected and report the unavailable operation rather than changing backend or silently doing nothing. |
| D-022 | Use a HYBRID Wayland backend until a safe integrated mpv connection path exists | Accepted | 2026-07-12 | W0 traces on SDR, HDR10/PQ, and HLG prove ordinary mpv 0.41 `gpu-next`/Vulkan native VO and identify one shell candidate, but stable libmpv cannot direct a session to the private bridge without a process-global race; the only race-free startup proxy candidate would also proxy Iced and violates the current boundary. Wayland therefore keeps GStreamer/Subwave for integrated presentation and offers mpv in native-window mode. Windows P5 and macOS P6 remain fully integrated native-VO targets. Fallback impact: no Wayland Auto change, no CPU/wgpu mpv frame path, and no change to Windows/macOS rollout gates. |
| D-023 | Keep X11 on integrated GStreamer under the LGPL release profile | Accepted | 2026-07-12 | mpv 0.41's Meson graph requires `gpl=true` for the X11 VO and therefore for native-window/overlay/`wid` presentation. The reviewed D-005 profile cannot ship that code. The Flatpak build asserts `gpl=false` and `x11=disabled`, retains Wayland Vulkan/dmabuf/VA-API, and its built/installed bundle resolves the pinned libmpv/FFmpeg/libplacebo closure from `/app`; Nix uses the same mpv license option. Both package builds compile `FERREX_MPV_X11=disabled`; `open_requested_session` preflights an X11-only environment into a structured `UnsupportedPlatform` fallback before creating libmpv, with a pure display/profile matrix test. Re-entry requires compatibly licensed upstream X11 support or an explicit distribution-policy/specification amendment, followed by the retained P5 matrix. Fallback impact: X11 Auto remains integrated GStreamer, in-process mpv is reported unavailable, and the separate external mpv process may remain an explicit handoff; Windows/macOS gates are unchanged. |
| D-024 | Use mpv's Win32 HWND as the video root and an owned, taskbar-suppressed Iced HWND as the controls overlay | Accepted for handoff | 2026-07-13 | The compile-time `FERREX_MPV_WINDOWS_PRESENTER=spike` path observes the full pointer-width `window-id`, validates both HWNDs, synchronizes client geometry/DPI/minimize/visibility at an independent native-root cadence, delegates fullscreen to mpv, and restores owner/style state on detach. Display-free presenter tests, pinned LGPL libmpv SDK/import-library tooling, exhaustive provenance for the staged runtime DLL closure, and a reviewed GStreamer PE/GIO/TLS closure with HLS/HTTPS smoke are defined; floating Rust/MSYS2 build tools remain identified in workflow logs rather than covered by that runtime-closure claim. Fallback impact: any preflight/attach failure dismisses the hidden overlay and selects mpv native-window; Auto, native HDR, and production capability remain closed until the Windows hardware/package/100-cycle matrix passes. |
| D-025 | Use mpv's AppKit NSWindow as the video root and an AppKit child Iced NSWindow as the controls overlay | Accepted for handoff | 2026-07-13 | The compile-time `FERREX_MPV_MACOS_PRESENTER=spike` path treats `window-id` only as mpv's read-only NSWindow observation, retains and manipulates AppKit objects behind a non-Send main-thread marker, follows the content layout/active Space/occlusion/scale state, and detaches before handing blocking libmpv termination to an off-main reaper. Display-free presenter tests, pinned core LGPL sources, version/hash-recorded Homebrew inputs, and a strict macOS 15+ bundle closure/HLS audit are defined; the first target artifact run remains open. Fallback impact: any preflight/attach failure dismisses the hidden overlay and selects mpv native-window; Auto, EDR/HDR, VideoToolbox, and production capability remain closed until Apple Silicon/Intel package/fullscreen/Spaces/100-cycle evidence passes. |

When resolving a pending/proposed decision, add the implementation reference,
test evidence, and fallback impact to its Notes field.

## 19. Definition of Done

The migration is complete when:

- [ ] Ferrex-owned playback commands/events/snapshots are the only player-domain
  backend contract.
- [ ] libmpv native-window mode is complete and release-packaged on all desktop
  targets.
- [ ] each platform has either a passing integrated presenter or a documented
  native-window fallback.
- [x] Wayland has a recorded HYBRID decision backed by P7 W0 evidence.
- [ ] mpv feature parity covers current controls, tracks, subtitles, progress,
  episodes, direct play, and transcoding output.
- [ ] no native-VO path transfers decoded frames through CPU/wgpu.
- [ ] HDR and hardware-decoding claims are backed by diagnostics and manual test
  evidence.
- [ ] lifecycle and 100-cycle stress gates pass.
- [ ] Nix, Flatpak, Windows, and macOS release artifacts are verified.
- [ ] fallback and rollback paths are documented and tested.
- [ ] platform Auto defaults match recorded rollout decisions.
- [ ] obsolete GStreamer/external/fork code is removed only after the rollback
  window, or retained with a documented capability.
- [ ] README, architecture, configuration, and troubleshooting docs describe the
  shipped system.

## 20. Immediate Next Actions

1. [x] Complete the P0 direct-use and behavior inventory.
2. [x] Draft the P1 Ferrex-owned playback contract and fake backend tests.
3. [x] Run the P2 FFI comparison and record D-004/D-005.
4. [x] Build the P3 native-window vertical slice before any graphics embedding
   work.
5. [x] In parallel, assemble the Wayland protocol/media test matrix needed for
   the P7 W1 feasibility gate.
6. [x] Make episode/navigation exit policy and local mpv extensions explicit,
   capability-gated, redacted, and covered by display-free tests.
7. [~] Complete the remaining UI quality-picker/episode gate against a live
   local Ferrex server. Authenticated direct play, protected fixture HLS, and a
   real server-generated `360p` rendition all pass display-backed through the
   network-bound router with header-only tickets, including cache reuse,
   shader, screenshot, seek, and ordered-stop coverage. The manual quality
   selection run and UI episode transition remain open.
8. [x] Record the P7 HYBRID decision without changing Wayland Auto.
9. [x] Implement compile-gated Windows P5 and macOS P6 native presenter handoff
   builds independently of the deferred Wayland bridge.
10. [~] Run the documented Windows/macOS representative-system package,
    fullscreen/focus/scale/HDR/hwdec, fallback, and 100-cycle matrices before
    changing either platform's production or Auto capability.
