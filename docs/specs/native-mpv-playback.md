# Native mpv Playback Integration

- **Status:** Accepted; implementation in progress
- **Scope:** `ferrex-player` desktop playback
- **Last updated:** 2026-07-24
- **Tracking plan:** [Native mpv Playback Migration Plan](../plans/native-mpv-playback-migration.md)

## 1. Purpose

This specification defines the target architecture for making mpv the primary
playback engine in Ferrex while keeping the Iced user experience native and
integrated. It is the design reference for implementation, review, testing, and
the staged migration away from GStreamer as the default player backend.

The central decision is to use libmpv as the playback control plane while mpv
retains ownership of decoding and native video presentation. Video frames do
not normally pass through Iced or wgpu. Iced owns application layout, controls,
input, and non-video UI.

This document is normative for the target design. The accompanying plan tracks
incremental delivery and may change sequencing without changing the
architecture defined here.

## 2. Decision Summary

1. **mpv is the target primary desktop playback backend.** GStreamer remains a
   supported migration and failure fallback until the mpv path satisfies the
   acceptance gates in this specification.
2. **Use libmpv without `mpv_render_context` for the primary path.** mpv uses its
   normal native VO, preferably `gpu-next`, and owns frame timing, hardware
   decoding, subtitles, color conversion, swapchain negotiation, and HDR
   signaling.
3. **Treat presentation as a platform capability.** A platform presenter joins
   mpv's native surface/window with the Iced player UI without copying decoded
   frames into wgpu.
4. **Keep the control API open-ended.** Typed Ferrex operations are conveniences
   over mpv commands and properties, not a replacement for them.
5. **Do not add mpv-specific APIs to Iced.** The first implementation uses
   current generic Iced raw-window access and custom-widget lifecycle. Any
   upstream Iced proposal must be independently useful and discussed upstream
   before code is submitted.
6. **Wayland currently uses a HYBRID backend policy.** W0 found no safe way for
   stable in-process libmpv to direct only its delayed VO/driver connections to
   a private bridge without a process-global race. GStreamer therefore remains
   the integrated Wayland backend and mpv uses ordinary native-window
   presentation until a maintainable path exists.
7. **Windows and macOS remain full integration targets.** The Wayland decision
   does not reduce their requirement for an embedded Ferrex player experience
   using mpv's native VO and platform presenter without routing decoded frames
   through wgpu.
8. **A normal mpv window is always an acceptable fallback.** Integration
   failure must not force a CPU-copy or SDR path when native mpv presentation
   remains available.

## 3. Context

### 3.1 Current playback paths

At the migration baseline, the extracted `ferrex-player-playback` crate stored
`SubwaveVideo` directly in `PlayerDomainState::video_opt` and called backend
methods from playback update and view code. Subwave supplies two materially
different paths:

- a Wayland GStreamer sink rendered through custom subsurfaces, including the
  current zero-copy/HDR effort; and
- an appsink path that uploads decoded image data into a custom Iced/wgpu
  primitive and is effectively the cross-platform fallback.

Ferrex also has
`crates/ferrex-player-playback/src/external_mpv.rs`, which starts an external
mpv process and uses JSON IPC for progress and limited lifecycle control. It is
a handoff rather than an integrated player.

The current arrangement has important strengths: the Wayland sink can accept a
host-provided display and surface, and that path already demonstrates HDR
playback in Ferrex's primary environment. It also has structural costs:

- behavior differs substantially between Wayland and other platforms;
- the Wayland path depends on a development GStreamer series and a narrow known
  working version;
- appsink cannot expose the full native HDR and presentation behavior;
- Ferrex owns subtitle, color, sink, and pipeline behavior that mpv already
  implements across platforms; and
- the Iced fork contains platform-specific surface hooks that are difficult to
  propose upstream.

### 3.2 Why not render libmpv into wgpu now

The stable libmpv render API primarily exposes OpenGL and software rendering.
A seamless OpenGL implementation is possible when a GUI toolkit owns and
exposes the OpenGL context, as demonstrated by Switchfin. That does not map
cleanly to Iced's portable wgpu renderer:

- wgpu intentionally hides the native graphics context and swapchain;
- `wgpu-hal` interop is unsafe, backend-specific, and not a stable application
  contract;
- Windows would require D3D/Vulkan/DX interop or a private mpv render backend;
- macOS OpenGL is deprecated and is not the desired HDR path;
- software rendering introduces a full CPU path; and
- the host becomes responsible for HDR target selection, metadata, frame
  timing, and synchronization.

Relevant upstream mpv efforts are still open as of the date above, including
issues `#6575` and `#11031` and pull requests `#16818` and `#17828`. The target
architecture therefore allows a future render-API presenter, but does not make
unmerged work a production dependency.

## 4. Goals

The implementation MUST:

- preserve mpv's broad demuxer, decoder, subtitle, audio, filter, script, and
  protocol compatibility;
