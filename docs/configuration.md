# Configuration

This guide explains how Ferrex is configured for local development and self‑hosting. It complements the quickstart in the README and the reference `.env.example`.

## Where Configuration Lives
- Generated environment file: `config/.env` (created by `just start` or `just config`).
- Example reference: `config/.env.example` (kept in repo).
- Derived assets and caches: `cache/`
- Optional demo seed data: `demo/` (when using demo mode)

Back up `config/.env` if you keep long‑lived credentials. The generator creates strong Postgres/Redis passwords.

## Core Environment Variables

These are the most commonly used variables. See `config/.env.example` for the authoritative list.

- `TMDB_API_KEY` – Required for metadata lookups.
- `FERREX_BIND` – Server bind address, e.g. `0.0.0.0:3000` (default HTTP port is 3000).
- `DATABASE_URL` – Postgres connection URL.
- `REDIS_URL` – Redis connection URL.
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

# Run the desktop player (release profile)
just run-player-release
```

## Profiles and Performance

Ferrex defines useful build profiles for faster iteration and improved runtime performance:

- Development: `just start --profile dev` (faster compile times)
- Priority: `just start --profile priority` (optimize workspace crates; recommended for the player)
- Release: `just run-player-release` or `just run-server-release`

The `ferrex-player` benefits noticeably from optimization.

## Tailscale Sidecar and Alternate Config Dirs

Run the stack with a Tailscale sidecar:

```bash
just start --mode tailscale
```

Use a separate Tailnet config directory and switch modes:

```bash
just config-tailnet from_dir="config" to_dir="config/tailnet"
just start --mode tailscale --config-dir config/tailnet
```

Conventions:
- Local mode uses `config/` (DB host `db` inside the Compose network).
- Tailnet mode uses `config/tailnet/` (DB host `127.0.0.1`).
- To use a different configuration directory entirely: `just start --config-dir config/prod`.

## Logging

Control server verbosity via `--rust-log`:

```bash
just start --rust-log 'sqlx=trace,ferrex=debug'
```

Alternatively, set `RUST_LOG` directly in `config/.env`.

## Demo Mode (Optional)

Ferrex includes a feature‑gated demo mode that seeds disposable libraries for exploration and testing. See `docs/demo-mode.md` for full details and environment variables.

## Security Notes

- Ferrex is under active development; avoid exposing the server directly to the public Internet.
- Prefer running on an internal network, behind a reverse proxy, or via the Tailscale sidecar.
- See `.github/SECURITY.md` for the security policy.
