---
title: "Native mpv extension API"
description: "Safe ownership, arbitrary command/property access, and trust boundaries for extending Ferrex's libmpv backend."
sidebar:
  order: 11
---

Ferrex keeps ordinary playback behavior behind the backend-neutral contract in
`ferrex-player-playback`. Features that need mpv functionality not yet modeled
there can use the public `ferrex-player-mpv` control-plane crate. This is the
documented developer API for raw commands, node values, properties,
observations, hooks, client messages, and future native events; it is not a
user-facing command console.

Use a typed `PlaybackCommand`/`PlaybackEvent` when behavior must also work with
Subwave or another backend. Keep an mpv-only operation at this extension
boundary when it is diagnostic, experimental, or inherently mpv-specific.

## Choose one owner model

`MpvSession` is a thread-affine, `!Send` owner. It is appropriate when the
platform event loop must create and service libmpv on the current thread.
`MpvWorker` owns the same session on one named thread and serializes requests
through bounded channels. Normal desktop playback uses `MpvWorker`; do not put
an `mpv_handle` in application state or call libmpv concurrently from Iced.

The wakeup callback only coalesces an atomic signal and unparks the owner.
Drain events on the owner and consume only the owned `MpvEvent` values returned
by Ferrex. Native event pointers are invalidated by the next
`mpv_wait_event` call and must never escape the wrapper.

## Commands, properties, and observations

The public owner APIs provide:

- `command_async` for standard string-vector commands;
- `command_node_async` for arbitrary array/map commands and nested `MpvNode`
  values;
- `get_property_async` and `set_property_async` for string, flag, integer,
  double, null, byte-array, array, and map values;
- `observe_property` and `unobserve_property` for arbitrary documented or
  future properties;
- `add_hook`, `continue_hook`, `set_event_enabled`, and client-message events;
- correlated `MpvRequestId`/`MpvObservationId` values in copied replies; and
- bounded node conversion through `MpvNodeLimits`.

A minimal owner-thread sketch is:

```rust
use ferrex_player_mpv::{
    MpvEvent, MpvFormat, MpvFunctionTable, MpvSessionConfig, MpvWorker,
    MpvWorkerConfig,
};

let mut worker = MpvWorker::spawn(
    MpvFunctionTable::linked(),
    MpvSessionConfig::native_window(),
    MpvWorkerConfig::default(),
)?;
let observation = worker.observe_property("estimated-vf-fps", MpvFormat::Double)?;
let request = worker.command_async(["show-text", "Ferrex diagnostic"])?;

for event in worker.drain_events() {
    match event {
        MpvEvent::PropertyChanged(change) if change.id == observation => {
            // The value is an owned MpvNode; reduce it into feature-owned state.
        }
        MpvEvent::AsyncReply(reply) if reply.id == request => {
            // Correlate success/failure without relying on event order.
        }
        _ => {}
    }
}

worker.unobserve_property(observation)?;
worker.shutdown()?;
```

Do not submit credentials through commands intended for logs or a process
argument vector. Authenticated media must continue to use the redacted
`PlaybackSource` mapping, which applies headers/cookies as per-file options.
Never log arbitrary command arguments, node values, property values, or copied
client messages without applying the playback redaction policy.

## Capability-gated playback extensions

User-facing local extensions use Ferrex-owned commands instead of exposing the
mpv owner through application state:

- `PlaybackSession::add_external_subtitle` maps a redacted local sidecar path
  to `sub-add`, optionally selects it, and lets normal `track-list` reduction
  expose the new stable identity and `is_external` flag;
- `PlaybackSession::capture_screenshot` maps to one `screenshot-to-file`
  request and requires an explicit destination plus video-only,
  video-with-subtitles, or full-window mode;
- `PlaybackSession::set_video_shaders` replaces mpv's ordered `glsl-shaders`
  list using argument-separated `change-list` commands; an empty list clears
  it; and
- `PlaybackSession::apply_video_profile` maps to `apply-profile` only when the
  effective policy enables trusted user configuration.

Inspect `PlaybackSnapshot::capabilities` before presenting these actions.
Subwave and other unsupported backends return a structured
`UnsupportedOperation` error rather than silently ignoring them. Explicit
shader files and screenshot destinations use `PlaybackFilePath`, whose debug
form is redacted. Diagnostic schema version 6 reports support booleans, the
observed active shader count, and the effective native log filter; it never
reports profile names, local paths, or copied log contents. Command arguments
remain separate libmpv values, and empty, multiline, overlong, or non-Unicode
paths fail before submission.

On 2026-07-12 the display-backed mpv 0.41 native-VO smokes loaded and selected
an external SRT sidecar, applied a temporary identity shader, confirmed the
observed shader count, wrote a non-empty screenshot, cleared the shader list,
and removed both temporary files. Separate runs covered the generated
multitrack text, animated ASS/attached-font, and PGS fixtures. Normal tests
verify command shapes and redaction without a display.

## Initialization options and trusted config

`MpvSessionConfig::with_option` is the pre-initialization option escape hatch.
Options are applied in order before `mpv_initialize`; they are not runtime
properties. Production behavior should start from a Ferrex profile rather than
constructing an unreviewed option list.

`MpvConfigPolicy::Deterministic` is the default. It disables standard user
config, script discovery, and external URL resolvers. The developer-only
`FERREX_MPV_CONFIG_POLICY=trusted-user` player switch selects
`MpvConfigPolicy::TrustedUser`, which enables standard mpv config,
`input.conf`, and scripts. Those files are trusted code running inside Ferrex.
Invalid policy values fail closed and diagnostics expose only the effective
high-level switches, never config contents or paths.

## Native logging and protocol traces

Normal playback briefly requests verbose libmpv messages during file startup
to discover version, VO, GPU, and adapter evidence, then returns to the `info`
filter. For an opt-in diagnostic run, set `FERREX_MPV_LOG_LEVEL` to one of
`none`, `fatal`, `error`, `warn`, `info`, `verbose`, `debug`, or `trace` to keep
that fixed native filter for the session. Use `RUST_LOG` separately to permit
the corresponding Ferrex log target, for example:

```bash
FERREX_MPV_LOG_LEVEL=trace \
  RUST_LOG=ferrex_player_playback=trace,ferrex_player_mpv=trace \
  cargo run -p ferrex-player --features mpv
```

Invalid or non-Unicode values fail closed and are not repeated in logs.
Messages still pass through generic credential redaction and the active
playback-source redactor before reaching the application logger. Review traces
for private filenames or server topology before sharing them. Wayland bridge
protocol capture is currently the redacted opt-in W0 harness documented in the
[Wayland spike](./native-mpv-wayland-spike/); no bridge runtime ships under the
HYBRID decision.

## Raw native escape hatch

`MpvSession::with_raw_handle` is the final unsafe boundary for a client API
symbol that the wrapper does not yet represent. Its callback must not retain or
destroy the handle, drain events, replace the wakeup callback, or race the
serialized owner. Prefer extending the fakeable function table and safe wrapper
instead. Any graphics/render-context work also requires an architecture review:
the native-VO migration must not become a private decoded-frame path.

## Verification expectations

Add fake-ABI coverage for every new command/property/event shape, including
reply correlation, cancellation, copied lifetimes, and teardown. Tests that
need a real VO belong in the ignored display-backed smoke gate and must use the
schema-generated fixtures. Keep normal unit tests display-free and ensure the
player still compiles without the `mpv` feature.