- permit mpv's normal hardware-decoding and native presentation paths;
- preserve native HDR and color-management behavior where mpv and the platform
  support it;
- present Iced controls as part of one coherent player experience;
- support Wayland, X11, Windows, and macOS with explicit capability reporting;
- expose arbitrary mpv commands, options, properties, observations, events,
  and node values in addition to typed Ferrex conveniences;
- avoid per-frame CPU readback or upload in the primary path;
- let mpv render independently of Iced's redraw cadence;
- fail deterministically to a documented fallback;
- keep watch progress, episode navigation, stream selection, and Ferrex server
  behavior independent of the selected playback backend;
- keep unsafe native-window and Wayland protocol code outside Iced; and
- support incremental rollout without changing the current default until its
  replacement passes platform gates.

The implementation SHOULD:

- bundle a known compatible libmpv build in release artifacts;
- allow an opt-in user mpv configuration while retaining deterministic Ferrex
  defaults;
- make presentation mode and capabilities visible in diagnostics;
- support a normal mpv-native window mode for compatibility and debugging; and
- isolate presenter failures from the player domain state machine.

## 5. Non-goals

The initial migration does not attempt to:

- make native video behave like an arbitrary Iced texture under transforms,
  rounded clipping, scrolling, or nested opacity;
- guarantee zero-copy for every codec, format, driver, or hardware decoder;
- implement a new stable Vulkan, D3D, Metal, or libplacebo render API for mpv;
- maintain a permanent private mpv graphics backend;
- make Iced itself understand mpv, GStreamer, HDR metadata, or Wayland
  subsurfaces;
- remove GStreamer before the Wayland and release-packaging gates pass;
- change server-side transcoding or media analysis solely because the desktop
  playback backend changes;
- cover Android, iOS, or console playback in the first implementation; or
- promise that every mpv script that assumes direct ownership of native input
  will work unchanged in integrated-Iced mode.

## 6. Terminology

- **Control plane:** libmpv commands, options, properties, observations, and
  events.
- **Native VO:** mpv's normal video output path, such as `gpu-next`, creating or
  using a platform-native presentation surface.
- **Presenter:** UI-thread platform code that joins the mpv native output with
  an Iced-owned player surface/window and synchronizes geometry and lifecycle.
- **Integrated mode:** Iced controls and input are visually integrated with the
  native mpv video surface.
- **Native-window mode:** mpv owns an ordinary top-level player window and may
  use its native input/OSC behavior.
- **Surface slot:** an axis-aligned logical rectangle reserved by Iced for
  native video. It is not an Iced texture.
- **Host:** the Iced view/window or native overlay surface participating in the
  presenter relationship.

## 7. Architectural Invariants

The following invariants apply across all platforms:

1. A decoded video frame MUST NOT cross into Iced/wgpu in the primary native-VO
   path.
2. `mpv_render_context` MUST NOT be created for the native-VO session.
3. The libmpv wakeup callback MUST only signal Ferrex. It MUST NOT call back
   into libmpv.
4. Normal libmpv calls and render/presenter operations MUST not form cyclic
   lock or wait dependencies.
5. Native presenter objects MUST be created, mutated, and destroyed on the
   platform-appropriate UI/event-loop thread unless the platform API explicitly
   permits otherwise.
6. Player domain logic MUST consume Ferrex-owned commands, events, snapshots,
   and track models instead of branching on `SubwaveVideo` versus mpv.
7. Presentation failure MUST be reported as a capability/error transition; it
   MUST NOT be inferred from a missing frame timer.
8. Geometry synchronization MUST happen at most once per host redraw/layout
   revision, not once per decoded frame.
9. The native presenter MUST be detached before its host window is destroyed.
10. Backend selection and every fallback transition MUST be logged with a
    machine-readable reason.
11. A retained shell MUST NOT be restored and a replacement native session
    MUST NOT launch until prior native teardown has reported positive
    completion. Teardown failure MUST keep later native launches closed for the
    remainder of that process.

## 8. Target Component Model

```text
+--------------------------------------------------------------+
| ferrex-player                                                |
|                                                              |
|  Player domain                                               |
|  +----------------------+       +--------------------------+ |
|  | PlaybackSnapshot     |<------| PlaybackEvent reducer    | |
|  | Ferrex track models  |       +--------------------------+ |
|  +----------+-----------+                    ^               |
|             |                                |               |
|             v                                |               |
|  Iced player controls ---- PlaybackCommand --+               |
|             |                                                |
|             v                                                |
|  NativeVideoSlot / dedicated player overlay                  |
+-------------+-------------------------------+----------------+
              | geometry/lifecycle            | control/events
              v                               v
+-----------------------------+    +-----------------------------+
| Platform presenter          |    | MpvSession                  |
| UI-thread native resources  |    | libmpv handle + event pump  |
+-------------+---------------+    +---------------+-------------+
              | native surface/window relationship  |
              +--------------------+-----------------+
                                   v
                         mpv native VO / gpu-next
```

