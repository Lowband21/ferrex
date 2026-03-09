# Configuration

This guide explains how Ferrex is configured for local development and self‑hosting. It complements the quickstart in the README and the reference `.env.example`.

## Where Configuration Lives
- Generated environment file: `.env` in the project root (created by `just start` or `just config`).
- Example reference: `.env.example` (kept in repo).
- Derived assets and caches: `cache/`
- Optional demo seed data: `demo/` (when using demo mode)

Back up `.env` if you keep long‑lived credentials. The generator creates strong Postgres/Redis passwords.

## Core Environment Variables

These are the most commonly used variables. See `.env.example` for the authoritative list.

- `TMDB_API_KEY` – Required for metadata lookups.
- `SERVER_HOST` / `SERVER_PORT` – Bind address and port (defaults: `0.0.0.0` / `3000`).
- `FERREX_SERVER_URL` – The URL clients use to reach the server (e.g., `http://localhost:3000`).
- `DATABASE_URL` – Postgres connection URL (host/local use) plus `DATABASE_URL_CONTAINER` for in-container commands.
- `REDIS_URL` – Redis connection URL (plus `REDIS_URL_CONTAINER` for in-container access).
- `RUST_LOG` – Server logging filter, e.g. `sqlx=trace,ferrex=debug`.
- `FERREX_MPV_PATH` – Optional override for mpv path on Windows if auto‑detection fails.
- TLS options – Paths can be provided via env (if you terminate TLS at the app). If you use a reverse proxy, terminate TLS there instead.
- Player URL – Run the player against a custom server with `FERREX_SERVER_URL=https://host:port`.

## Generating Configuration

From the repo root:

```bash
# Generate/refresh config without starting services
just config

# Start the full stack (DB, Redis, ferrex-server)
just start
# (same as: ferrexctl stack up)
# Bring the stack down:
#   ferrexctl stack down

# Run the desktop player (release profile)
just run-player-release
```

## Compose Files / Overlays

- `docker-compose.yml` is the default self-host stack and pulls the published server image.
- `docker-compose.dev.yml` adds a local build of `ferrex-server` (used by `just` via `FERREX_COMPOSE_FILES`).
- `docker-compose.perf.yml` enables the Postgres performance preset (huge pages `try`, io_uring, larger buffers).

Unraid: see `docs/unraid.md`.

## Nix (NixOS)

This repo includes a flake for local development and for running the player with
a pinned Linux GStreamer build.

```bash
# dev shell
nix develop

# run player (NixOS-friendly)
nix run .#ferrex-player
```

## Profiles and Performance

Ferrex defines useful build profiles for faster iteration and improved runtime performance:

- Development: `just start --profile dev` (faster compile times)
- Priority: `just start --profile priority` (optimize workspace crates; recommended for the player)
- Release: `just run-player-release` or `just run-server-release`

The `ferrex-player` benefits noticeably from optimization.

## Tailscale Sidecar (single env file)

Run the stack with the Tailscale sidecar; no extra `.env` is required:

```bash
just start --mode tailscale
```

`just start --mode tailscale` automatically overrides the container endpoints to `127.0.0.1` for Postgres and Redis inside the shared Tailnet namespace while keeping your base `.env` intact.

## Logging

Control server verbosity via `--rust-log`:

```bash
just start --rust-log 'sqlx=trace,ferrex=debug'
```

Alternatively, set `RUST_LOG` directly in `.env`.

## Demo Mode (Optional)

Ferrex includes a feature‑gated demo mode that seeds disposable libraries for exploration and testing. See `docs/demo-mode.md` for full details and environment variables.

## Postgres Performance Configuration

Ferrex supports configurable Postgres performance presets for different hardware configurations:

### Presets

Use `FERREX_POSTGRES_PRESET` to select a predefined configuration:

- **`small`** (4-8GB RAM): shared_buffers=512MB, effective_cache_size=2GB, work_mem=16MB, max_connections=50
- **`medium`** (16-32GB RAM): shared_buffers=4GB, effective_cache_size=12GB, work_mem=64MB, max_connections=100
- **`large`** (64GB+ RAM): shared_buffers=16GB, effective_cache_size=48GB, work_mem=256MB, max_connections=200
- **`custom`**: Use individual environment variables (see below)

### Usage

```bash
# During initial setup
ferrexctl init --postgres-preset=medium

# Or set manually in .env
FERREX_POSTGRES_PRESET=medium
```

### Individual Overrides

You can override specific parameters regardless of preset:

- `FERREX_POSTGRES_SHARED_BUFFERS` - Shared memory for Postgres (e.g., "4GB")
- `FERREX_POSTGRES_EFFECTIVE_CACHE_SIZE` - OS cache estimate (e.g., "12GB")
- `FERREX_POSTGRES_WORK_MEM` - Per-operation memory (e.g., "64MB")
- `FERREX_POSTGRES_MAX_CONNECTIONS` - Max concurrent connections (e.g., "100")
- `FERREX_POSTGRES_SHM_SIZE` - Docker shm_size (e.g., "8g")
- `FERREX_POSTGRES_MAINTENANCE_WORK_MEM` - Maintenance operations memory
- `FERREX_POSTGRES_WAL_BUFFERS` - Write-ahead log buffers
- `FERREX_POSTGRES_HUGE_PAGES` - Huge pages support ("on" or "off")
- `FERREX_POSTGRES_MIN_WAL_SIZE` - Minimum WAL size
- `FERREX_POSTGRES_MAX_WAL_SIZE` - Maximum WAL size

Example with overrides:
```bash
FERREX_POSTGRES_PRESET=medium
FERREX_POSTGRES_SHARED_BUFFERS=8GB  # Override preset value
```

## TLS / HTTPS

Ferrex can terminate TLS directly. If you prefer a reverse proxy (nginx, Caddy, Traefik), terminate TLS there and run Ferrex over HTTP on localhost.

To enable HTTPS directly in Ferrex, set certificate and key paths:

```bash
TLS_CERT_PATH=/path/to/cert.pem
TLS_KEY_PATH=/path/to/key.pem
```

Advanced (optional):

- `TLS_MIN_VERSION` – Minimum TLS version to allow. Defaults to `1.3`.
  - `1.3` (recommended) or `1.2`.
- `TLS_CIPHER_SUITES` – Comma‑separated allow‑list of TLS 1.3 cipher suites.
  - Example: `TLS13_AES_256_GCM_SHA384,TLS13_CHACHA20_POLY1305_SHA256`

Notes:
- Default behavior is TLS 1.3 (Ferrex Player is the primary client).
- If you set `TLS_MIN_VERSION=1.3`, very old clients that only support TLS 1.2 will fail to connect — this is expected and desired for hardening.
- Certificate hot‑reload is supported: when `cert.pem`/`key.pem` contents change, the server reloads them (checked every ~5 minutes).

## Security Notes

- Ferrex is under active development; avoid exposing the server directly to the public Internet.
- Prefer running on an internal network, behind a reverse proxy, or via the Tailscale sidecar.
- See `.github/SECURITY.md` for the security policy.
