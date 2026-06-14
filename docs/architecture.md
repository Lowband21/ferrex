# Ferrex Architecture

This document gives a bird’s‑eye view of Ferrex’s components and how they fit together. It complements the top‑level README and links out to deeper docs where relevant.

## Workspace Layout

Ferrex is a Rust workspace with these primary crates:

- `ferrex-server` – HTTP/WS API, scanning/orchestration, persistence (Axum + Postgres + Redis).
- `ferrex-player` – Desktop client binary/facade that keeps the installed `ferrex-player` command name and historical imports.
- `ferrex-player-app` – Desktop application shell (bootstrap/config, state composition, root update/view/subscription wiring, Iced daemon/application construction, presets, and runtime hooks).
- `ferrex-player-ui` – Desktop UI/presentation crate (Iced views/widgets, Iced subscription/task adapters, design tokens, shader widgets/WGSL assets, and 10-foot surfaces).
- `ferrex-player-api` – Player-facing API adapters, service traits, and DTO re-exports used by desktop and future clients.
- `ferrex-player-auth`, `ferrex-player-repository`, `ferrex-player-library`, `ferrex-player-media`, `ferrex-player-metadata`, `ferrex-player-search`, `ferrex-player-settings`, and `ferrex-player-user-admin` – Extracted player data/domain crates. These crates avoid Iced/subwave runtime code (settings only shares `iced_core` color/point DTOs) and expose dependency-light state machines, selectors, streams, or contracts for the UI crate to adapt.
- `ferrex-player-playback` – Extracted playback/video domain crate that owns Subwave/MPV playback state, controls, and overlay helpers behind explicit UI-shell ports.
- `ferrex-player-foundation` – Dependency-light player primitives and contracts that must not depend on UI/video/domain crates.
- `ferrex-core` – Shared domain types, services, and orchestration runtime.
- `ferrex-model` – Shared data models and DTOs.
- `ferrex-contracts` – API contracts and schema glue.

Related docs:
- Scan/orchestration runtime details: `crates/ferrex-core/src/domain/scan/orchestration/runtime/README.md`
- Player specifics and platform notes: `crates/ferrex-player/README.md`
- Player crate dependency boundaries: `docs/player-dependency-boundaries.md`
- Demo mode: `docs/demo-mode.md`
- UI testing workflow: `docs/ui-testing-workflow.md`

Crate-boundary and baseline validation commands:

```bash
./scripts/check-player-crate-boundaries.sh
cargo fmt --all --check
nix develop .#ferrex-player --command cargo check --workspace --all-targets
nix develop .#ferrex-player --command cargo test -p ferrex-core --lib
```

## High‑Level Diagram

```
   +------------------------+            HTTP/WS             +------------------------+
   |     ferrex-player      |   <----------------------->    |     ferrex-server      |
   | (app shell + Iced UI)  |                                |   (Axum + Postgres)    |
   |                        |  watch stat, metadata, images  |                        |
   +-+--------------+-------+          +------+              +------+----------+------+
     |              |                  |      |                     |          |
     |              |                  |      |                     |          |
     v              v                  v      v                     v          v
  Appsink   Wayland subsurface    GStreamer pipeline            Postgres      Redis
  Player     (HDR zero-copy)      (decode/metadata)             (state)  (rate-limiting)

```

## Components

### Server (`ferrex-server`)
- Frameworks: Axum (HTTP/WS), SQLx (Postgres), Redis.
- Responsibilities:
  - Scan and index libraries, fetch metadata/artwork, derive and cache image variants.
  - Expose REST/WS endpoints for authentication, library content, watch progress/events.
  - Orchestrate background jobs (scan, analyze, enrich, index) with durable leases and retries.
- Orchestration runtime:
  - Worker pools per JobKind; jobs leased with TTL and renewed pre‑expiry.
  - Expired leases are resurrected by housekeeping.
  - Queue invariants: `state = 'ready'` and `available_at <= NOW()` gate eligibility; partial unique index on `dedupe_key` enforces de‑dup across relevant states.
  - See `crates/ferrex-core/src/domain/scan/orchestration/runtime/README.md` for specifics.

### Player (`ferrex-player` + `ferrex-player-app` + `ferrex-player-ui`)
- Binary/facade: `ferrex-player` keeps the installed command name and historical `ferrex_player::*` imports by delegating to `ferrex-player-app`.
- App shell: `ferrex-player-app` owns runtime bootstrap/config, state/domain composition, cross-domain routing surfaces, root update/view/subscription wiring, Iced daemon/application construction, presets, and logger/profiling hooks.
- UI/presentation: `ferrex-player-ui` owns the Iced views/widgets, Iced task/subscription adapters, design tokens, shader widgets, WGSL assets, and 10-foot surfaces.
- Data/API crates: `ferrex-player-auth`, `ferrex-player-repository`, `ferrex-player-library`, `ferrex-player-media`, `ferrex-player-metadata`, `ferrex-player-search`, `ferrex-player-settings`, `ferrex-player-user-admin`, and `ferrex-player-api` own player data-domain behavior and service contracts without pulling in Iced/subwave runtime code; settings only shares `iced_core` color/point DTOs. Async work crosses this boundary through `ferrex-player-foundation` domain tasks or dependency-light streams that the UI crate wraps.
- Playback/video: `ferrex-player-playback` owns Subwave/MPV playback state, controls, subscriptions, and overlay helpers behind explicit ports that `ferrex-player-ui` adapts.
- Video: subwave backend with platform‑optimized paths.
  - Wayland/HDR: GStreamer path with subsurfaces enables zero‑copy HDR output.
  - Other platforms: cross‑platform backend or mpv hand‑off with watch status tracking.
- Focus: smooth, low‑latency poster grids and animated navigation.

### Core (`ferrex-core`)
- Domain types and services shared across server and player.
- Orchestration runtime primitives (QueueService, EventBus, leases, and backoff).
- Long‑term: candidate surface for FFI (Swift/Kotlin) bindings.

### Video Backend (subwave)
- Evolved from `iced_video_player` toward a unified API for platform‑optimized rendering.
- Goals: frame pacing, zero‑copy where possible, and predictable latency under load.

### UI Stack (Iced fork)
- Tracks upstream Iced with targeted changes: primitive batching and Wayland subsurface support.
- Workspace pins Iced crates to the fork to ensure consistent behavior.

## Data & Messaging
- Transport: HTTP/WS between player and server; opaque access/refresh tokens (no JWT) returned on password login.
- Watch status: server tracks progress regardless of native or mpv playback.
- Caching: image variants and derived assets live under `cache/` on the server side.

## Security & Deployment Notes
- Active development: do not expose the server directly to the public Internet yet; prefer internal networks or a reverse proxy.
- For remote access, consider the Tailscale sidecar in `docker-compose.tailscale.yml`.
- See `.github/SECURITY.md` for the project security policy.
