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
color/point DTOs for accent-color state. See
`../../docs/player-dependency-boundaries.md` for the guard policy and intentional
compatibility shims.

## Build prerequisites

- Rust 1.90+ (workspace MSRV)
- Linux builds require GStreamer + FFmpeg development headers. The CI workflow
  shows the current package list used for builds.

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

## Validation commands

Use these commands after player crate graph or UI/app changes:

```bash
./scripts/check-player-crate-boundaries.sh
cargo fmt --all --check
nix develop .#ferrex-player --command cargo check --workspace --all-targets
nix develop .#ferrex-player --command cargo test -p ferrex-core --lib
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

Ferrex’s Wayland HDR path relies on the GStreamer 1.27.x development series.
Pinned to **GStreamer 1.27.2** for now (newer 1.27.x builds have known regressions
that haven’t been addressed yet).

## Windows MPV override

If MPV auto-detection fails on Windows, set `FERREX_MPV_PATH` to the full path
to `mpv.exe`.

## Linux Flatpak

When distributed as a Flatpak bundle:

```bash
flatpak install --user ./ferrex-player*.flatpak
flatpak run io.github.lowband21.FerrexPlayer
```
