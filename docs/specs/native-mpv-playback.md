# Native mpv playback design

This document records the durable design of Ferrex's desktop mpv integration.
It describes ownership, platform behavior, fallback, and validation. The Rust
types and tests remain the source of truth for implementation details.

## Scope

Ferrex uses libmpv as a playback control plane while mpv retains ownership of
decoding and native video presentation. Iced owns application layout, controls,
input, and non-video UI. Decoded video does not pass through Iced or wgpu in the
native-VO path.

Selection is platform-specific. On macOS, mpv is the default and only supported
playback engine; GStreamer is unavailable. Other platforms retain their
existing selection policy and may use mpv's ordinary native window or the
external mpv compatibility path.

## Durable decisions

1. **Use mpv's native VO.** Ferrex does not build its primary playback path on
   `mpv_render_context` or transfer decoded frames through wgpu.
2. **Keep the player domain backend-neutral.** Ferrex-owned commands, events,
   snapshots, and track models isolate UI and persistence behavior from the
   selected playback engine.
3. **Use a pinned LGPL libmpv profile.** The supported in-process build is mpv
   0.41 with GPL-only components disabled. Runtime compatibility is checked
   before a session starts.
4. **Make fallback explicit and deterministic.** Presenter failure can fall
   back to mpv native-window playback. GStreamer and external mpv remain
   separately selectable where platform policy permits them; macOS never
   falls back to GStreamer.
5. **Keep Wayland hybrid.** GStreamer remains the integrated Wayland backend.
   Explicit mpv playback uses an ordinary native window because stable libmpv
   cannot safely direct only its delayed Wayland connections to Ferrex's host
   connection.
6. **Keep X11 hybrid under the LGPL profile.** mpv 0.41 requires its GPL build
   option for the X11 VO and `wid`, so the reviewed in-process build does not
   claim X11 presentation.
7. **Use native platform presenters on Windows and macOS.** mpv owns the native
   video root while Ferrex attaches Iced controls without copying video frames.
8. **Treat configuration and credentials as security boundaries.** User mpv
   config is opt-in, media credentials stay out of normal logs and process
   arguments, and diagnostic output is redacted.

## Component and ownership model

```text
Iced player UI
    |
    | PlaybackCommand / PlaybackSnapshot
    v
Ferrex playback domain
    |                         |
    | control and events      | geometry and lifecycle
    v                         v
libmpv session owner      platform presenter
    |                         |
    +------------+------------+
                 v
          mpv native video output
```

`ferrex-player-playback` owns the backend-neutral contract and lifecycle
reduction. Backend adapters translate that contract to GStreamer, in-process
mpv, or the external process boundary. Views read a `PlaybackSnapshot`; they do
not poll libmpv or GStreamer directly.

`ferrex-player-mpv` owns the libmpv handle, command serialization, property
observation, event copying, compatibility checks, and log redaction. Native
callbacks only wake the owner. They do not call back into libmpv or mutate UI
state.

The platform presenter is independent of the libmpv control owner. It keeps
window-system objects on the UI thread, joins the Iced controls host to mpv's
native root, and reports presentation changes through copied Ferrex events.

## Lifecycle invariants

- Host readiness and mpv-window readiness may arrive in either order.
- Every attachment belongs to a generation; stale events cannot attach an old
  native surface to a replacement session.
- Geometry is synchronized on host revisions, not decoded-frame cadence.
- Zero-sized, clipped, hidden, or suspended hosts do not require destruction of
  the playback core.
- The presenter detaches before its Iced host or mpv-owned native objects are
  released.
- Final progress and terminal state are captured before the session is
  destroyed.
- The retained application shell, navigation, and replacement playback wait
  for positive native teardown completion.
- A teardown failure prevents another native presenter from starting in the
  same process.

These rules keep native presentation independent of Iced redraw timing and
prevent overlapping AppKit, Win32, or libmpv ownership during replacement.

## Platform behavior

| Platform | Integrated path | Explicit mpv path | Current decision |
|---|---|---|---|
| Wayland | GStreamer subsurface | mpv native window | Hybrid; no private in-process Wayland bridge |
| X11 | GStreamer | external mpv | Hybrid under the LGPL libmpv profile |
| Windows | GStreamer remains available | native mpv root with Iced controls presenter, then native-window fallback | Presenter remains explicitly gated |
| macOS on Apple Silicon | in-root AppKit mpv presenter | mpv native-window fallback | Default golden path; core path functionally validated |

Intel/x86_64 Macs are legacy and outside the supported validation matrix.

### Wayland

Ferrex does not proxy mpv's Wayland objects into Iced. Stable libmpv has no
per-session mechanism for directing only its delayed VO connections to a
private display, and process-wide environment changes would be racy. The
integrated path therefore remains the existing GStreamer subsurface. Selecting
mpv uses its ordinary native window.

### X11

