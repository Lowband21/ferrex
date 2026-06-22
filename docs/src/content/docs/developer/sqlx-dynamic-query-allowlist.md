---
title: "SQLx dynamic query policy and allowlist"
description: "Policy for compile-checked SQLx macros, reviewed dynamic-query exceptions, and guard commands."
sidebar:
  order: 5
---

> **Drift-prevention:** This Starlight page is the canonical docs-site version. The legacy `docs/*.md` path now points here instead of carrying a second copy.

Ferrex keeps non-test application SQL on SQLx compile-checked macros wherever a statement is static and preparable. Repository/business queries must use `query!`, `query_as!`, `query_scalar!`, `query_file!`, or `migrate!` so type/schema drift is caught by SQLx offline metadata and CI.

Dynamic SQLx APIs are forbidden in non-test Rust code unless they are explicitly reviewed admin/DDL exceptions:

- `sqlx::query(...)`
- `sqlx::query_as(...)`
- `sqlx::query_scalar(...)`
- `sqlx::query_with(...)`
- `sqlx::raw_sql(...)`
- `sqlx::QueryBuilder`

The machine-readable source of truth is [`scripts/sqlx-dynamic-allowlist.toml`](https://github.com/Lowband21/ferrex/blob/dev/scripts/sqlx-dynamic-allowlist.toml). The guard fails on unallowlisted uses, stale allowlist entries, and expired temporary exceptions.

## Current approved exceptions

| Location | Statement source | Rationale |
| --- | --- | --- |
| `crates/ferrex-server/src/infra/postgres_tuning.rs` | `build_alter_system_statements` output executed by `apply_admin_tuning` | `ALTER SYSTEM SET ...` is administrative PostgreSQL utility SQL with runtime-selected setting values. It is only used on the admin tuning pool and is not a static, preparable application query. |
| `crates/ferrex-core/src/database/postgres.rs` | `tuning_statements` passed into `PostgresDatabase::new` and applied in `after_connect` | Per-connection `SET ...` tuning statements are runtime-generated from detected/overridden tuning parameters. PostgreSQL session `SET` utility statements cannot be parameterized as normal prepared SQLx macros. |

No repository/business read or write query is approved as an exception.

## Exception process

1. First try to express the SQL as a compile-checked SQLx macro and regenerate `.sqlx/` metadata.
2. Only request an exception for non-preparable PostgreSQL utility/admin/DDL SQL whose text must be generated at runtime.
3. Add exactly one `[[exceptions]]` entry to `scripts/sqlx-dynamic-allowlist.toml` for the source file and SQLx symbol.
4. Include a reviewer, a concrete rationale, and `scope = "permanent"` or `scope = "temporary"`.
5. Temporary exceptions must include an ISO `expires = "YYYY-MM-DD"`; the guard fails after that date.
6. Run the guard. Stale or overly broad entries must be removed before review.

## Commands

```bash
# Check only the dynamic SQLx policy/allowlist
just sqlx-dynamic-guard
# or:
./scripts/check-sqlx-dynamic-guard.py

# Run fixture tests for the guard implementation
just sqlx-dynamic-guard-self-test

# Run both required SQLx enforcement checks
just sqlx-enforcement
```

CI and the pre-push hook run the dynamic guard plus the SQLx offline prepare check. See [Disposable SQLx PostgreSQL workflow](/developer/sqlx-postgres-workflow/) for disposable database setup and cache regeneration.
