# Ferrex Demo Mode Guide

Ferrex ships with an optional **demo mode** that seeds a disposable media library, relaxes certain validation rules, and auto-configures demo credentials. This helps developers, QA, and prospective users explore Ferrex without sourcing real media.

---

## At a Glance
- **Synthetic library**: Generates movies and series folders/files with plausible names sourced from TMDB's popular lists.
- **Live metadata**: Requires a valid `TMDB_API_KEY` so the seeder can call TMDB's popular endpoints for both movies and TV shows.
- **Temporary storage**: Seeds directories under a demo root (defaults to `$CACHE_DIR/demo-media`); all files are zero-length placeholders.
- **Relaxed validation**: Scanner accepts zero-byte files for demo libraries (and only demo libraries) and skips expensive FFmpeg probes when configured, so the pipeline remains fast.
- **Isolated database**: Server automatically switches to a reserved `demo` database to avoid touching production data.
- **Auto-provisioned admin**: A demo user (`demo`/`demodemo` by default) is created and granted admin rights.
- **Player auto-login**: When built with the demo feature, the desktop player can enable remember-me using the demo credentials.
- **In-app demo controls** *(player build with `demo` feature)*: The library management controls view exposes demo-only sizing fields and scanner controls so you can manage resets and scan actions directly from the UI.
- **Admin controls**: REST endpoints expose demo status and allow resetting the synthetic tree at runtime.

---

## Enabling Demo Mode

### Build-time feature flags
- `ferrex-core`: enable `demo` feature.
- `ferrex-server`: enable `demo` feature (pulls in core’s implementations).
- `ferrex-player`: enable `demo` feature (optional; activates player-side conveniences).

Example workspace build:
```bash
cargo build --features ferrex-core/demo,ferrex-server/demo,ferrex-player/demo
```

### Server
Run with either CLI flag or environment variable:
```bash
# CLI
ferrex-server --demo

# or env
FERREX_DEMO_MODE=1 ferrex-server
```
Optional env overrides (in-progress implementation):
- `FERREX_DEMO_ROOT=/tmp/ferrex-demo` – fixed root path.
- `FERREX_DEMO_OPTIONS='{...json...}'` – complete JSON configuration (see **Configuration** below).
- `FERREX_DEMO_USERNAME` / `FERREX_DEMO_PASSWORD` – override default credentials.
- `FERREX_DEMO_ALLOW_DEVIATIONS`, `FERREX_DEMO_DEVIATION_RATE` – adjust imperfect-structure behavior.
- `FERREX_DEMO_MOVIE_COUNT`, `FERREX_DEMO_SERIES_COUNT` – quick sizing shortcuts.
- `TMDB_API_KEY=<key>` – **required**; without it demo mode will fail to generate structures.

### Player
Requires demo feature. Opt-in via env or CLI flag:
```bash
FERREX_PLAYER_DEMO_MODE=1 ferrex-player
# or
ferrex-player --demo
```

---

## Configuration Reference (unstable)
Demo generation is driven by `DemoSeedOptions`:
- `root`: optional filesystem root (Path). Defaults to `$CACHE_DIR/demo-media`.
- `libraries`: array of library definitions:
  - `library_type`: `Movies` or `Series`.
  - `name`: optional custom display name.
  - `movie_count`: number of movies (movies libraries only).
  - `series_count`: number of shows (series libraries only).
  - `seasons_per_series`: `(min,max)` inclusive range.
  - `episodes_per_season`: `(min,max)` inclusive range.
  - `allow_deviations`: override global deviation flag.
- `allow_deviations`: inject imperfect structures (missing episodes, odd folders).
- `deviation_rate`: 0.0–1.0 probability applied across deviations.
- `skip_metadata_probe`: skip FFmpeg metadata extraction (default `true`).
- `allow_zero_length_files`: relax size validation for fake files (default `true`).

Minimal JSON example:
```json
{
  "allow_deviations": true,
  "deviation_rate": 0.25,
  "libraries": [
    { "library_type": "Movies", "movie_count": 20 },
    {
      "library_type": "Series",
      "series_count": 5,
      "seasons_per_series": [1,3],
      "episodes_per_season": [6,10]
    }
  ]
}
```
Set via `FERREX_DEMO_OPTIONS`.

---

## Runtime Behavior
- **Filesystem seeding**: On startup, the server fetches TMDB popular lists, builds the directory tree, and creates zero-byte files according to the plan. Resetting will wipe and re-seed the tree with a fresh TMDB snapshot.
- **Database isolation**: If demo mode is active, the server rewrites `postgres://.../dbname` to `postgres://.../dbname_demo`. You may need to create the database upfront (`createdb dbname_demo`).
- **Library registration**: Generated libraries are inserted into the DB (or re-used if names match). Filesystem watchers are disabled for demo libs to avoid unnecessary noise.
- **Policy enforcement**: Scanner checks `demo::policy()` to bypass size checks and FFmpeg probes. Only files under registered demo libraries are affected.
- **User provisioning**: `demo` user is created automatically with admin role; credentials configurable via env. Perfect for staging or quick sales demos.
- **Admin REST API** (requires admin auth):
  - `GET /api/v1/admin/demo/status` – inspect current plan, counts, and root path.
  - `POST /api/v1/admin/demo/reset` – regenerate filesystem and re-sync libraries.

---

## Caveats & Notes
- Demo mode is **feature-gated**; production builds without the flag include no demo code.
- Placeholder files are zero-size; streaming/transcoding won’t work—this mode is for library UX/testing, not media playback.
- Do not rely on debug/demo database for real data; it is meant to be disposable.
- When demo mode is disabled, all behavior reverts to normal without lingering side effects (testing needed).

---

## Quick Commands Reference
```bash
# Start server in demo mode
cargo run -p ferrex-server --features demo -- --demo

# Start player in demo mode (separate terminal)
FERREX_SERVER_URL=https://localhost:3000 \
FERREX_PLAYER_DEMO_MODE=1 \
cargo run -p ferrex-player --features demo -- --demo

# Reset demo libraries via curl (requires admin token)
curl -X POST \
  -H "Authorization: Bearer <token>" \
  https://localhost:3000/api/v1/admin/demo/reset
```

Happy demoing!
