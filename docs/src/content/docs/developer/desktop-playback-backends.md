---
title: "Desktop playback backends"
description: "Build, select, diagnose, and troubleshoot Ferrex's GStreamer, in-process libmpv, and external mpv desktop playback paths."
sidebar:
  order: 7
---

Ferrex is migrating desktop playback behind a backend-neutral contract. The
player domain consumes Ferrex-owned commands, events, snapshots, tracks, and
capabilities; concrete GStreamer and libmpv objects stay inside their adapters.
The [architecture specification](https://github.com/Lowband21/ferrex/blob/dev/docs/specs/native-mpv-playback.md)
defines the target, while the
[migration plan](https://github.com/Lowband21/ferrex/blob/dev/docs/plans/native-mpv-playback-migration.md)
records measured rollout gates.

## Shipped choices and current defaults

| Choice | Presentation | Current role |
|---|---|---|
| Auto | Platform release policy | GStreamer remains first during the rollback/soak window. |
| GStreamer | Integrated Subwave surface or embedded fallback | Default migration backend; the integrated HDR path on Wayland and the LGPL-compatible X11 path. |
| mpv integrated | Platform native presenter | Explicit Windows/macOS handoff build; never selected by Auto until that platform's representative-system gate passes. |
| mpv native window | libmpv `gpu-next` native VO | Developer opt-in and compatibility path; no decoded frame enters Iced/wgpu. |
| External mpv | Separate process and private IPC | Explicit crash-isolated/X11 compatibility handoff; never selected by Auto. |

Wayland is intentionally HYBRID: integrated playback stays on GStreamer and an
explicit mpv request uses mpv's ordinary native window. Stable libmpv cannot
currently direct only its delayed Wayland connections through a private bridge
without a process-global race. X11 also stays on GStreamer in the reviewed
LGPL-only package because mpv 0.41 gates its X11 VO and `wid` code on the GPL
build profile. Neither policy silently selects a CPU-copy mpv path.

Windows and macOS now have compile-gated Win32/AppKit presenter handoff builds,
with independent package and representative-system gates. Consult the
migration plan rather than assuming that a development machine finding libmpv
proves a release artifact or a production-ready integrated presenter.

## Build and select the developer backend

The default feature set remains buildable without linked libmpv. Enable the
in-process backend explicitly:

```bash
cargo run -p ferrex-player --features mpv
```

Use **Play in MPV** in an mpv-enabled build. On Windows and macOS it requests
the compile-gated integrated presenter and falls back to mpv's native window
with a structured reason if preflight or attachment fails. On Wayland it
requests the ordinary native-window path; the reviewed X11 package retains the
external handoff. If the build omits the mpv feature, the historical action
retains its external-process behavior. Auto is deliberately unchanged during
the staged rollout.

The exact spike build commands and hardware checklist are in the
[native playback fixture handoff](/developer/native-playback-fixtures/#windows-and-macos-integrated-presenter-handoff/).

Release packages must carry the pinned, LGPL-only libmpv/FFmpeg/libplacebo
closure. A system libmpv found through a developer package manager is not
release evidence. The Flatpak and Nix profiles additionally reject mpv's GPL
option and record their resolved feature/license profiles.

## Deterministic fallback order

Every requested and selected target is recorded separately. During migration,
Auto tries integrated then embedded GStreamer. An unavailable exact request can
follow the policy-approved chain through mpv native-window, integrated or
embedded GStreamer, and finally the explicit external target. Duplicate targets
are removed, and the first rejection becomes the machine-readable fallback
reason.

HDR-required selection rejects candidates that cannot preserve native HDR
signaling. A presenter failure should therefore prefer mpv native-window over
an SDR frame-upload path. Initialization/load failures checkpoint the last
observed position before returning to Auto/GStreamer.

Common reason codes include `backend_disabled`, `unsupported_platform`,
`requested_unavailable`, `missing_capability`, `presenter_failed`, and
`backend_failure`. A fallback is not inferred from a missing frame timer.

## Configuration and trusted code

Ferrex's default libmpv profile disables standard user config, script
discovery, and external URL resolvers. Native-window compatibility enables
controlled OSC/input bindings; integrated presentation disables them because
Iced owns input. Trusted local development can opt into normal mpv config,
`input.conf`, and scripts:

```bash
FERREX_MPV_CONFIG_POLICY=trusted-user \
  cargo run -p ferrex-player --features mpv
```

Those files execute inside the Ferrex process. Invalid or non-Unicode policy
values fail closed to `deterministic` and are not repeated in logs.

`FERREX_MPV_PATH` applies only to discovery of the separately launched mpv
executable, primarily on Windows. It does not replace or locate the linked
libmpv used by the in-process feature.

## Safe diagnostics

Open the in-player settings panel during playback to see the requested and
selected backend, presentation mode, integrated-presenter state, fallback
reason, input HDR evidence, native-output HDR evidence, configured decoder
policy, and observed hardware decoder. These labels deliberately do not infer
HDR, hardware decoding, or zero-copy from the backend name.

The serializable diagnostic snapshot also includes client/runtime versions,
VO/GPU context and adapter, color parameters, frame timing counters, presenter
geometry and scale, capability flags, and the ordered fallback chain. It never
contains the playback URI, headers, cookies, user config contents, shader
paths, screenshot paths, or copied log messages.

For a fixed native-message filter, set `FERREX_MPV_LOG_LEVEL` to `none`,
`fatal`, `error`, `warn`, `info`, `verbose`, `debug`, or `trace`, and permit the
same Ferrex target with `RUST_LOG`:

```bash
FERREX_MPV_LOG_LEVEL=trace \
  RUST_LOG=ferrex_player_playback=trace,ferrex_player_mpv=trace \
  cargo run -p ferrex-player --features mpv
```

Without the override, Ferrex captures bounded verbose startup evidence and
returns to informational native messages after file initialization. Invalid
values fail closed without being echoed. Credential and active-source
redaction still applies, but diagnostic logs can reveal local filenames,
hardware names, or private network topology; review them before sharing.

For deeper, redacted protocol evidence, use the documented
[fixture matrix](/developer/native-playback-fixtures/) and
[Wayland trace harness](/developer/native-mpv-wayland-spike/). Store generated
media and results only below the ignored `target/` locations described there.

## Troubleshooting

### The in-process choice falls back immediately

Check that the player was built with `--features mpv`, then inspect the visible
fallback reason. `backend_disabled` means the feature is absent.
`unsupported_platform` on packaged X11 is expected under the LGPL profile.
`backend_failure` or `backend_initialization` usually indicates an incompatible
or missing runtime library; compare the reported client API with the minimum
API 2.2 requirement.

### mpv opens but the video is not integrated

Native-window presentation is the selected compatibility mode, not a hidden
render failure. Check the presenter build gate and structured fallback reason.
An integrated request selects the normal mpv window when preflight or
attachment fails; it must not leave the hidden controls host behind. Passing
one development run still does not approve the lifecycle, input, HDR, stress,
packaging, or Auto gates.

### Playback works in development but not in a package

Inspect the package's loader closure and license/build-profile record. It must
not depend on a Nix store or local package manager path. Flatpak libraries must
resolve from `/app`; Windows and macOS packages must carry their reviewed DLL
or dylib closure. Do not work around a missing package closure by silently
switching to a software/headless VO.

### Authenticated media fails

In-process backends receive a credential-free URL and playback-scoped bearer
header. Avoid putting a ticket in command arguments or logs. Run the
[playback authentication regression](/reference/qa/playback-auth-regression/)
to distinguish ticket issuance, HTTP range/HLS propagation, and decoder
failures. The external compatibility process receives its temporary query URL
over private IPC rather than argv.

### Controls or episode transitions behave differently

Capture the requested/selected targets and fallback chain, then reproduce with
the generated multitrack fixture. Progress, EOF/error handling, next/previous,
Back/Home, track selection, and resume are backend-neutral policy. A difference
between backends is a regression unless diagnostics explicitly report an
unsupported capability.

## Rollback

Auto remains the one-step rollback to the current GStreamer policy during the
migration. The external mpv action remains explicit and process-isolated. Do
not remove GStreamer, external mpv, or the pinned Iced/Subwave capability until
the platform gate, release artifact, fallback, soak period, and at least one
rollback release have been verified and recorded.
