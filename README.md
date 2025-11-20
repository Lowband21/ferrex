# Ferrex
This repositor contains a performance first, self‑hosted media server and desktop player focused on responsive browsing and bleeding edge hardware accelerated video playback. It isn’t a cloud hosted service or web application, it’s a native first server and desktop player written in Rust, with custom render optimizations to keep latency spikes in check at high-refresh rates while bespoke animated poster cards bring your media to life.
Relative to mainstream media servers that optimize for remote access and client compatibility, Ferrex is purpose-built for desktop feel:
 - Scroll through high DPI posters and watch them animate smoothly into view as fast as client hardware can upload image textures to the GPU.
 - Enjoy virtually instant UI responsiveness as you browse, sort, filter, and search through expansive libraries
 - Live on the bleeding edge with Wayland and GStreamer HDR integrations.

## Who it's for:
Self‑hosters and all forms of technology enthusiasts who want a fluid and reliable desktop experience, and enjoy making the most of their hardware and media. Full HDR zerocopy video playback is currently wayland exclusive and requires a development build of gstreamer (1.27.X) for proper HDR metadata passthrough, while other platforms require the use of a secondary video player backend for internal playback. All platforms have the option to launch current media with mpv from the native playback interface while preserving position and tracking external watch progress.

## Getting Started:

- Install Docker + Docker Compose, Rust toolchain, and `just`.
- From the repository root run `just start`. This bootstraps configuration, generates strong Postgres credentials into `config/.env`, and launches Postgres, Redis, and the Ferrex server.
  - Use `just start --mode tailscale` to include the Tailscale sidecar.
  - For a clean separation between local and Tailnet configs:
    - Prepare a Tailnet config directory: `just config-tailnet from_dir="config" to_dir="config/tailnet"`
    - Start with: `just start --mode tailscale --config-dir config/tailnet`
    - Local mode continues to use `config/` (DB host `db`), while Tailnet uses `config/tailnet/` (DB host `127.0.0.1`).
  - Use `just start --config-dir config/prod` to generate an alternate configuration directory with its own `config/.env`.
  - Use `just start --profile dev` to build and launch with the unoptimized development Cargo profile, or use the `priority` profile to enable optimizations on workspace packages but not dependencies. Particularly useful for ferrex-player as Iced performs noticeably worse without build optimization.
  - Add `just start --rust-log 'sqlx=trace,ferrex=debug'` (or any [`RUST_LOG`](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) filter) to adjust container logging without editing config files.
  - Pass `just start --clean` to discard stale containers before restarting the stack.
- To regenerate config without starting the stack run `just init-config` (interactive) or `just config` (non-interactive), then `just start` to launch.
- Review `config/.env` for all server settings; keep it backed up as it contains generated passwords.

## Architecture:

- All three crates model their problem domains through DDD principles and patterns.
  - Server (Axum+Postgres) scans your media and fetches rich metadata and high resolution images for your libraries with durable, extensible and highly parallelized incremental scan orchestration that ensures work is completed efficiently across server interruptions both anticipated and not.
  - Player (Iced+GStreamer) renders large grids smoothly with animated posters and plays via either a Wayland-subsurface path (for native output/ HDR on Wayland) or a cross-platform backend, switchable during playback with position and settings preserved.
  - Core shared between both server and player, providing strong types for compile-time validated API and consolidated high-level behavior described by domain specific modules, traits and types. With the long-term plan being to adapt the core into an FFI bridge for use in the development of Swift and Kotlin mobile applications.
  - A separate video backend repository that I'm calling `subwave`, originally based on iced_video_player but long since far diverged, it aims to provide a unified API for platform optimized video rendering.
  - A fork of Iced master tracking closely the latest upstream changes alongside my primitive batching and wayland subsurface integration features. Both of which aim to avoid modifying Iced API surfaces, with primitive batching accomplishing that goal as of my last rebase on master, while the wayland integration still needs a similar treatment.

## Acknowledgements:
- Naturally, this project would never be possible without standing upon the shoulders of giants.