The reviewed LGPL mpv 0.41 profile excludes the X11 VO and `wid`. Ferrex reports
in-process X11 mpv as unavailable instead of allowing a headless or CPU-copy
fallback. Integrated GStreamer and the external mpv process remain available.

### Windows

mpv owns the video HWND. A taskbar-suppressed Iced controls host follows its
content geometry, DPI, visibility, focus, minimize, fullscreen, and teardown.
Failure to attach dismisses the controls host and leaves mpv native-window
playback available.

### macOS

mpv owns the sole visible `NSWindow`. Ferrex reparents the Iced renderer
`NSView` into that window's content hierarchy; the Iced staging window remains
unordered and is never presented as an overlay. AppKit access stays on the main
thread, and blocking libmpv shutdown begins only after the view is detached.

The presenter derives backing scale, focus, cursor, IME, visibility,
fullscreen, Spaces, and close events from local content bounds and the actual
host window. Ferrex does not use macOS `wid` and does not create, position,
focus, or continuously monitor a second controls window.

This required a temporary macOS-only winit 0.30.13 compatibility patch. The
patch retains the exact renderer view independently of its donor window,
preserves its logical `WindowId`, follows the current host for window-sensitive
state, suppresses donor lifecycle mutations while foreign-hosted, and removes
external-root observers before detach or close. Although platform-scoped, it
spans substantial view, event, focus, IME, notification, scaling, and teardown
semantics. The correction is generic to foreign AppKit view hosting and
contains no mpv- or window-manager-specific policy.

Native Apple Silicon validation confirmed normal `Auto` playback as one
mpv-owned window with working in-root Ferrex controls. It does not qualify
HDR/EDR output, a particular VideoToolbox decode path, or the remaining polish
items recorded in the
[desktop backend guide](../src/content/docs/developer/desktop-playback-backends.md#current-macos-follow-ups).

## Winit fork exit

The vendored winit patch is temporary. Ferrex will upstream or delete it rather
than maintain a permanent fork.

The preferred exit is a generic upstream change that safely retains the actual
`WinitView`, defines window-sensitive behavior while that view is hosted by a
different `NSWindow`, and adds non-media regression coverage for embedded views.
Once a released winit version containing that support is consumed by Iced,
Ferrex removes the `[patch.crates-io]` override and the vendored tree in the same
upgrade.

The next winit/Iced upgrade may not rebase or expand this fork. It must either
consume released upstream support and remove the override and vendored tree,
or select the contingency: a Ferrex-owned macOS Iced host built from
`iced_runtime` and `iced_wgpu`, with AppKit input and rendering implemented
inside the platform adapter beneath mpv's root window.

Removing the vendored fork is an acceptance criterion for completing the macOS
integration. Intel macOS is not an additional prerequisite.

## Backend selection and fallback

Ferrex distinguishes these choices:

- **Auto:** integrated mpv on macOS; the existing platform policy elsewhere.
- **mpv integrated:** require the platform presenter.
- **mpv native window:** use mpv's normal top-level window.
- **GStreamer:** use the existing adapter where supported; unavailable on macOS.
- **External mpv:** use the process-isolated compatibility boundary.

An integrated-presenter failure is recorded before Ferrex selects the next
policy-approved path. On macOS that path can only be mpv's native window; an
mpv initialization failure ends playback instead of entering GStreamer.
Fallback never silently changes into a decoded-frame upload path to preserve
the appearance of integration. Diagnostics report the requested backend,
selected backend, presentation mode, and fallback reason.

## Configuration, extensions, and security

Ferrex starts mpv with a deterministic profile. Standard user configuration,
input bindings, and scripts are disabled unless the user explicitly selects
the trusted-user policy. Invalid policy values fail closed.

Playback sources carry credentials as typed, redacted headers. In-process
backends receive those headers directly. The legacy external process receives
its source through private IPC rather than the child argument vector. Logs,
errors, diagnostics, and retained state do not expose media URLs, headers,
cookies, local paths, or tokens.

Ferrex exposes common operations through typed playback commands. Optional mpv
features such as sidecar subtitles, profiles, shaders, and screenshots remain
capability-gated. Unsupported backends return a structured unsupported result
without changing the selected backend.

## Diagnostics and validation

Runtime diagnostics distinguish configured policy from observed behavior. They
may include libmpv version, VO, graphics API, hardware decoder, input and output
color parameters, presenter state, scale, geometry, and fallback history.
Ferrex reports HDR or zero-copy only when the native output provides supporting
evidence.

Automated coverage exercises command and event ownership, compatibility
checks, source redaction, fallback reduction, stale-generation rejection,
geometry and visibility changes, detach ordering, progress persistence, and
repeated session teardown. Platform-specific tests cover the display-free
presenter state machines.

## References

- [mpv client API](https://github.com/mpv-player/mpv/blob/master/include/mpv/client.h)
- [mpv render API](https://github.com/mpv-player/mpv/blob/master/include/mpv/render.h)
- [Ferrex winit patch notes](../../third-party/winit-0.30.13-ferrex/FERREX-PATCH.md)