### 8.1 Playback domain contract

Ferrex MUST own a backend-neutral contract. Its initial implementation lives
under `ferrex-player-playback::contract`; the exact Rust layout may evolve, but
it should have the following shape:

```rust
pub enum PlaybackCommand {
    Load(PlaybackSource),
    SetPaused(bool),
    SeekAbsolute(Duration),
    SeekRelative(DurationDelta),
    SetVolume(f64),
    SetMuted(bool),
    SetSpeed(f64),
    SelectAudio(TrackId),
    SelectSubtitle(Option<TrackId>),
    SelectChapter(ChapterId),
    SelectEdition(EditionId),
    SetContentFit(ContentFit),
    SetFullscreen(bool),
    Stop,
}

pub enum PlaybackEvent {
    StateChanged(PlaybackState),
    PositionChanged(Duration),
    DurationChanged(Option<Duration>),
    BufferChanged(BufferState),
    TracksChanged(TrackCatalog),
    ChaptersChanged(Vec<Chapter>),
    ChapterChanged(Option<ChapterId>),
    EditionsChanged(Vec<Edition>),
    EditionChanged(Option<EditionId>),
    VideoParametersChanged(VideoParameters),
    Ended(EndReason),
    Error(PlaybackError),
    Presenter(PresenterEvent),
}

pub struct PlaybackSnapshot {
    pub state: PlaybackState,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub tracks: TrackCatalog,
    pub chapters: Vec<Chapter>,
    pub current_chapter: Option<ChapterId>,
    pub editions: Vec<Edition>,
    pub current_edition: Option<EditionId>,
    pub video: Option<VideoParameters>,
    pub capabilities: PlaybackCapabilities,
}
```

Commands are asynchronous messages to the backend owner. Events update one
snapshot in the Iced application state. The view reads the snapshot and never
polls libmpv or GStreamer directly.

The GStreamer/Subwave adapter and mpv adapter MUST implement the same behavioral
contract during migration. Backend-specific diagnostics may be attached to a
structured diagnostic payload, but must not leak into normal player messages.

### 8.2 Mpv control API

`MpvSession` owns one libmpv core and provides:

- pre-initialization option setting;
- async command submission with request identifiers;
- typed and node-valued property get/set;
- property observation with stable observation identifiers;
- event draining;
- log forwarding with secret redaction;
- API/version and compiled-capability reporting; and
- an explicitly unsafe/raw escape hatch where the safe wrapper cannot represent
  a supported libmpv operation.

Typed Ferrex behavior maps to standard properties and commands, including at
least:

- `pause`, `time-pos`, `duration`, `speed`, `volume`, and `mute`;
- `track-list`, `aid`, `sid`, `chapter-list`, `chapter`, `edition-list`, and
  `edition`;
- `demuxer-cache-state`, `core-idle`, `eof-reached`, and `seeking`;
- `video-params`, `video-out-params`, `hwdec-current`, and `vo-configured`;
- `loadfile`, `seek`, `stop`, and playlist commands; and
- `fullscreen` where mpv owns the top-level window.

Content fit uses one deterministic native-VO property set so it does not depend
on renderer geometry or decoded-frame uploads:

| Ferrex fit | `keepaspect` | `video-unscaled` | `panscan` |
|---|---:|---|---:|
| Contain | `yes` | `no` | `0.0` |
| Cover | `yes` | `no` | `1.0` |
| Fill | `no` | `no` | `0.0` |
| None/native size | `yes` | `yes` | `0.0` |
| Scale down only | `yes` | `downscale-big` | `0.0` |

These properties are submitted asynchronously through the same serialized
owner as other controls. Ferrex updates the requested fit in its snapshot only
after the serialized owner accepts all three submissions.

The wrapper MUST preserve arbitrary command/property access so future mpv
features do not require a new Ferrex release merely to become reachable.

### 8.3 Presentation contract

The native presenter is separate from `MpvSession` because its resources and
threading rules differ. Conceptually it provides:

```rust
pub struct SurfaceGeometry {
    pub logical_bounds: Rectangle,
    pub visible_bounds: Option<Rectangle>,
    pub scale_factor: f64,
}

pub trait NativePresenter {
    fn attach(&mut self, host: HostWindow<'_>) -> Result<(), PresenterError>;
    fn synchronize(&mut self, geometry: SurfaceGeometry)
        -> Result<(), PresenterError>;
    fn set_visible(&mut self, visible: bool) -> Result<(), PresenterError>;
    fn set_fullscreen(&mut self, fullscreen: bool)
        -> Result<(), PresenterError>;
    fn detach(&mut self);
    fn capabilities(&self) -> PresenterCapabilities;
}
```

This trait is illustrative: the implementation may need a command handle around
a UI-thread-owned state machine instead of a Rust trait object. It MUST NOT
impose `Send` on platform resources that are correctly event-loop-local.

Capabilities include at least:

