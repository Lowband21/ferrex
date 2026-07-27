---
title: "Desktop playback backends"
description: "Select, diagnose, and troubleshoot Ferrex's GStreamer and mpv desktop playback paths."
sidebar:
  order: 7
---

Ferrex keeps playback state and controls independent of the selected engine.
GStreamer, in-process libmpv, and external mpv implement the same player-domain
contract while retaining their own presentation and lifecycle rules. See the
[native mpv design](https://github.com/Lowband21/ferrex/blob/dev/docs/specs/native-mpv-playback.md)
for ownership and platform decisions.

## Available backends

| Choice | Presentation | Role |
|---|---|---|
| Auto | Platform policy | Integrated mpv on macOS; existing policy elsewhere. |
| GStreamer | Integrated or embedded Subwave surface | Primary integrated path on Wayland and X11; unavailable on macOS. |
| mpv integrated | Platform-native presenter | Default macOS presenter and explicit Windows presenter. |
| mpv native window | libmpv native VO | In-process compatibility path with no decoded frames in Iced/wgpu. |
| External mpv | Separate process controlled through private IPC | Explicit process-isolated compatibility path. |

## Build and select mpv

Normal macOS player builds enable the in-process backend automatically. On
other platforms, enable it explicitly:

```bash
cargo run -p ferrex-player --features mpv
```

In an mpv-enabled build, **Play in MPV** requests the platform's supported mpv
presentation. If integrated presentation is unavailable, Ferrex records the
reason and uses mpv's native window when policy permits. Without the `mpv`
feature, the action retains its external-process behavior.

## Platform behavior

| Platform | Integrated path | Explicit mpv behavior |
|---|---|---|
| Wayland | GStreamer subsurface | Ordinary mpv native window. |
| X11 | GStreamer | External mpv under the reviewed LGPL profile. |
| Windows | GStreamer remains available | Explicit native presenter with native-window fallback. |
| macOS on Apple Silicon | In-root AppKit mpv presenter | Default; functional one-window path validated, with follow-ups below. |

Intel/x86_64 Macs are legacy and outside the supported validation matrix.

## Current macOS follow-ups

Native Apple Silicon validation confirmed normal `Auto` playback as one
mpv-owned window with working in-root Ferrex controls. Remaining polish:

- window size and fullscreen state do not always remain consistent across
  transitions; and
- automatic OSD reveal can wait for the next pointer click.

HDR/EDR output, a particular VideoToolbox decode path, and extended stress
qualification remain unclaimed.

## Fallback

Ferrex records requested and selected backends separately. An integrated mpv
request may fall back to mpv native-window playback, then another
policy-approved backend where the platform allows one. On macOS, native-window
mpv is the end of the fallback chain; GStreamer is never selected. Duplicate
targets are removed from the chain, and the first rejection is retained as the
visible reason.

Fallback does not silently choose a decoded-frame upload path to preserve the
appearance of integration. Playback position is checkpointed before returning
to another backend after an initialization or load failure.

## Configuration and trusted code

Ferrex's deterministic libmpv profile disables standard user config, script
discovery, and external URL resolvers. Native-window mode enables only the
controlled input behavior required for that presentation. Trusted local use
can opt into normal mpv config, `input.conf`, and scripts:

```bash
FERREX_MPV_CONFIG_POLICY=trusted-user \
  cargo run -p ferrex-player --features mpv
```

Those files execute inside the Ferrex process. Invalid policy values fail
closed to the deterministic profile and are not repeated in logs.

`FERREX_MPV_PATH` applies only to discovery of the separately launched mpv
executable. It does not locate or replace the libmpv linked by the in-process
feature.

## Safe diagnostics

The in-player settings panel reports the requested and selected backend,
presentation mode, presenter state, fallback reason, and observed video output
information. HDR, hardware decoding, and zero-copy are reported only when
runtime evidence supports them.

Set the native mpv log filter with `FERREX_MPV_LOG_LEVEL` and enable the Ferrex
targets with `RUST_LOG`:

```bash
FERREX_MPV_LOG_LEVEL=debug \
  RUST_LOG=ferrex_player_playback=debug,ferrex_player_mpv=debug \
  cargo run -p ferrex-player --features mpv
```

Supported native levels are `none`, `fatal`, `error`, `warn`, `info`,
`verbose`, `debug`, and `trace`. Invalid values fail closed without being
echoed. Credentials and active media sources are redacted, but logs may still
identify local hardware or network topology; review them before sharing.

## Troubleshooting

### The in-process choice falls back immediately

Outside macOS, confirm that the player was built with `--features mpv`, then
inspect the visible fallback reason. A disabled backend means the feature is
absent. An initialization failure usually means the linked runtime is missing
or incompatible. In-process mpv being unavailable on X11 is expected under
the LGPL profile.

### mpv opens in its own window

Native-window playback is the compatibility mode, not a hidden render failure.
Inspect the presenter state and fallback reason. An attachment failure must
dismiss the controls host before selecting the normal mpv window.

### Authenticated media fails

In-process backends receive a credential-free URL and playback-scoped header.
The external process receives its source through private IPC rather than its
argument vector. Inspect the server response, redacted source diagnostics, and
decoder error separately when narrowing a failure.
