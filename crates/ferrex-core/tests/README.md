# Ferrex Core Test Suite

## Overview

The core test suite contains fast, deterministic unit tests and DB-backed integration tests that use `#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]`. The migrator ensures schema is applied before each test.

## Prerequisites (PostgreSQL)

`#[sqlx::test]` for Postgres requires a reachable Postgres instance indicated by `DATABASE_URL`. The macro will create per-test databases and apply migrations automatically.

Example local setup:

```bash
export DATABASE_URL="postgresql://postgres:password@localhost:5432/postgres"
```

Use an account with permission to create/drop databases.

## Running Tests

Run all core tests:

```bash
cargo test -p ferrex-core
```

Run a single test file:

```bash
cargo test -p ferrex-core --test orchestration
```

## Scanner Runtime Regressions

The scanner regression suite combines the reusable deterministic testkit with focused runtime checks in `ferrex-core` and `ferrex-server`. See `crates/ferrex-server/tests/README.md` for the complete command list, DB-backed cases, and CI/nextest guidance.

The core-only entry points are:

```bash
nix develop .#ferrex-player --command env cargo test -p ferrex-core --test scanner_runtime_testkit
nix develop .#ferrex-player --command env cargo test -p ferrex-core --lib domain::scan::actors::library::tests
```

DB-backed scanner queue/dispatcher checks require `DATABASE_URL` with create/drop database privileges and `SQLX_OFFLINE=true` for compile-time SQLx metadata:

```bash
nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL" cargo test -p ferrex-core --lib domain::scan::orchestration::dispatcher::tests -- --test-threads=1
nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL" cargo test -p ferrex-core --lib domain::scan::orchestration::persistence::tests::enqueue_reuses_ready_deferred_and_leased_dedupe_rows -- --test-threads=1
nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL" cargo test -p ferrex-core --lib domain::scan::orchestration::scan_run::tests -- --test-threads=1
```

## Notes

- Tests with `#[sqlx::test]` are isolated and run against ephemeral databases managed by the macro.
- `watch_status_library_series_continue` also provisions an isolated database per test. When a localhost `DATABASE_URL` is unavailable, it starts a temporary PostgreSQL from the Nix dev shell and applies the same migrator.
- No manual `cargo sqlx migrate` is required for tests; the migrator runs automatically.
