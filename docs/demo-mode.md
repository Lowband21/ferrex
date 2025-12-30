# Demo Mode

Demo mode is an optional, feature-gated way to explore Ferrex without real
media. The server generates a synthetic media tree and provisions a demo admin
user.

## Requirements

- Build the server with the `demo` feature.
- `TMDB_API_KEY` must be set (demo seeding calls TMDB).

## Run

Server (demo build):

```bash
cargo run -p ferrex-server --features demo -- --demo
```

Or via env:

```bash
FERREX_DEMO_MODE=1 cargo run -p ferrex-server --features demo
```

Player conveniences are also behind the `demo` feature (optional):

```bash
cargo run -p ferrex-player --features demo -- --demo
```

## Defaults / Overrides

- Demo root: `FERREX_DEMO_ROOT` (defaults to `<cache_root>/demo-media` if unset).
- Demo database: `ferrex_demo` (override via `DEMO_DATABASE_NAME`).
- Demo credentials: `demo` / `demodemo` (override via `FERREX_DEMO_USERNAME`,
  `FERREX_DEMO_PASSWORD`).
- Size shortcuts: `FERREX_DEMO_MOVIE_COUNT`, `FERREX_DEMO_SERIES_COUNT`
  (advanced: `FERREX_DEMO_OPTIONS` JSON).

## Admin API (requires admin auth)

- `GET /api/v1/admin/demo/status`
- `POST /api/v1/admin/demo/reset`
- `POST /api/v1/admin/demo/resize`

## Caveats

- Demo files are zero-byte placeholders; playback is not the goal.
- Demo mode intentionally isolates state (filesystem + DB); do not enable it on
  a real media dataset.