- integrated overlay support;
- embedded surface support;
- native HDR signaling support;
- fractional scaling support;
- native-window fallback availability;
- whether mpv or Iced owns fullscreen; and
- any known compositor requirement.

## 9. Session and Presenter Lifecycle

The implementation MUST model lifecycle explicitly. A suggested state model is:

```text
Uninitialized
    -> Initializing
    -> Idle
    -> Loading
    -> AwaitingHost / AwaitingVoSurface
    -> Presenting
    -> Hidden or Suspended
    -> Stopping
    -> Idle
    -> Terminating
    -> Terminated
```

Host and VO readiness can arrive in either order outside Wayland. Every attach
attempt carries a monotonically increasing generation so late native or libmpv
events from an old load cannot attach to a new player.

Required lifecycle behavior:

1. Entering the player creates or acquires a playback session.
2. Integrated mode waits for both host and VO readiness.
3. The presenter attaches once per generation.
4. Zero-sized or fully clipped geometry hides/unmaps presentation without
   destroying the playback core.
5. Window occlusion/suspension is forwarded as a power/performance hint.
6. Leaving the player detaches native presentation before closing its Iced
   window.
7. `stop` is issued and relevant final position is captured before the core is
   destroyed.
8. Presenter resources are destroyed before libmpv terminates when they refer
   to mpv-owned native objects.
9. On macOS, termination must keep the AppKit main loop serviceable while mpv's
   VO tears down.

The implementation MUST tolerate repeated load, stop, backend switch, and
window recreation cycles without retaining native surfaces or callbacks.

## 10. Threading and Event Delivery

### 10.1 Control plane

A dedicated owner serializes normal libmpv access. It may be a worker thread or
a strictly serialized executor, subject to platform constraints discovered in
the initial spike.

- `mpv_set_wakeup_callback` only wakes the owner/Iced subscription.
- The owner drains `mpv_wait_event(handle, 0)` until `MPV_EVENT_NONE`.
- Blocking property calls are not performed in Iced `view` or native callbacks.
- Prefer async commands and async property updates for user operations.
- Event payloads are copied into Ferrex-owned data before the next libmpv event
  call invalidates pointers.
- Log callbacks redact stream credentials and authorization headers.

### 10.2 Presenter plane

Presenter changes execute on the native UI/event-loop thread. Cross-thread mpv
signals are converted into presenter commands and wake Iced; they do not mutate
window-system objects directly.

The native VO renders at its own cadence. Iced redraws only for UI animation,
input, snapshot changes, or geometry changes. Playback MUST NOT create a
continuous Iced redraw loop merely to poll position. Periodic progress
persistence remains timer-driven at a substantially lower rate.

## 11. Iced Integration

### 11.1 Surface widget

An external `NativeVideoSlot` custom widget reserves layout and owns presenter
attachment state in `iced::advanced::widget::Tree::State`.

The pinned Iced revision does not expose a host on `Shell`. Before the first
attachment, the widget therefore emits one host-capture request. The
application services it with `window::run`, copies the raw window/display
handles into an event-loop thread-local lease, and returns only a pointer-free
result through the Iced task channel. The lease is detached/released before
window destruction and never imposes `Send` on the presenter or native host.

On `Window::RedrawRequested`, the widget:

1. verifies that the generation's event-loop-local host lease is ready;
2. reads its current layout bounds and inherited viewport;
3. computes visibility and the scale-aware geometry revision;
4. attaches the presenter if needed;
5. synchronizes geometry only if it changed; and
6. requests a follow-up redraw only when the presenter reports pending host
   work.

Its normal `draw` implementation does not draw video. It may draw a fallback
poster, loading state, or black rectangle before native presentation attaches.

When the widget leaves the tree, its state detaches the presenter. Window-close
handling MUST also perform explicit teardown so correctness does not depend
only on drop order.

### 11.2 Transparency and layers

In an integrated player, the Iced surface over the video region must preserve
alpha. Normal controls are rendered as SDR UI over a separately managed video
surface. This is intentional:

- mpv owns video color conversion and HDR description;
- the compositor/window system can blend SDR UI and HDR video as separate
  surfaces; and
- Iced does not need to select a 10-bit surface merely because video is HDR.

The implementation MUST verify that the Iced window does not advertise an
opaque region over transparent video pixels.

### 11.3 Iced upstream policy

The first implementation requires no new Iced API on the pinned fork:

- `Window` exposes raw window and display handle traits;
- `window::run` supports event-loop-local native-window setup callbacks;
- custom-widget `Tree::State` and redraw events provide slot lifecycle and
  geometry; and
- custom wgpu primitives remain available for a future render presenter.

Ferrex MUST NOT upstream its current platform-specific Wayland hook. If
prototype experience demonstrates a generic missing facility, the smallest
candidate is pass-through support for a foreign parent window in
`window::Settings`, corresponding to functionality already modeled by winit.
The proposal must use non-media examples such as webviews, terminals, and
camera surfaces, and must be discussed with Iced maintainers before a PR.

