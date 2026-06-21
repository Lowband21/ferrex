# Ferrex Server Tests

End-to-end HTTP tests require a running server and external resources. They are gated behind the `e2e` feature and do not run by default.

Run E2E tests explicitly:

```bash
cargo test -p ferrex-server --features e2e -- --ignored
```

Notes:
- Tests target `http://localhost:3000` by default; ensure the server is running and matches expected routes.
- Keep `--ignored` so unit tests don’t block while E2E runs.

## Scanner regression suite

The scanner regression suite protects the runtime contracts for scan kickoff deduplication, downstream-aware scan completion, rehydrated scan recovery, persisted cursor completion replay, series dispatcher gating, and series bundle finalization. Run commands from the repository root; the canonical shell is used because it provides the Rust toolchain plus PostgreSQL binaries needed by the self-contained server scan test.

### Focused commands

Fast/in-process coverage with no external database requirement:

```bash
nix develop .#ferrex-player --command env cargo test -p ferrex-core --test scanner_runtime_testkit
nix develop .#ferrex-player --command env cargo test -p ferrex-core --lib domain::scan::actors::library::tests
nix develop .#ferrex-player --command env cargo test -p ferrex-server --lib infra::scan::scan_manager::tests
nix develop .#ferrex-player --command env cargo test -p ferrex-server --test scan_completion_regression -- --test-threads=1
```

DB-backed coverage requires `DATABASE_URL` pointing at a PostgreSQL database where the user can create/drop per-test databases. The `sqlx::test` cases apply `ferrex_core::MIGRATOR` automatically; no manual migration step is needed. Keep `SQLX_OFFLINE=true` so compile-time SQLx checks use the checked-in metadata while the tests use `DATABASE_URL` at runtime.

```bash
export DATABASE_URL="postgresql://postgres:password@localhost:5432/postgres?options=-csearch_path%3Dferrex,public"

nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL" cargo test -p ferrex-core --lib domain::scan::orchestration::dispatcher::tests -- --test-threads=1
nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL" cargo test -p ferrex-core --lib domain::scan::orchestration::persistence::tests::enqueue_reuses_ready_deferred_and_leased_dedupe_rows -- --test-threads=1
nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL" cargo test -p ferrex-core --lib domain::scan::orchestration::scan_run::tests -- --test-threads=1
nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL" cargo test -p ferrex-server --test scan_lifecycle -- --test-threads=1
nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL" cargo test -p ferrex-server --test mobile_media_flatbuffers series_bundle_finalization_emits_once_and_feeds_sync_fetch -- --test-threads=1
```

### CI and nextest notes

- These tests are ordinary Rust tests and can run under `cargo nextest run` with the same package/test filters once CI provisions the same `DATABASE_URL` contract.
- Keep DB-backed scanner commands serialized until a dedicated nextest profile assigns database-heavy tests to a constrained group.
- `scan_completion_regression` intentionally starts a temporary PostgreSQL process using `initdb`/`postgres` from the Nix shell. That keeps the focused server regression self-contained and avoids adding Docker/testcontainers dependencies.
- The scanner runtime testkit uses temporary directories, in-memory event buses, and fake actors/providers rather than TMDB, network, or media playback services, so it is suitable for fast CI lanes.
