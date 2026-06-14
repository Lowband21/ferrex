# Orchestrator runtime overview

This runtime supervises single-process workers that consume jobs from a `QueueService` and publish lifecycle events on an `EventBus`.

Key points:
- Worker pools per JobKind (FolderScan, MediaAnalyze, MetadataEnrich, IndexUpsert)
- Leases: jobs are leased with a TTL and renewed before expiry.
- Renewals: default TTL is 30s; renew when half elapsed or when <2s remain (renew_min_margin_ms).
- Housekeeping periodically rescans for expired leases and resurrects eligible jobs.
- Actors (library, folder, etc.) are kept resident to accept commands; they are not removed after Start.

Queue invariants:
- Eligibility is controlled by `state = 'ready'` and `available_at <= NOW()`.
- Deduplication uses a partial unique index on `dedupe_key` for states (ready, deferred, leased).
- Exponential backoff for retryable failures updates only `available_at`.

Notes:
- For fairness across libraries, consider a dequeue path using the `(library_id, priority, available_at, created_at)` index when hotspots emerge.
- Background image variant generation is disabled for now; callers should use get_or_download_variant or handle None from ensure_variant_async.
