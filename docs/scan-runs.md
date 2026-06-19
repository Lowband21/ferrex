# Durable library scan runs

Ferrex now has an explicit `library_scan_runs` contract for durable, idempotent library-level scan starts.

- `ScanRunMode::Manual` is the public default and preserves the existing user-triggered bulk library scan path.
- `run_key` is deterministic: `library:{library_id}:mode:{mode}`. The database keeps a partial unique index on active run keys (`pending`, `running`, `paused`) so terminal runs do not block a later scan for the same library and mode.
- Start responses keep the legacy `scan_id` and `correlation_id` fields and add `mode`, `status`, `idempotency_key`, `run_key`, and `disposition` (`created` or `reused`). Active scan snapshots add the same durable run identity fields while preserving scan-id-based SSE/subscription behavior.
- `orchestrator_jobs.correlation_id` persists the optional correlation supplied by `EnqueueRequest`, allowing restart recovery code to restore the original job/run correlation instead of relying only on the in-memory correlation cache.

The legacy `scan_state` table is intentionally not reused. It is not referenced by the current scan runtime, and its `scan_type`/`status` model describes an older resumability concept rather than the public library+mode single-flight contract. Keeping `library_scan_runs` separate avoids ambiguous migrations and lets follow-up runtime work adopt the durable contract explicitly.
