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
# (same as: ferrex-init stack up)
# Bring the stack down:
#   ferrex-init stack down

# Run the desktop player (release profile)
just run-player-release
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
