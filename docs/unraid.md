# Unraid

Ferrex targets Docker-first self-hosting, and Unraid is a great fit. The smoothest
path today is running the provided Compose stack (server + Postgres + Redis).

## Recommended: Compose stack

1) Copy these files into a persistent app folder, e.g.:

- `/mnt/user/appdata/ferrex/docker-compose.yml`
- `/mnt/user/appdata/ferrex/docker-compose.perf.yml` (optional)
- `/mnt/user/appdata/ferrex/.env` (create from `.env.example`)

2) Edit `.env`:

- `MEDIA_ROOT=/mnt/user/media` (or wherever your library lives)
- `TMDB_API_KEY=...` (optional but recommended)

Optional (permissions / typical Unraid template style):

- `PUID=99`
- `PGID=100`
- `UMASK=0022`

3) Bring it up:

```bash
cd /mnt/user/appdata/ferrex
docker compose up -d
```

### Perf overlay (optional)

If your host supports huge pages and you want io_uring + larger Postgres buffers:

```bash
docker compose -f docker-compose.yml -f docker-compose.perf.yml up -d
```

Notes:
- `docker-compose.perf.yml` sets very large Postgres memory defaults (e.g.
  `shared_buffers=16GB`) and `/dev/shm` size. Adjust to your machine.
- Huge pages are host-managed. Ferrex’s Postgres config uses `huge_pages=try`
  so it will fall back safely when not configured.

## Alternative: single-container template

Ferrex depends on Postgres + Redis. If you prefer Unraid Community Applications,
you can run Ferrex as a single container and point it at separate Postgres/Redis
containers, but Compose remains the most turnkey path.
