<div align="center">

# Ferrex

<p><em>Native media server + desktop player focused on zero‑copy HDR on Wayland and low‑latency animated browsing.</em></p>
</div>

![player grid](https://media.lowband.me/images/grid_fallback.png)

<p align="center">
  <img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.90%2B-orange?logo=rust&logoColor=white&style=flat" />
  <img alt="Rust edition" src="https://img.shields.io/badge/edition-2024-orange?logo=rust&logoColor=white&style=flat" />
  <a href="#license"><img alt="License" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-informational?style=flat" /></a>
  <a href="https://ferrexmedia.org/"><img alt="Docs" src="https://img.shields.io/badge/docs-ferrexmedia.org-0078D4?style=flat" /></a>
  <a href="https://github.com/Lowband21/ferrex/actions/workflows/ci.yml">
    <img alt="CI" src="https://img.shields.io/github/actions/workflow/status/Lowband21/ferrex/ci.yml?branch=main&label=CI&logo=githubactions&style=flat" />
  </a>
  <a href="https://github.com/Lowband21/ferrex/graphs/commit-activity">
    <img alt="Commit activity" src="https://img.shields.io/github/commit-activity/m/Lowband21/ferrex?style=flat" />
  </a>
</p>

## What is Ferrex?
A Rust‑native media server and player focused on delivering a smooth and low latency experience with hardware‑accelerated playback. Ferrex isn’t a cloud service or web app—it’s a tightly integrated native server + desktop player optimized for high‑refresh UI, zero‑copy video, and smooth animated poster grids.

- Feels local, because it is: batched rendering of custom UI primitives keeps latency spikes in check as you fling through high‑DPI posters.
- Zero‑copy HDR on Wayland: a Wayland‑subsurface path uses current GStreamer stable HDR support to preserve metadata and avoid expensive copies.
- Pragmatic elsewhere: playback runs behind a backend-neutral contract with
  GStreamer rollback, an opt-in in-process libmpv native-window path, and an
  explicit process-isolated mpv handoff.

Status: pre-alpha (0.1.0-alpha). Expect rapid changes while core surfaces continue to stabilize.

## Why it exists

Existing home media tools are flexible but often not fast in the ways that feel satisfying and enjoyable to use. Ferrex is an experiment in interactive performance as a first class feature.

## Who it’s for

Self‑hosters and performance‑minded enthusiasts who value a fluid desktop experience and want to make use of their hardware efficiently—especially on Wayland, where full HDR zero‑copy playback relies on the GStreamer 1.28 stable series for correct HDR metadata passthrough (tested with **GStreamer 1.28.4**). Windows and macOS have explicit in-process libmpv presenter handoff builds while GStreamer/external-mpv remain the rollback policy; Auto, HDR, and hardware-decoding capability claims stay gated on representative native-output evidence.

## Highlights

- Responsive UI across sorting, filtering, and searching large libraries.
- Animated poster grids that stream in as fast as your GPU can swallow textures.
- Keyboard driven and animated UI navigation/scrolling.
- Wayland HDR pipeline with a subsurface strategy tailored for native output.
- In-process libmpv native-window playback and an external mpv handoff, both
  with backend-neutral watch status and redacted diagnostics.

## Quickstart

### Docker/Podman

- Docker + Docker Compose
- `TMDB_API_KEY` (required for metadata; leave blank to disable)

1) Create `.env` from the template and set at least `MEDIA_ROOT`:

```bash
cp .env.example .env
${EDITOR:-nano} .env
```

2) Start the stack (Postgres, Redis, Ferrex server):

```bash
docker compose up -d
```

Optional performance preset (huge pages + io_uring + larger Postgres buffers):

```bash
docker compose -f docker-compose.yml -f docker-compose.perf.yml up -d
```

