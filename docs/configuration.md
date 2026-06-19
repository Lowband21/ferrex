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
a pinned Linux GStreamer build. `nix develop` is the canonical development shell
and includes the player-capable GStreamer/GPU environment. The historical
`ferrex-player` shell remains as an alias for existing scripts.

```bash
# canonical player-capable dev shell
nix develop

# backward-compatible alias
nix develop .#ferrex-player

# lean server/core shell without player-only GStreamer/GPU inputs
nix develop .#server

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

## Scanner / Incremental Scans

Scanner settings can be supplied in `scanner.toml`, `scanner.json`, `config/scanner.toml`, `config/scanner.json`, `SCANNER_CONFIG_PATH`, or inline JSON via `SCANNER_CONFIG_JSON`. Existing installs that do not provide a scanner config keep safe defaults: libraries auto-scan every 60 minutes, filesystem watching is enabled per library, and the watcher uses `auto` strategy with native notifications falling back to polling.

Library create/update API payloads can override per-library policy:

```json
{
  "name": "Movies",
  "library_type": "Movies",
  "paths": ["/media/movies"],
  "scan_interval_minutes": 60,
  "auto_scan": true,
  "watch_for_changes": true
}
```

Local filesystem example (low latency native watching):

```toml
video_extensions = ["mkv", "mp4", "avi", "mov", "webm", "m4v"]
ignored_extensions = ["tmp", "part"]

[orchestrator.watch]
strategy = "auto"          # auto | native | poll
debounce_window_ms = 250
max_batch_events = 8192
poll_interval_ms = 30000

[orchestrator.maintenance]
enabled = true
tick_interval_ms = 60000
max_jobs_per_library = 128
max_root_entries_per_library = 512
```

Network/container mount example (prefer bounded polling over unreliable native events):

```toml
video_extensions = ["mkv", "mp4", "mpeg", "ts"]
ignored_extensions = ["tmp", "part", "download"]
ignored_path_patterns = ["**/.staging/**"]

[orchestrator.watch]
strategy = "poll"
poll_interval_ms = 120000
debounce_window_ms = 1000
max_batch_events = 2048
poll_backoff_max_ms = 600000

[orchestrator.maintenance]
enabled = true
tick_interval_ms = 300000
max_jobs_per_library = 64
max_root_entries_per_library = 256
```

Invalid scanner config fails during startup with the field path in the error (for example, `scanner.orchestrator.watch.poll_interval_ms must be greater than 0`). Operators can inspect the effective policy and health counters via the scan config/metrics/status endpoints; these report watch strategy, poll/debounce/batch settings, maintenance sweep policy, media/ignore filters, watcher registrations, replay lag, stale cursor counts, and overflow events.

The stable Movies/Series layout contract and diagnostic code taxonomy are documented in `docs/scanner-layout-contract.md` and codified in `ferrex_core::domain::scan::manifest`.

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
- See `.github/SECURITY.md` for the vulnerability disclosure policy.
- See `docs/auth-security.md` for password login, remember-device, PIN login, auto-login, profile listing privacy, lockout, revoke, and recovery semantics.
