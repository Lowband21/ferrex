---
title: "Disposable SQLx PostgreSQL workflow"
description: "Per-worktree PostgreSQL workflow for SQLx migrations and offline metadata updates without touching shared databases or secrets."
sidebar:
  order: 6
---

> **Drift-prevention:** This Starlight page is the canonical docs-site version. The legacy `docs/*.md` path now points here instead of carrying a second copy.

Ferrex has a first-class local PostgreSQL workflow for agents and contributors who need a live database for SQLx migrations or query metadata updates without touching `.env`, Docker/Podman, or any shared database.

The workflow uses the PostgreSQL binaries from the Nix dev shells, including the `pg_uuidv7` extension, and writes a per-worktree cluster under `${XDG_STATE_HOME:-$HOME/.local/state}/ferrex/sqlx-db/<worktree-hash>/`. Connections use a Unix socket, local trust auth, no passwords, and an empty `PGPASSFILE` so existing secrets are not consulted.

## Enter a Nix shell

```bash
nix develop .#server
# or, when validating player/UI changes too:
nix develop .#ferrex-player
```

Both shells provide PostgreSQL with `pg_uuidv7` and `sqlx-cli` (`cargo sqlx`).

## Commands

From the repository root:

```bash
# Start or reuse this worktree's PostgreSQL cluster and write .env.sqlx
./scripts/dev/sqlx-db.sh start

# Show cluster state, socket path, and generated env file path
./scripts/dev/sqlx-db.sh status

# Apply Ferrex migrations to the disposable database
./scripts/dev/sqlx-db.sh migrate

# Start, migrate, and refresh SQLx offline metadata
./scripts/dev/sqlx-db.sh prepare

# Check checked-in SQLx metadata without connecting to a database
./scripts/dev/sqlx-db.sh prepare-check

# Stop the cluster but keep data and .env.sqlx
./scripts/dev/sqlx-db.sh stop

# Recreate an empty migrated cluster for this worktree
./scripts/dev/sqlx-db.sh reset

# Stop and delete the cluster plus .env.sqlx
./scripts/dev/sqlx-db.sh destroy
```

Equivalent `just` recipes are available as `just sqlx-db-start`, `just sqlx-db-status`, `just sqlx-db-migrate`, `just sqlx-db-prepare`, `just sqlx-db-prepare-check`, `just sqlx-db-stop`, `just sqlx-db-reset`, and `just sqlx-db-destroy`.

## Dynamic SQLx policy checks

Non-test repository/business queries must use SQLx compile-checked macros. Dynamic SQLx APIs are reserved for reviewed non-preparable PostgreSQL admin/DDL exceptions listed in `scripts/sqlx-dynamic-allowlist.toml` and described in [SQLx dynamic query policy and allowlist](/developer/sqlx-dynamic-query-allowlist/).

Run the policy guard after touching Rust database code:

```bash
just sqlx-dynamic-guard
# or:
./scripts/check-sqlx-dynamic-guard.py
```

Run the complete local SQLx enforcement stack before pushing:

```bash
just sqlx-enforcement
```

That composite target runs the dynamic guard and the offline SQLx prepare/cache check.

## Regenerating and checking `.sqlx/` cache

Use the disposable database when query macros or migrations change:

```bash
nix develop .#server
./scripts/dev/sqlx-db.sh prepare
# review .sqlx/ changes, then verify without a live DB:
./scripts/dev/sqlx-db.sh prepare-check
```

`prepare` starts the per-worktree cluster, applies migrations, grants local describe privileges, and refreshes checked-in `.sqlx/*.json` metadata. `prepare-check` unsets ambient database variables and runs SQLx in offline mode, so it is safe for CI, hooks, and agents.

## Using the generated env file

`start`, `migrate`, `prepare`, and `reset` write `.env.sqlx` with safe local values:

- `DATABASE_URL` for the app role
- `DATABASE_URL_ADMIN` for migration/admin actions
- `PGHOST`, `PGPORT`, `PGDATABASE`, `PGUSER`, `PGSSLMODE`, and an empty `PGPASSFILE`

To run a command manually against the disposable database:

```bash
set -a
source .env.sqlx
set +a
cargo run -p ferrex-server
```

Do not copy these URLs into `.env`; rerun `./scripts/dev/sqlx-db.sh start` in each worktree instead. `.env.sqlx` is ignored by version control, and the PostgreSQL state lives outside the worktree so Nix flake evaluation never sees live socket files.

## Safety guarantees

- `prepare` may add transient public shadow tables inside the disposable cluster so SQLx can describe legacy `public.*` test queries while the application schema remains `ferrex`-first.
- The script never reads `.env` and overwrites any inherited `DATABASE_URL` with generated Unix-socket URLs before invoking SQLx.
- Live database commands refuse ambient non-local or prod-looking `DATABASE_URL`/`DATABASE_URL_ADMIN` values before proceeding.
- Generated URLs are validated to target this worktree's socket directory, expected database name, expected roles, and `sslmode=disable`; URLs containing passwords are rejected.
- PostgreSQL is configured with `listen_addresses = ''`, so the disposable cluster does not listen on TCP.
- `stop`, `reset`, and `destroy` clean up stale sockets and PID files only inside the marked per-worktree state directory.
