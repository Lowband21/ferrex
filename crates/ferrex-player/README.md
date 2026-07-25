# Ferrex Player

Desktop client for Ferrex (native UI + video backend).

Status: pre-alpha. Expect breaking changes.

Implementation note: the runtime application shell lives in the
`ferrex-player-app` workspace crate and the UI implementation lives in
`ferrex-player-ui`; this package keeps the installed binary name and
compatibility facade.

## Crate graph

Runtime dependencies flow downward from the facade and app shell into the UI
crate, then into dependency-light player data/API crates:

```text
ferrex-player -> ferrex-player-app -> ferrex-player-ui
  -> ferrex-player-auth / ferrex-player-repository / ferrex-player-library
  -> ferrex-player-media / ferrex-player-metadata / ferrex-player-playback
  -> ferrex-player-search / ferrex-player-settings / ferrex-player-user-admin
  -> ferrex-player-api / ferrex-player-foundation
```

Only `ferrex-player-app`, `ferrex-player-ui`, and the extracted
`ferrex-player-playback` video domain own Iced/subwave runtime code. The lower
player crates expose state, selectors, service contracts, domain tasks, or
stream builders that the UI crate adapts; settings only shares `iced_core`
color/point DTOs for accent-color state. See the
[Player dependency boundaries](https://ferrexmedia.org/developer/player-dependency-boundaries/)
for the guard policy and intentional compatibility shims.

## Build prerequisites

- Nix is recommended for Linux development: `nix develop` enters the canonical
  player-capable shell with pinned GStreamer/GPU runtime wiring.
- `nix develop .#ferrex-player` is a backward-compatible alias for the same
  shell; use `nix develop .#server` when you only need the lean server/core
  environment without player-only GStreamer/GPU inputs.
- Without Nix, install Rust 1.90+ (workspace MSRV) plus Linux GStreamer + FFmpeg
  development headers. The CI workflow shows the current package list used for
  builds.

## Running

Start the server stack:

```bash
just start
```

Run the player:

```bash
just run-player
# or: just run-player-release
```

The player connects to `FERREX_SERVER_URL` (defaults to `http://localhost:3000`).

## Headless screenshots

Capture a preset UI state to a PNG and JSON sidecar without opening a desktop
window:

```bash
ferrex-player screenshot \
  --preset FirstRunAuth \
  --viewport 1440x900 \
  --scale-factor 1 \
  --mode Immediate \
  --settle-ms 200 \
  --output ./artifacts/first-run.png
```

Run `ferrex-player screenshot list` to print deterministic scenario names and
short descriptions. The command supports `--ice <PATH>` to replay an existing
`.ice` script before capture. When `.ice` metadata includes `preset`, `viewport`,
or `mode`, explicit CLI values must match it. If headless renderer initialization
fails, the command exits non-zero with suggested `WGPU_BACKEND`,
`WGPU_ADAPTER_NAME`, and software rendering environment variables.

### Theater Plate visual QA matrix

The screenshot harness includes deterministic Theater Plate detail presets for
balanced, bright, busy/text-like, low-detail, missing-backdrop, compact/tall, and
10-foot review. Write captures under `target/` only; PNG baselines are not
committed. For before/after review, run the same command set with `before/` and
`after/` output directories.

```bash
mkdir -p target/theater-plate-visual-qa/before target/theater-plate-visual-qa/after

nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateGood --viewport 1280x720 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/desktop-good-1280x720.png
nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateGood --viewport 1440x900 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/desktop-good-1440x900.png
nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateGood --viewport 1920x1080 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/desktop-good-1920x1080.png

nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateCompact --viewport 390x844 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/compact-390x844.png
nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateCompact --viewport 900x1600 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/tall-900x1600.png

nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateTenFoot --viewport 1280x720 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/tenfoot-1280x720.png
nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateTenFoot --viewport 1920x1080 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/tenfoot-1920x1080.png

nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateBright --viewport 1440x900 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/bright-1440x900.png
nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateBusyText --viewport 1440x900 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/busy-text-1440x900.png
nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateLowDetail --viewport 1440x900 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/low-detail-1440x900.png
nix develop .#ferrex-player --command env cargo run -p ferrex-player -- screenshot --preset TheaterPlateMissingBackdrop --viewport 1440x900 --scale-factor 1 --mode Immediate --settle-ms 200 --output target/theater-plate-visual-qa/after/missing-backdrop-1440x900.png
```

Reject a Theater Plate visual review if any capture looks like raw wallpaper,
shows a hard plate edge, makes title/metadata/action text unreadable, or leaves a
stale poster-depth artifact in the missing-backdrop fixture. If the renderer is
unavailable, record the command output and skip reason in the review packet.

## Validation commands

Use these commands after player crate graph or UI/app changes:

```bash
./scripts/check-player-crate-boundaries.sh
cargo fmt --all --check
nix develop --command env cargo check --workspace --all-targets
nix develop --command env cargo test -p ferrex-core --lib
cargo test -p ferrex-player-app --test ui_end_to_end
```

For focused extracted-domain checks, run the changed player crates directly, for
example:

```bash
cargo test -p ferrex-player-repository -p ferrex-player-library \
  -p ferrex-player-media -p ferrex-player-search -p ferrex-player-settings \
  -p ferrex-player-user-admin
```

## Wayland HDR note

Ferrex’s Wayland HDR path relies on the GStreamer 1.28 stable series. The Nix
and Flatpak packaging pin **GStreamer 1.28.4**; when building outside those
environments, use matching GStreamer and plugin development headers.

## Experimental in-process mpv

Build with `--features mpv` to enable the developer-only in-process backend.
The explicit **Play in MPV** action requests the compile-gated integrated
presenter on Windows/macOS and the native-window backend elsewhere; failed
presenter preflight/attachment falls back to native-window mode with a
structured reason. Auto continues to use the existing backend during
migration.

The reviewed macOS handoff bundle targets **macOS 15.0 or newer** on both
Apple Silicon and Intel. Its bundle metadata and every staged Mach-O load
command are audited against that floor; this handoff does not claim support
for older macOS releases.

Ferrex uses a deterministic mpv profile by default: standard user config,
scripts, and external URL resolvers are disabled. Controlled native OSC/input
bindings are enabled for native-window compatibility and disabled when Iced
owns an integrated controls overlay. For trusted local development only,
standard mpv config, `input.conf`, and scripts can be enabled explicitly:

```bash
FERREX_MPV_CONFIG_POLICY=trusted-user \
  cargo run -p ferrex-player --features mpv
```

mpv config and scripts execute inside the Ferrex process. Do not enable this
policy for untrusted configuration. Invalid policy values fail closed to the
deterministic profile. Playback diagnostics report the effective policy,
capability-gated external-subtitle/screenshot/shader/profile support, and only
the active shader count—never config contents, profile names, or local paths. Extension authors
should follow the
[native mpv extension API](https://ferrexmedia.org/developer/native-mpv-extension-api/)
for owner-thread, local-extension, raw-command, observation, and redaction
rules.

Native messages use a bounded verbose-at-startup then informational policy by
default. Diagnostic runs may select a fixed filter without changing playback
behavior:

```bash
FERREX_MPV_LOG_LEVEL=trace \
  RUST_LOG=ferrex_player_playback=trace,ferrex_player_mpv=trace \
  cargo run -p ferrex-player --features mpv
```

Accepted levels are `none`, `fatal`, `error`, `warn`, `info`, `verbose`,
`debug`, and `trace`. Invalid values fail closed without being echoed. Copied
messages remain credential/source-redacted, but traces can still reveal local
filenames or system topology and must be reviewed before sharing.

## Windows MPV override

If MPV auto-detection fails on Windows, set `FERREX_MPV_PATH` to the full path
to `mpv.exe`.

## Linux Flatpak

When distributed as a Flatpak bundle:

```bash
flatpak install --user ./ferrex-player*.flatpak
flatpak run io.github.lowband21.FerrexPlayer
```

The manifest enables the in-process mpv feature and bundles pinned mpv 0.41.0,
FFmpeg 8.1.2, and libplacebo with a build-time-asserted LGPL-only profile.
Wayland native-window mpv includes Vulkan, dmabuf, VA-API, PipeWire, and Pulse
support. mpv 0.41's X11 VO is GPL-only and is deliberately excluded from this
profile; Flatpak X11 sessions retain integrated GStreamer playback and the
separate external-player compatibility action.