Unraid note: see the [Unraid deployment notes](https://ferrexmedia.org/operator/unraid/) for recommended paths and `PUID`/`PGID` support.

### Development (build from source)

Nix users should start with the canonical, player-capable shell:

```bash
nix develop
# equivalent alias for existing workflows:
nix develop .#ferrex-player
```

Use the lean server/core shell when you only need backend crates and want to
skip GStreamer/GPU inputs:

```bash
nix develop .#server
```

Without Nix, install Rust stable 1.90+, just, and Linux GStreamer + FFmpeg
development headers (see the CI workflow for the current package list).

```bash
just start
# equivalent: ferrexctl stack up
```

### And the player:

```bash
just run-player-release
```

More options (profiles, logging, tailscale, host vs docker server): see [Configuration](https://ferrexmedia.org/operator/configuration/) and the [Contributing Guide](.github/CONTRIBUTING.md).

## Packaging and Release

Ferrex provides `ferrexctl` commands for packaging and release automation:

```bash
# Run preflight checks (fmt, clippy, tests, deny, audit)
ferrexctl package preflight --scope=workspace

# Create a release (builds binary, docker image, GitHub release)
ferrexctl package release-init 0.1.0-alpha --dry-run

# Package for Windows (cross-compilation with GStreamer bundling)
ferrexctl package windows --target=x86_64-pc-windows-gnu

# Build Flatpak bundle
ferrexctl package flatpak
```

See `ferrexctl --help` for all packaging options.

## Platform Support

- Linux / Wayland: primary target. Zero‑copy HDR pipeline via GStreamer 1.28 stable and Wayland subsurfaces.
  - Tested environment: Arch Linux (Hyprland WM). Please report results for GNOME/KDE/wlroots compositors.
  - Player specifics and platform notes: see [crates/ferrex-player/README.md](crates/ferrex-player/README.md).

- Other platforms: the cross-platform GStreamer path remains the current Auto
  policy. An mpv-enabled developer/release build can explicitly request
  in-process native-window playback; the separate external action remains a
  crash-isolated compatibility handoff.

### Compatibility

| Platform | Current Auto/integrated path | Explicit mpv path | Evidence-qualified status |
|---|---|---|---|
| Linux (Wayland) | GStreamer 1.28 subsurface | In-process native window or external process | HYBRID. GStreamer HDR/zero-copy and mpv `gpu-next`/hwdec have platform evidence; integrated mpv is deferred. |
| Linux (X11) | Integrated GStreamer | External process only in the reviewed package | HYBRID. mpv 0.41 X11 VO is excluded from the LGPL-only in-process build. |
| Windows | GStreamer rollback | Compile-gated Win32 owned-overlay presenter; native-window/external fallback | Representative-system handoff ready; Auto, HDR, hwdec, taskbar/focus/fullscreen, and stress gates remain open. |
| macOS | GStreamer rollback | Compile-gated AppKit in-root `NSView` presenter; native-window/external fallback | Representative-system handoff ready; Auto, HDR/EDR, VideoToolbox, Spaces/fullscreen, and stress gates remain open. |

See [Desktop playback backends](https://ferrexmedia.org/developer/desktop-playback-backends/)
for build selection, deterministic fallback order, diagnostics, platform
limitations, and rollback. The implementation specification and live rollout
checklist are linked from the [architecture page](https://ferrexmedia.org/developer/architecture/).

## Security notes

Ferrex is under active development.

- Prefer running on an internal network, behind a reverse proxy, or via the Tailscale sidecar.
- Avoid exposing the server directly to the public Internet for now.

See [Security Policy](.github/SECURITY.md) for details.

## Architecture

See [Architecture](https://ferrexmedia.org/developer/architecture/) for the diagram and component responsibilities (server, player, core, video backend, and UI stack).

## Configuration

See [Configuration](https://ferrexmedia.org/operator/configuration/) for options and workflows, and [`.env.example`](.env.example) for the authoritative reference of environment variables.

## FAQ

See the [FAQ](https://ferrexmedia.org/operator/faq/). Public documentation is built from the Starlight source under [`docs/src/content/docs/`](docs/src/content/docs/).

## Development

See the [Contributing Guide](.github/CONTRIBUTING.md) for local setup, commands, and contribution guidelines.

Dependency updates are handled by Dependabot weekly (Mon 04:00 UTC) across the Cargo workspace, GitHub Actions, and Dockerfiles in `docker/`. Updates are grouped to keep PR noise low—details in the Contributing Guide.

## Roadmap

See the [Changelog](CHANGELOG.md) for highlights and open issues/discussions for upcoming work.

## Contributing

Please read the [Contributing Guide](.github/CONTRIBUTING.md) and [Code of Conduct](.github/CODE_OF_CONDUCT.md) before opening PRs.

## License

Licensed under MIT OR Apache‑2.0.

## Acknowledgements

Standing on the shoulders of giants—especially the Iced and GStreamer communities, whose work makes native UI and high‑fidelity video possible.

Attribution: This product uses the TMDB API but is not endorsed or certified by TMDB. See [Trademarks](.github/TRADEMARKS.md).
