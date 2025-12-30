# Ferrex Architecture

This document gives a bird’s‑eye view of Ferrex’s components and how they fit together. It complements the top‑level README and links out to deeper docs where relevant.

## Workspace Layout

Ferrex is a Rust workspace with these primary crates:

- `ferrex-server` – HTTP/WS API, scanning/orchestration, persistence (Axum + Postgres + Redis).
- `ferrex-player` – Desktop client (Iced + subwave video backend; GStreamer or mpv hand‑off).
- `ferrex-core` – Shared domain types, services, and orchestration runtime.
- `ferrex-model` – Shared data models and DTOs.
- `ferrex-contracts` – API contracts and schema glue.

Related docs:
- Scan/orchestration runtime details: `ferrex-core/src/domain/scan/orchestration/runtime/README.md`
- Player specifics and platform notes: `ferrex-player/README.md`
- Demo mode: `docs/demo-mode.md`
- UI testing workflow: `docs/ui-testing-workflow.md`

## High‑Level Diagram

```
   +------------------------+            HTTP/WS             +------------------------+
   |     ferrex-player      |   <----------------------->    |     ferrex-server      |
   |    (Iced + subwave)    |                                |   (Axum + Postgres)    |
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
  - See `ferrex-core/src/domain/scan/orchestration/runtime/README.md` for specifics.

### Player (`ferrex-player`)
- UI: Iced (custom fork pinned in workspace).
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
