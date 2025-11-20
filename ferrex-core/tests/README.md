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

## Notes

- Tests with `#[sqlx::test]` are isolated and run against ephemeral databases managed by the macro.
- No manual `cargo sqlx migrate` is required for tests; the migrator runs automatically.