A one-shot generic pre-present action may be considered only if the Wayland
prototype proves synchronous staging during redraw is insufficient. Persistent
callback registries are out of scope.

## 12. Platform Presentation

### 12.1 Wayland

#### Current HYBRID decision

D-022 records HYBRID as the current Wayland release architecture. Integrated
playback uses the proven GStreamer/Subwave surface path; an mpv selection uses
mpv's ordinary native window. The bridge design below is retained as normative
re-entry criteria, not an active release commitment. Reopening it requires a
new decision backed by a safe per-session connection bootstrap or an explicit
amendment for another maintainable topology.

This Wayland-only decision does not alter the Windows or macOS integrated mpv
presenter requirements in sections 12.2 and 12.4.

#### Target ownership for bridge re-entry

Iced owns the real `xdg_toplevel`, decorations, input, application identity,
and fullscreen state. mpv owns a native Wayland video surface that becomes a
desynchronized subsurface below the transparent Iced surface.

This direction preserves Iced's existing input and application window while
allowing mpv to retain its normal Wayland VO, Vulkan/EGL WSI, dmabuf,
hardware-decoding, and color-management behavior.

#### Required bridge behavior

mpv does not support `wid` on Wayland. The presenter therefore uses a private
in-process protocol bridge inspired by Jellyfin Desktop's `wl-proxy` work. The
bridge MUST:

1. expose a private Wayland socket used only by mpv's VO;
2. forward ordinary requests, events, and file descriptors;
3. map mpv-created upstream objects onto the same upstream Wayland connection
   as the Iced parent;
4. capture the first relevant mpv `wl_surface`;
5. suppress forwarding of its `xdg_surface` and `xdg_toplevel` role requests;
6. assign the upstream surface a `wl_subsurface` role under Iced's parent;
7. use desynchronized child commits for independent video cadence;
8. synthesize `xdg_surface.configure` and `xdg_toplevel.configure` events from
   the surface slot's logical size and state;
9. translate mpv fullscreen, minimize, close, move, and resize requests into
   host actions where meaningful;
10. keep native pointer, keyboard, and touch ownership with Iced in integrated
    mode;
11. forward output, fractional-scale, viewporter, presentation-time, tearing,
    content-type, idle-inhibit, dmabuf, explicit-sync, and color-management
    behavior needed by the selected VO; and
12. destroy the subsurface role before the Iced parent is destroyed.

The bridge MUST NOT create its upstream child on an unrelated Wayland
connection. Wayland forbids constructing a subsurface relationship across
clients. `xdg-foreign` supplies relationship metadata, not reparenting, and is
not a substitute.

The bridge MUST NOT create a second viewport or color-management role on mpv's
surface when mpv already owns one. It virtualizes shell ownership while leaving
video-specific surface extensions with mpv.

#### Configure and geometry

The slot sends logical size, clipping visibility, scale, and host state to the
bridge. Subsurface position changes are staged before the next Iced parent
commit. mpv receives a configure matching the logical video extent and remains
responsible for buffer scale, viewport destination, and render size.

A geometry command must have a defined acknowledgment/order boundary before the
host presents. If current Iced redraw ordering is sufficient, no framework
change is made. If not, the generic Iced discussion described in section 11.3
is required before adding hooks.

#### Connection bootstrap risk

Stable libmpv does not accept an application-provided Wayland display for its
normal VO. W0 also observed delayed libmpv/libplacebo/driver connection
activity, so a temporary `WAYLAND_DISPLAY`/`WAYLAND_SOCKET` override cannot
safely direct only mpv to the bridge. A process-lifetime startup proxy avoids
the race only by proxying Iced too, which violates the current private mpv-only
boundary and greatly expands ownership risk. Symbol interposition and scoped
environment overrides are rejected. This blocker is the basis for D-022 and
must be resolved by a new architecture decision before bridge work resumes.

#### Wayland backend and fallback

Under D-022:

1. Auto/integrated playback uses the proven GStreamer Wayland path;
2. an explicit mpv selection uses ordinary mpv native-window mode; and
3. failure reaches the other policy-approved backend or an explicit playback
   error.

Integrated mpv is reported as unavailable with the connection-bootstrap reason.
Ferrex MUST NOT silently choose appsink/software presentation for HDR content.

### 12.2 Windows

The preferred full-player arrangement is:

- mpv owns its normal top-level HWND and gpu-next/D3D presentation;
- a transparent undecorated Iced playback window is attached as an owned or
  child overlay above the mpv content area;
- only one taskbar entry and one apparent player window are exposed; and
- Iced owns integrated input while fullscreen/window state is delegated through
  the presenter to the mpv root.

The presenter synchronizes content rectangle, DPI, visibility, z-order, focus,
minimize, and teardown. The overlay is created hidden, attached using raw
window handles, then shown to prevent startup flicker.

For an inline surface with no overlapping Iced controls, a host child HWND may
be passed as mpv's `wid`; mpv creates its own child and fills the host. This is
an alternate capability, not the required full-player arrangement.

