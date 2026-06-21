# SQLx dynamic query allowlist

Ferrex keeps application SQL on SQLx compile-checked macros wherever the statement is static and preparable. The remaining direct dynamic SQLx calls are limited to PostgreSQL tuning statements that are built from typed configuration values and cannot be represented as SQLx macros because the statement text is runtime-generated utility SQL.

| Location | Statement source | Rationale |
| --- | --- | --- |
| `crates/ferrex-server/src/infra/postgres_tuning.rs` | `build_alter_system_statements` output executed by `apply_admin_tuning` | `ALTER SYSTEM SET ...` is administrative PostgreSQL utility SQL with runtime-selected setting values. It is only used on the admin tuning pool and is not a static, preparable application query. |
| `crates/ferrex-core/src/database/postgres.rs` | `tuning_statements` passed into `PostgresDatabase::new` and applied in `after_connect` | Per-connection `SET ...` tuning statements are runtime-generated from detected/overridden tuning parameters. PostgreSQL session `SET` utility statements cannot be parameterized as normal prepared SQLx macros. |
