# Scan observability operator workflow

Ferrex records scan runs in durable storage and mirrors the same information into the player scanner diagnostics panel. Use this workflow when a scan is slow, failed after retries, or needs to be correlated with server logs.

## Surfaces

- **Player UI:** Settings → Libraries → **Scanner diagnostics** shows scanner health, active runs, recent run history, selected-run timeline events, failure summaries, and recovery controls.
- **API:** `/api/v1/scan/health`, `/api/v1/scan/active`, `/api/v1/scan/runs`, `/api/v1/scan/runs/{id}`, `/api/v1/scan/runs/{id}/events`, `/api/v1/scan/runs/{id}/failures`, and `/api/v1/scan/recover`.
- **QA helper:** `scripts/qa/scan-observability-e2e.sh` starts a scan, polls live and durable state, prints IDs for log lookup, reads retained history, and optionally enqueues a recovery retry.

## End-to-end QA

Run against a local or staging server with a library that is safe to scan:

```bash
FERREX_SERVER_URL=http://127.0.0.1:3000 \
FERREX_LIBRARY_ID=<library-uuid> \
FERREX_TOKEN=<optional-bearer-token> \
scripts/qa/scan-observability-e2e.sh
```

The script verifies this sequence:

1. Starts a manual scan with an explicit correlation ID.
2. Reads scanner health and active scan state.
3. Reads latest live progress for the accepted scan ID.
4. Polls durable run detail until a terminal `completed`, `failed`, or `canceled` status.
5. Reads historical run list, run detail, event timeline, and failure summaries after terminal completion.
6. Prints the scan ID, correlation ID, and run key for log tracing.
7. If `FERREX_RECOVERY_PATH` is set, posts `/api/v1/scan/recover` for that owned path.

After the script completes, restart the server and re-run the printed `curl /api/v1/scan/runs/{scan_id}` command. The run detail, timeline, and failures should still be available until retention pruning removes terminal runs.

## Scanner health

Use `/api/v1/scan/health` or the player diagnostics health card to answer:

- Are folder/analyze/metadata/index/image queues backing up?
- How many active scans are registered?
- How many retained runs and failed runs are available for history lookup?
- Are filesystem watchers enabled and active for the expected libraries?

A health status of **Needs attention** means an operator should inspect queues, watcher errors, failed runs, and recent server logs before retrying work.

## Run timeline and failure categories

For a scan ID:

```bash
curl "$FERREX_SERVER_URL/api/v1/scan/runs/$SCAN_ID"
curl "$FERREX_SERVER_URL/api/v1/scan/runs/$SCAN_ID/events?limit=200"
curl "$FERREX_SERVER_URL/api/v1/scan/runs/$SCAN_ID/failures?limit=100"
```

Timeline events are sequence ordered and include status, current path, correlation ID, idempotency key, retry count, and failed-after-retries counts. Failure summaries are grouped by subject and category. Operator-facing categories include permission issues, missing paths, timeouts, no playable media, canceled work, and generic scan item failures.

The player must present failed-after-retries as **Failed**, **Needs attention**, or **Failed after retries** copy. Primary player/operator copy should not use internal dead-letter terminology; raw debug details are available only when explicitly requested from the API for diagnostics.

## Correlation IDs and logs

Every start/recovery request accepts a correlation ID and every durable run/event records one. Use it to connect UI/API state to server logs:

```bash
rg "$CORRELATION_ID" <server-log-dir>
rg "$SCAN_ID|$RUN_KEY" <server-log-dir>
```

When reporting a scan issue, include:

- `scan_id`
- `correlation_id`
- `run_key`
- terminal `status`
- last timeline `sequence`
- failure `category` and `message_code`

## Recovery and idempotency

Use `/api/v1/scan/recover` only for a path owned by the target library:

```bash
curl -X POST "$FERREX_SERVER_URL/api/v1/scan/recover" \
  -H 'Content-Type: application/json' \
  --data '{"library_id":"<library-uuid>","path":"/media/movies/Broken","correlation_id":"<uuid>"}'
```

Recovery enqueues a high-priority folder scan with merge/dedupe enabled. Repeating the same recovery while equivalent work is queued should merge into existing work instead of deleting user data or duplicating destructive actions. Recovery never removes media, watch state, or player cache entries; it only rechecks the target path and lets normal indexing update derived scan/media records.

## Retention and pruning

Terminal scan runs, events, and failure summaries are retained for historical lookup until retention pruning deletes terminal runs older than `scanner.orchestrator.maintenance.scan_run_retention_days` (30 days by default; set `0` to disable pruning). Active `pending`, `running`, and `paused` runs are not retention-prune targets. If an event cursor is older than retained events, the API returns replay-gap metadata with a recovery hint; reload the run detail or request events from the returned next sequence.

## Failure fixture checklist

To validate failed-after-retries display, scan a library path that is readable enough to be listed but contains a folder/file that consistently fails (for example, a permission-denied subfolder in a disposable test library). Expected evidence:

- `/api/v1/scan/runs/{id}` ends in `failed` with a needs-attention terminal summary.
- `/api/v1/scan/runs/{id}/events` includes retrying progress followed by a terminal failure event.
- `/api/v1/scan/runs/{id}/failures` includes an actionable category/message such as `filesystem_permission` / `scan.folder_permission_denied`.
- Player diagnostics shows failed-after-retries using operator-safe copy and exposes copyable scan/correlation IDs.
- Re-running recovery for the same path is safe and idempotent; user media and watch data remain intact.