Native-window mode leaves mpv's HWND independent and may enable mpv's OSC.

### 12.3 X11

mpv 0.41 compiles its X11 VO and `wid` support only when Meson's `gpl` option
is enabled. D-005 requires Ferrex release artifacts to link the reviewed
LGPL-only libmpv profile, so those paths are not present in the bundled
library. D-023 therefore makes X11 a licensing-gated hybrid: integrated
playback remains on GStreamer, and the separately launched external mpv action
may remain available as an explicit process boundary. In-process mpv MUST be
reported as unavailable rather than failing into a headless or CPU-copy VO.

If a future mpv release provides X11 native VO under a compatible profile, or
Ferrex adopts a different reviewed distribution policy, the preferred re-entry
arrangement mirrors Windows:

- mpv owns its normal X11 window;
- an ARGB Iced overlay is attached/stacked above it;
- geometry, focus, and visibility follow the mpv root; and
- Iced owns integrated input.

A compositing manager is required for that overlay. Non-composited X11 would
use a proven inline `wid` host with non-overlapping controls or a normal
native-window fallback, but neither path is claimed for the current LGPL-only
bundle.

### 12.4 macOS

The modern mpv Cocoa/Swift path owns its native `NSWindow` and video layer. The
presenter obtains the native mpv window when available and attaches the
transparent Iced controls `NSView` inside mpv's content hierarchy. The Iced
staging window remains unordered and is never a visible controls overlay. The
presenter synchronizes backing scale, content bounds, focus,
Spaces/fullscreen transitions, occlusion, and teardown on the AppKit main
thread.

The pinned winit 0.30.13 implementation is not natively reparent-safe: upstream
recovers its renderer view by casting the staging window's current content
view and resolves host-sensitive state through that staging window. Ferrex
therefore carries a narrow AppKit compatibility patch that retains the exact
`WinitView`, preserves its original logical `WindowId`, follows `[view window]`
for metrics/input state, observes an external root without replacing mpv's
delegate, suppresses donor lifecycle/fullscreen mutations while hosted, and
removes those observations before detach or close. This is a generic
foreign-view correction, not an mpv-specific Iced API. Native Apple
Silicon/Intel evidence remains mandatory before the patch is production
qualified or proposed upstream.

Ferrex MUST NOT make macOS depend on `wid`. Although generic libmpv header text
still mentions macOS, current mpv source does not consume `WinID` in the modern
macOS window backend. Native-root composition and ordinary native-window mode
are the supported strategies.

In-root view behavior across native fullscreen and Spaces must be proven in the
platform spike. If the foreign-view relationship cannot be made reliable,
normal mpv native-window mode remains the release fallback; an OpenGL render
path is not promoted merely to emulate embedding.

## 13. Input, Focus, and Window Ownership

Integrated mode routes keyboard, pointer, touch, and controller gestures
through Iced. Player actions produce `PlaybackCommand`s or raw mpv input
commands. Ferrex remains responsible for its current shortcuts and controls.

On macOS native-root playback, a background press is routed through the
canonical player message path to `performWindowDragWithEvent:` on mpv's root
window. Actual control and menu surfaces consume the press first; titlebar and
resize-frame behavior remains AppKit-owned. The hidden staging window MUST NOT
be the target of an Iced window-drag, focus, mode, minimize, maximize, or
fullscreen action.

The integration SHOULD expose a mapping layer for mpv key names so scripts and
bindings can be invoked intentionally. It does not need to forward every host
input event by default.

Window ownership differs by presenter:

- Wayland: Iced owns top-level state and sends synthetic state/configures to
  mpv.
- Windows and macOS native-root mode: mpv owns top-level state and the Iced
  overlay follows it.
- X11 remains GStreamer-integrated under D-023; the native-root rule is a
  re-entry requirement for a future compatible libmpv profile.

The application-level fullscreen command goes through the presenter and is
updated from the resulting native state. `PlayerDomainState` MUST not toggle an
optimistic fullscreen boolean without confirmation.

When the overlay is hidden, input policy must be explicit. Ferrex may keep a
transparent input target to reveal controls, or temporarily return input to
mpv and use mpv input bindings to reveal the overlay. The selected policy must
be tested for focus and power impact on each platform.

## 14. Color, HDR, Subtitles, and Frame Pacing

mpv owns:

- source color interpretation;
- hardware-decoder image import;
- scaling, tone mapping, dithering, and user shaders;
- native swapchain/surface format selection;
- native color-space and HDR metadata signaling;
- ASS, text, bitmap/PGS, and external subtitle rendering; and
- display synchronization and frame scheduling.

Iced owns SDR UI and lets the compositor/window system combine surfaces.
Ferrex MUST not infer HDR solely from filenames or force an HDR output profile
without mpv/native output evidence.

Capabilities and diagnostics should surface at least:

