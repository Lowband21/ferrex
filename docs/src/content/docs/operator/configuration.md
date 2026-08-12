---
title: "Configuration"
description: "How Ferrex is configured for local development and self-hosted operation, including env files, Nix shells, scanner policy, logging, and TLS."
sidebar:
  order: 2
---

> **Drift-prevention:** This Starlight page is the canonical docs-site version. The legacy `docs/*.md` path now points here instead of carrying a second copy.

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

## Intelligence Runtime Variables

The Phase 2 intelligence runtime is configured with `FERREX_INTELLIGENCE_*` variables. The runtime is disabled by default; when enabled it calls the configured local/trusted OpenAI-compatible provider.

- `FERREX_INTELLIGENCE_ENABLED` – Enables grounded runtime execution when set to `true`.
- `FERREX_INTELLIGENCE_BASE_URL` – OpenAI-compatible provider base URL. Local development default: `http://localhost:8081/v1`.
- `FERREX_INTELLIGENCE_API_KEY` – Optional provider API key placeholder; leave blank for local providers that do not require a key.
- `FERREX_INTELLIGENCE_MODEL` – Optional model override. Local development default: `gemma-4-12b`.
- `FERREX_INTELLIGENCE_MODEL_TIMEOUT_MS`, `FERREX_INTELLIGENCE_TOOL_TIMEOUT_MS`, `FERREX_INTELLIGENCE_TOTAL_TIMEOUT_MS` – Model, tool, and whole-run timeout budgets in milliseconds.
- `FERREX_INTELLIGENCE_MAX_STEPS`, `FERREX_INTELLIGENCE_MAX_TOOL_CALLS` – Upper bounds for runtime loops.
- `FERREX_INTELLIGENCE_MAX_OUTPUT_BYTES`, `FERREX_INTELLIGENCE_MAX_TOOL_RESULT_BYTES` – Byte caps for generated output and persisted tool results.
- `FERREX_INTELLIGENCE_MAX_RETRIES` – Retry count for transient provider/runtime failures.
- `FERREX_INTELLIGENCE_PER_USER_CONCURRENCY` – Per-user concurrent run limit; additional active run starts for the same user receive a `concurrency_limit` response until a slot is free.

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

Unraid: see [Unraid](/operator/unraid/).

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

Scanner settings can be supplied in `scanner.toml`, `scanner.json`, `config/scanner.toml`, `config/scanner.json`, `SCANNER_CONFIG_PATH`, or inline JSON via `SCANNER_CONFIG_JSON`. New libraries default to bounded automatic maintenance every 15 minutes, filesystem watching is enabled per library, and the watcher uses `auto` strategy. Watch events remain the primary low-latency path; the shorter maintenance interval is the fallback for missed notifications and newly added top-level media folders. Existing libraries keep their persisted interval until an operator updates them. On Linux, `auto` selects polling immediately for CIFS/SMB3 and NFS mounts; other filesystems use native notifications with polling fallback.

Library create/update API payloads can override per-library policy:

```json
{
  "name": "Movies",
  "library_type": "Movies",
  "paths": ["/media/movies"],
  "scan_interval_minutes": 15,
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

[orchestrator.lease]
dispatch_timeout_ms = 1800000 # 30 minutes per job execution attempt
```

When a root has more eligible folders than a maintenance pass can enqueue, Ferrex rotates deterministic partitions on successive scan intervals. This rotation does not depend on the prior partition completing successfully, so folders beyond the configured bound are still revisited.

Job execution attempts are bounded to 30 minutes by default. When an actor or external dependency never returns, Ferrex stops renewing that attempt, releases its lease and workload capacity, and routes the job through the normal retry policy. Operators can lower `orchestrator.lease.dispatch_timeout_ms` for faster recovery, but it should remain above the longest expected scan, analysis, metadata, indexing, or image operation.

Network/container mount example (prefer bounded polling over unreliable native events):

Explicit `poll` remains useful for container mounts whose host filesystem type is hidden from the container. If `native` is forced on a detected CIFS/NFS mount, startup logs an operator warning because notifications may be unreliable.

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

### Optional transcript indexing

Transcript indexing is **off by default**. When enabled, Ferrex stores only redacted, bounded subtitle segments from supported text sources: sidecar SubRip/WebVTT (`.srt`, `.vtt`, `.webvtt`) files and text-convertible embedded subtitle streams reported by `ffprobe`/`ffmpeg`. Bitmap subtitle formats such as PGS/DVD subtitles are skipped. Redaction runs before persistence and search indexing; built-in patterns cover email addresses, phone-like numbers, URL query secrets, bearer/token assignments, and deployment-specific `custom_regexes`.

```toml
[orchestrator.transcript_indexing]
enabled = true
embedded_enabled = true
sidecar_enabled = true
allowed_languages = ["en", "es"] # empty means all detected languages
max_subtitle_bytes = 4194304
max_segments_per_media = 20000
max_chars_per_segment = 4000
max_chars_per_snippet = 320
extraction_timeout_ms = 15000
concurrency_budget = 1

[orchestrator.transcript_indexing.redaction]
enabled = true
redact_emails = true
redact_phone_numbers = true
redact_url_secrets = true
redact_bearer_tokens = true
custom_regexes = ["(?i)internal-case-[0-9]+"]
```

Operational controls:

- Retry extraction for a playable item: `POST /api/v1/media/{movie|episode}/{id}/refresh-transcripts`.
- Purge stored transcript sources/segments without deleting media files or non-transcript intelligence artifacts: `POST /api/v1/libraries/{library_id}/media/{movie|episode}/{id}/transcripts:purge` with `{"reason":"operator request"}`.
- Purge and request a rebuild when the media file is still available: `POST /api/v1/libraries/{library_id}/media/{movie|episode}/{id}/transcripts:rebuild`.
- Search snippets through `POST /api/v1/intelligence/timed-text:search`; snippet length is clamped by both request caps and `max_chars_per_snippet`.

Validation commands for transcript changes:

```bash
cargo fmt --all --check
nix develop .#ferrex-player --command env cargo check --workspace --all-targets
nix develop .#ferrex-player --command env cargo test -p ferrex-core --lib
DATABASE_URL=postgres://... cargo test -p ferrex-core --test transcript_repository
DATABASE_URL=postgres://... cargo test -p ferrex-server --test intelligence_routes transcript_purge_and_rebuild_routes_remove_searchable_segments
```

Invalid scanner config fails during startup with the field path in the error (for example, `scanner.orchestrator.watch.poll_interval_ms must be greater than 0`). Operators can inspect the effective policy and health counters via the scan config/metrics/status endpoints; these report watch strategy, poll/debounce/batch settings, the job dispatch deadline, transcript indexing controls, maintenance sweep policy, media/ignore filters, watcher registrations, replay lag, stale cursor counts, overflow events, and root-discovery truncation/deferred-entry counters.

## Logging

Control server verbosity via `--rust-log`:

```bash
just start --rust-log 'sqlx=trace,ferrex=debug'
```

Alternatively, set `RUST_LOG` directly in `.env`.

## Demo Mode (Optional)

Ferrex includes a feature‑gated demo mode that seeds disposable libraries for exploration and testing. See [Demo mode](/operator/demo-mode/) for full details and environment variables.

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
- See the [project security policy](/reference/project-policies/#security-policy) for the vulnerability disclosure policy.
- See [Authentication security model](/operator/auth-security/) for password login, remember-device, PIN login, auto-login, profile listing privacy, lockout, revoke, and recovery semantics.
