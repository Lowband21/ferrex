<div align="center">

# Ferrex

<p><em>Native media server + desktop player focused on zero‑copy HDR on Wayland and low‑latency animated browsing.</em></p>
</div>

https://github.com/user-attachments/assets/e7b42e2f-59fa-4347-a5f8-cc49192d5d41

<p align="center">
  <img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.90%2B-orange?logo=rust&logoColor=white&style=flat" />
  <img alt="Rust edition" src="https://img.shields.io/badge/edition-2024-orange?logo=rust&logoColor=white&style=flat" />
  <a href="#license"><img alt="License" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-informational?style=flat" /></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux%20%2F%20Wayland-informational?logo=linux&style=flat" />
  <img alt="Native" src="https://img.shields.io/badge/Native-Server%20%2B%20Player-2c3e50?style=flat" />
  <img alt="UI" src="https://img.shields.io/badge/UI-Iced%20(fork)-5865f2?style=flat" />
  <img alt="HDR" src="https://img.shields.io/badge/HDR-zero--copy%20(wayland)-blueviolet?style=flat" />
  <img alt="GStreamer" src="https://img.shields.io/badge/GStreamer-1.27.x-0a7?style=flat" />
  <img alt="Postgres" src="https://img.shields.io/badge/Postgres-enabled-336791?logo=postgresql&logoColor=white&style=flat" />
  <img alt="Redis" src="https://img.shields.io/badge/Redis-enabled-DC382D?logo=redis&logoColor=white&style=flat" />
  <img alt="Docker Compose" src="https://img.shields.io/badge/Docker-Compose-informational?logo=docker&style=flat" />
  <img alt="PRs welcome" src="https://img.shields.io/badge/PRs-welcome-brightgreen?style=flat" />
  <img alt="Status" src="https://img.shields.io/badge/status-active--development-yellow?style=flat" />
</p>


<p align="center">
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
- Zero‑copy HDR on Wayland: a Wayland‑subsurface path makes use of bleeding edge GStreamer HDR developments to preserve metadata and avoid expensive copies.
- Pragmatic elsewhere: on other platforms, Ferrex can hand off to mpv.

Status: active development. Expect rapid changes while core surfaces continue to stabilize.

## Why it exists

Existing home media tools are flexible but often not fast in the ways that feel satisfying and enjoyable to use. Ferrex is an experiment in interactive performance as a first class feature.

## Who it’s for

Self‑hosters and performance‑minded enthusiasts who value a fluid desktop experience and want to make use of their hardware efficiently—especially on Wayland, where full HDR zero‑copy playback shines (currently requires a development build of GStreamer 1.27.x for correct HDR metadata passthrough). Windows and macOS may utilize mpv hand‑off or the alternate player backend that does not include any HDR passthrough or tone-mapping.

## Highlights

- Responsive UI across sorting, filtering, and searching large libraries.
- Animated poster grids that stream in as fast as your GPU can swallow textures.
- Keyboard driven and animated UI navigation/scrolling.
- Wayland HDR pipeline with a subsurface strategy tailored for native output.
- mpv hand‑off with watch status tracking maintained.

## Screenshots / Demo

- Fastest way to try it: see [Demo Mode](docs/demo-mode.md) to seed a disposable library.
- Screenshots and short clips will be added soon.

## Quickstart

### Build Prerequisites

- Docker + Docker Compose
- Rust toolchain (stable 1.90+, edition 2024)
- just (https://github.com/casey/just)
- mpv (optional; currently required on windows for playback)

### Start the stack

```bash
# from repo root
just start
```

### And the player:

```bash
just run-player-release
```

This bootstraps configuration, generates strong Postgres credentials into `config/.env`, and launches Postgres, Redis, and the Ferrex server. Keep `config/.env` backed up—it contains generated passwords.

More options (profiles, logging, Tailscale, alternate config dirs): see [Configuration](docs/configuration.md) and the [Contributing Guide](.github/CONTRIBUTING.md).

## Platform Support

- Linux / Wayland: primary target. Zero‑copy HDR pipeline via GStreamer (dev 1.27.x) and Wayland subsurfaces.
  - Tested environment: Arch Linux (Hyprland WM). Please report results for GNOME/KDE/wlroots compositors.
  - Player specifics and platform notes: see [ferrex-player/README.md](ferrex-player/README.md).

- Other platforms: playback via the cross‑platform backend or "Open with MPV" from detail views.

### Compatibility

| Platform          | Playback path              | HDR passthrough | Zero‑copy | Status             |
|-------------------|----------------------------|-----------------|-----------|--------------------|
| Linux (Wayland)   | GStreamer + subsurface     | Yes (1.27.x)    | Yes       | Primary, supported |
| Linux (Xorg)      | Alt backend / mpv hand‑off | No              | No        | Works, less ideal  |
| Windows           | Alt backend / mpv hand‑off | No (today)      | No        | Experimental       |
| macOS             | Alt backend / mpv hand‑off | No (today)      | No        | Experimental       |

## Security notes

Ferrex is under active development.

- Prefer running on an internal network, behind a reverse proxy, or via the Tailscale sidecar.
- Avoid exposing the server directly to the public Internet for now.

See [Security Policy](.github/SECURITY.md) for details.

## Architecture (bird’s‑eye)

See [Architecture](docs/architecture.md) for the diagram and component responsibilities (server, player, core, video backend, and UI stack).

## Configuration

See [Configuration](docs/configuration.md) for options and workflows, and [`config/.env.example`](config/.env.example) for the authoritative reference of environment variables.



## FAQ

See the [FAQ](docs/faq.md).

## Known Issues

Track and report issues at: https://github.com/Lowband21/ferrex/issues

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
