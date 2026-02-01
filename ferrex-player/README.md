# Ferrex Player

Desktop client for Ferrex (native UI + video backend).

Status: pre-alpha. Expect breaking changes.

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