- `video-params` and `video-out-params`;
- `hwdec-current`;
- selected VO and graphics context;
- detected output color characteristics where mpv exposes them; and
- presenter/compositor color-management support.

"Zero-copy" is reported only as an observed diagnostic with evidence; it is not
a universal capability promise. Some decode formats and driver paths may
legitimately copy while still using native presentation.

## 15. Backend Selection and Fallback

Ferrex exposes conceptual backend choices:

- **Auto:** release-policy default with capability-based fallback.
- **mpv integrated:** require the native presenter; report a clear error or
  policy-approved fallback if unavailable.
- **mpv native window:** use ordinary mpv presentation and native OSC/input as
  configured.
- **GStreamer:** use the existing Subwave adapter during migration.
- **External mpv:** optional process-isolated fallback while it remains
  maintained.

Under D-022, Auto continues selecting integrated GStreamer on Wayland while
Windows and macOS proceed through independent gates toward integrated mpv. An
explicit Wayland mpv selection uses native-window presentation. Moving Wayland
Auto to integrated mpv requires a new GO decision and all deferred Wayland exit
criteria in the tracking plan.

Fallback selection MUST consider content requirements. For example, an
integrated presenter failure during HDR playback should prefer mpv
native-window mode over an SDR appsink path. The user-facing diagnostics must
state the selected backend and reason.

## 16. mpv Configuration and Compatibility Policy

Ferrex supplies a deterministic built-in mpv profile and permits supported
user overrides.

Default policy:

- prefer `vo=gpu-next` where included, while retaining a tested fallback list;
- begin with a conservative hardware-decoding policy and expose user override;
- do not load arbitrary user config or scripts unless the user enables it;
- do not disable mpv capabilities merely because Ferrex lacks a typed UI for
  them;
- pass authentication as headers/cookies where possible instead of embedding
  secrets in URLs; and
- redact URLs, headers, cookies, and tokens in logs and error reports.

Release builds SHOULD bundle libmpv, FFmpeg, libplacebo, and required runtime
assets at known versions. Runtime diagnostics include mpv, client API, FFmpeg,
and libplacebo versions.

The minimum supported libmpv version and exact linking strategy are finalized
by the dependency/packaging spike. The wrapper must fail gracefully with a
clear capability result when a system libmpv is missing or incompatible in a
development configuration.

## 17. Packaging and Platform Integration

The migration includes, not postpones, release packaging:

- Nix development and NixOS package inputs;
- Linux dynamic-library lookup and RPATH policy;
- Flatpak modules, permissions, GPU, audio, and Wayland socket behavior;
- Windows DLL discovery and bundling;
- macOS dylib/framework bundling, signing, and AppKit main-thread requirements;
- license inventory for the exact mpv/FFmpeg build options; and
- CI build coverage for every supported target.

A development machine finding a system libmpv is insufficient evidence of a
shippable backend.

## 18. Security

libmpv and optional user scripts operate inside the Ferrex process. Therefore:

- user config and scripts are opt-in and clearly described as trusted code;
- untrusted remote media does not control arbitrary mpv command execution;
- stream credentials are not placed in process arguments when avoidable;
- command and property names originating outside trusted Ferrex code are
  validated against their intended use;
- URL/header logging uses the existing application redaction policy or adds one
  before mpv rollout; and
- external tools such as `yt-dlp` are disabled by default unless explicitly
  packaged and enabled.

## 19. Observability

A diagnostic snapshot MUST include:

- requested and selected backend/presentation mode;
- every fallback decision and reason;
- libmpv and native presenter lifecycle state;
- VO, GPU API/context, adapter, and hardware decoder when exposed;
- current video and output color parameters;
- current surface logical/physical size and scale;
- dropped/delayed frame statistics exposed by mpv;
- Wayland bridge protocol/capability summary without sensitive object data; and
- the last structured playback/presenter error.

Normal logs should remain concise. Protocol tracing and verbose mpv logs are
opt-in diagnostics.

## 20. Verification and Acceptance Criteria

### 20.1 Automated coverage

The implementation requires tests for:

- command serialization and async reply correlation;
- property node conversion and event payload ownership;
- snapshot reduction under reordered or repeated property events;
- track identity and selection across reloads;
- lifecycle generation rejection of stale events;
- fallback policy decisions;
- final progress persistence on stop, EOF, error, and presenter failure;
- widget geometry, clipping, visibility, and drop behavior with a fake
  presenter;
- platform capability parsing where testable without a display; and
- repeated session creation/termination without leaked callbacks.

### 20.2 Platform matrix

Manual/integration coverage includes:

- Wayland: Hyprland/wlroots, KDE, and GNOME where available;
- Linux GPUs: Intel, AMD, and NVIDIA proprietary where available;
- X11 with and without a compositing manager;
- supported Windows versions with SDR and HDR displays;
- macOS Intel and Apple Silicon where supported; and
- fractional scaling and moving between displays with different scales.

### 20.3 Media matrix

At minimum:

