#!/usr/bin/env bash
set -euo pipefail

# End-to-end scan observability QA helper.
#
# Required:
#   FERREX_SERVER_URL  Base URL, for example http://127.0.0.1:3000
#   FERREX_LIBRARY_ID  Library UUID to scan
# Optional:
#   FERREX_TOKEN       Bearer token for authenticated servers
#   FERREX_TIMEOUT_SECONDS  Poll timeout for terminal status (default: 300)
#   FERREX_RECOVERY_PATH    Path owned by the library to enqueue as a recovery retry
#
# The script starts a manual scan, observes live state, waits for a terminal
# durable run, reads the retained timeline/failures, and optionally exercises
# the idempotent recovery enqueue path. It prints correlation IDs and endpoint
# payload paths so operators can match API state with server logs.

SERVER_URL=${FERREX_SERVER_URL:?set FERREX_SERVER_URL, e.g. http://127.0.0.1:3000}
LIBRARY_ID=${FERREX_LIBRARY_ID:?set FERREX_LIBRARY_ID to a library UUID}
TIMEOUT_SECONDS=${FERREX_TIMEOUT_SECONDS:-300}
TOKEN=${FERREX_TOKEN:-}
RECOVERY_PATH=${FERREX_RECOVERY_PATH:-}

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 2
  }
}

require curl
require jq
require python3

AUTH_ARGS=()
if [[ -n "$TOKEN" ]]; then
  AUTH_ARGS=(-H "Authorization: Bearer $TOKEN")
fi

api() {
  local method=$1
  local path=$2
  local body=${3:-}
  if [[ -n "$body" ]]; then
    curl -fsS "${AUTH_ARGS[@]}" \
      -H 'Content-Type: application/json' \
      -X "$method" \
      --data "$body" \
      "$SERVER_URL$path"
  else
    curl -fsS "${AUTH_ARGS[@]}" \
      -H 'Content-Type: application/json' \
      -X "$method" \
      "$SERVER_URL$path"
  fi
}

uuid() {
  python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
}

started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
correlation_id=$(uuid)
start_body=$(jq -n --arg correlation_id "$correlation_id" \
  '{correlation_id: $correlation_id, mode: "manual"}')

echo "==> Starting scan for library $LIBRARY_ID"
start_response=$(api POST "/api/v1/libraries/$LIBRARY_ID/scans:start" "$start_body")
echo "$start_response" | jq .

scan_id=$(jq -r '.data.scan_id // .scan_id' <<<"$start_response")
accepted_correlation=$(jq -r '.data.correlation_id // .correlation_id' <<<"$start_response")
run_key=$(jq -r '.data.run_key // .run_key // empty' <<<"$start_response")

echo "scan_id=$scan_id"
echo "correlation_id=$accepted_correlation"
echo "run_key=$run_key"
echo "started_at=$started_at"

echo "==> Reading scanner health and active scans"
api GET "/api/v1/scan/health" | jq .
api GET "/api/v1/scan/active" | jq .

echo "==> Reading latest progress snapshot"
api GET "/api/v1/scan/progress?scan_id=$scan_id" | jq . || true

echo "==> Waiting up to ${TIMEOUT_SECONDS}s for terminal durable run"
deadline=$((SECONDS + TIMEOUT_SECONDS))
terminal_status=""
while (( SECONDS < deadline )); do
  run_payload=$(api GET "/api/v1/scan/runs/$scan_id" || true)
  if [[ -n "$run_payload" ]]; then
    terminal_status=$(jq -r '.data.run.status // .run.status // empty' <<<"$run_payload")
    sequence=$(jq -r '.data.run.sequence // .run.sequence // empty' <<<"$run_payload")
    completed=$(jq -r '.data.run.completed_items // .run.completed_items // empty' <<<"$run_payload")
    total=$(jq -r '.data.run.total_items // .run.total_items // empty' <<<"$run_payload")
    echo "status=$terminal_status sequence=$sequence progress=$completed/$total"
    case "$terminal_status" in
      completed|failed|canceled) break ;;
    esac
  fi
  sleep 5
done

if [[ "$terminal_status" != "completed" && "$terminal_status" != "failed" && "$terminal_status" != "canceled" ]]; then
  echo "scan did not reach a terminal state before timeout" >&2
  exit 1
fi

ends_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo "==> Terminal run detail ($terminal_status)"
echo "$run_payload" | jq .

echo "==> Historical runs for library"
api GET "/api/v1/scan/runs?library_id=$LIBRARY_ID&limit=10" | jq .

echo "==> Timeline events"
api GET "/api/v1/scan/runs/$scan_id/events?limit=200" | jq .

echo "==> Failure summaries (if any)"
api GET "/api/v1/scan/runs/$scan_id/failures?limit=100" | jq .

if [[ -n "$RECOVERY_PATH" ]]; then
  recovery_correlation=$(uuid)
  recovery_body=$(jq -n \
    --arg library_id "$LIBRARY_ID" \
    --arg path "$RECOVERY_PATH" \
    --arg correlation_id "$recovery_correlation" \
    '{library_id: $library_id, path: $path, correlation_id: $correlation_id}')
  echo "==> Enqueueing recovery retry for $RECOVERY_PATH"
  api POST "/api/v1/scan/recover" "$recovery_body" | jq .
  echo "recovery_correlation_id=$recovery_correlation"
fi

cat <<EOF
==> Operator follow-up
- Search server logs for correlation_id=$accepted_correlation and scan_id=$scan_id.
- In the player, open Settings > Libraries > Scanner diagnostics and verify:
  * health counters match /api/v1/scan/health,
  * the run appears in recent history with status $terminal_status,
  * timeline events/failures are visible without primary dead-letter wording,
  * copy buttons expose scan_id and correlation_id.
- If testing restart/reload, restart the server now and rerun:
  curl ${TOKEN:+-H "Authorization: Bearer ***"} "$SERVER_URL/api/v1/scan/runs/$scan_id"
  The retained detail/events/failures should still be available until retention pruning.
started_at=$started_at
ended_at=$ends_at
EOF