- H.264, HEVC, VP9, and AV1;
- 8-bit SDR, 10-bit SDR, HDR10/PQ, and HLG where test hardware permits;
- representative software- and hardware-decoding paths;
- ASS with fonts/animation, SRT/WebVTT, PGS/DVD bitmap subtitles, and external
  subtitles;
- multiple audio/subtitle tracks, chapters, editions, and attachments;
- local files and authenticated HTTP range playback; and
- direct play plus Ferrex server transcoding output.

### 20.4 Native presentation gates

Before mpv becomes the default on a platform:

- play/pause/seek/track controls and progress reporting are feature-complete;
- resize, DPI change, minimize, hide/show, fullscreen, suspend/resume, and close
  are stable;
- no decoded frame enters the CPU/wgpu path in native-VO mode;
- expected hardware decoding is demonstrated through mpv diagnostics;
- HDR output and metadata are manually validated on supported hardware;
- Iced controls render and receive input without corrupting video color;
- 100 repeated load/stop/window cycles complete without native resource growth
  or crashes;
- presenter failure reaches a documented fallback; and
- release packaging installs and starts without developer-only paths.

### 20.5 Wayland-specific re-entry gate

D-022 defers this gate while HYBRID is active. Any future integrated Wayland
mpv proposal must satisfy, in addition to the above:

- the bridge proves parent and child are on the same upstream connection;
- no real `xdg_toplevel` role is assigned to mpv's video surface;
- configure/ack behavior remains valid across resize and fullscreen;
- fractional-scale, viewporter, dmabuf, explicit synchronization, presentation,
  and color-management traffic required by mpv is preserved;
- Iced retains input and no duplicate seat consumes events;
- teardown never outlives or destroys the Iced display; and
- failure is clean on compositors missing optional protocols.

## 21. Migration and Removal Policy

Migration is adapter-first:

1. Introduce Ferrex-owned playback commands, events, snapshots, and track
   models.
2. Adapt the current Subwave/GStreamer path without changing behavior.
3. Add libmpv control and native-window mode.
4. Add Windows and macOS integrated presenters; retain Wayland HYBRID unless a
   new bridge decision passes its re-entry gate.
5. Run both backends behind an opt-in selector and collect diagnostics.
6. Change per-platform Auto defaults only after that platform passes its gate.
7. Remove code only after at least one release retains a tested rollback.

GStreamer may remain after mpv becomes default when it provides a documented
capability not yet replaced. It is removed from desktop playback only when:

- no supported platform selects it in Auto;
- release and CI packaging no longer require it for playback;
- rollback data shows the mpv path is stable; and
- server/media-pipeline uses are confirmed independent.

The current external mpv path is removed or demoted only after in-process
native-window mode provides equivalent fallback and progress behavior.

## 22. Open Decisions and Required Spikes

The following are implementation decisions, not reasons to weaken the target
architecture:

1. Which Rust FFI foundation best exposes full libmpv while allowing a safe
   Ferrex wrapper?
2. What minimum bundled mpv version and build options are required on each
   platform?
3. Should development builds support runtime dynamic loading in addition to
   release bundling?
4. What future libmpv/upstream or otherwise maintainable mechanism can direct
   only mpv's Wayland connections to a private bridge without a process-global
   race? This is deferred under D-022.
5. If D-022 is reopened, can the bridge safely multiplex onto winit's existing
   display connection with independent event queues across all target
   compositors?
6. Is staging Wayland subsurface state during redraw sufficient, or is a
   generic one-shot pre-present facility demonstrably necessary?
7. Which transparent overlay relationship is most reliable for Windows and
   X11 while preserving one taskbar/window identity?
8. Does an Iced child NSWindow survive macOS native fullscreen and Spaces
   transitions reliably, or is a lower-level NSView target required?
9. What input policy best permits controls to appear when the transparent
   overlay is otherwise hidden?
10. Which mpv configuration and scripts are enabled by default without making
    behavior depend on a user's standalone mpv installation?

Each spike must produce a short decision record in the tracking plan before the
related production phase begins.

## 23. References

- mpv client API: <https://github.com/mpv-player/mpv/blob/master/include/mpv/client.h>
- mpv render API: <https://github.com/mpv-player/mpv/blob/master/include/mpv/render.h>
- mpv Vulkan render request: <https://github.com/mpv-player/mpv/issues/6575>
- mpv Vulkan/dmabuf request: <https://github.com/mpv-player/mpv/issues/11031>
- mpv gpu-next render draft: <https://github.com/mpv-player/mpv/pull/16818>
- mpv libplacebo render RFC: <https://github.com/mpv-player/mpv/pull/17828>
- Jellyfin Desktop native-VO precedent: <https://github.com/andrewrabert/jellyfin-desktop-sdl-cef>
- Switchfin direct-render precedent: <https://github.com/dragonflylee/switchfin>
- Iced contribution guidance: <https://github.com/iced-rs/iced/blob/master/CONTRIBUTING.md>
