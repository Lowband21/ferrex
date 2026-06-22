---
title: "Intelligence foundation and Phase 2 runtime"
description: "Backend intelligence read models, bounded LLM tool contracts, local provider setup, draft-only runtime semantics, and privacy-safe transcript search."
sidebar:
  order: 7
---

> **Drift-prevention:** This Starlight page is the canonical docs-site version. The legacy `docs/intelligence-foundation.md` path now points here instead of carrying a second copy.

Ferrex intelligence is a backend-only safety boundary for local LLM features. Phase 1 added bounded read models, DTOs, and audit storage. Phase 2 adds a local provider boundary, a grounded tool loop, durable run events, draft artifact creation, and opt-in transcript snippets. It does **not** let a model run code, write arbitrary rows, promote artifacts, mutate playback/library state, or read full transcript bodies.

## Public backend contract

All public payloads are defined in `crates/ferrex-core/src/api/types/intelligence.rs` and routed by `crates/ferrex-server/src/handlers/intelligence.rs` / `crates/ferrex-server/src/routes/v1.rs`.

| Surface | Purpose |
| --- | --- |
| `POST /api/v1/intelligence/libraries/overview` | Bounded per-library counts, summaries, facets, and artifact ids. |
| `POST /api/v1/intelligence/facets` | Same bounded overview payload focused on facet consumers. |
| `POST /api/v1/intelligence/candidates:search` | Lexical candidate media search with grounding references, optional artifact ids, and optional transcript grounding. |
| `POST /api/v1/intelligence/timed-text:search` | Bounded timestamped transcript snippet search over redacted stored subtitle segments. |
| `POST /api/v1/intelligence/artifacts` / `POST /api/v1/intelligence/artifacts:search` | Bounded artifact summary search. |
| `GET /api/v1/intelligence/artifacts/{artifact_id}` | Bounded artifact detail summary; raw artifact `content` is not returned. |
| `POST /api/v1/intelligence/items/{media_id}/context` | Bounded item context packet with related items, artifacts, and grounding. |
| `POST /api/v1/intelligence/items/{media_id}/related` | Bounded related-item context for a seed media item. |
| `POST /api/v1/intelligence/runs` | Start an asynchronous grounded run when the runtime/provider is enabled. |
| `GET /api/v1/intelligence/runs/{run_id}` | Read run status, current phase, terminal state, summaries, and draft ids. |
| `GET /api/v1/intelligence/runs/{run_id}/events` | Replay ordered run events as SSE; `Last-Event-ID` resumes after a sequence. |
| `POST /api/v1/intelligence/runs/{run_id}:cancel` | Request cancellation for an in-flight run. |
| `POST /api/v1/intelligence/runs/{run_id}/audit` | Bounded run/tool-call audit summaries. |
| `GET /api/v1/intelligence/drafts` | List draft artifacts visible to the authenticated user, optionally by run. |
| `GET /api/v1/intelligence/drafts/{artifact_id}` | Fetch a draft payload, including persisted source edges, for its owner. |
| `GET /api/v1/intelligence/provider/status` | Report configured provider readiness and advertised models. |

`{media_id}` path parameters are encoded as `movie:<uuid>`, `series:<uuid>`, `season:<uuid>`, or `episode:<uuid>`; handlers also accept plural and parenthesized variants for compatibility.

## Safety bounds

Responses are designed for model-ready context assembly without exposing unbounded database or provider payloads:

- `IntelligencePagination` and `IntelligenceCaps` clamp page sizes, candidates, facets, related items, artifacts, grounding references, tool calls, summary lengths, and timed-text snippet budgets.
- `IntelligenceSummary` truncates on character boundaries and records whether truncation happened.
- Artifact APIs return `IntelligenceArtifactSummary` plus provenance/grounding; raw `intelligence_artifacts.content` JSON bodies remain out-of-band for public artifact summaries.
- Timed-text APIs return timestamped snippets only; whole transcript bodies, local paths, source content hashes, and command stderr never leave repository internals.
- Transcript text is redacted before `transcript_segments` persistence/search indexing and then clamped again by request caps plus the operator `max_chars_per_snippet` policy.
- User-scoped artifacts, watch-state rows, run audits, draft payloads, and transcript-derived artifacts are only visible to authorized users; global rows remain visible to authenticated users.
- Catalog/watch-state refresh and invalidation paths mark stale read-model rows and dependent artifacts invalidated instead of serving stale context.

## Runtime safety boundary

- The model only sees the approved Ferrex tool schemas plus the `final_response` action.
- Tool arguments are JSON-schema validated, library/user scoped, audited, redacted, and bounded by per-tool row/byte/time limits.
- The runtime rejects unapproved/direct-write actions such as shell, SQL, playlist writes, library edits, or arbitrary artifact promotion.
- Secrets in prompts, metadata, and tool audit payloads are redacted before persistence or model-visible summaries.
- Provider errors are mapped to stable `IntelligenceErrorCode` values; local `llama.cpp` providers can reject native tool options and Ferrex falls back to JSON-schema/prompt-only action selection.

## Approved model tool list

| Tool | Side effect | Purpose |
| --- | --- | --- |
| `library_overview` | Read-only | Bounded library counts, summaries, facets, and artifact ids. |
| `facets` | Read-only | Bounded facet groups for discovery planning. |
| `candidate_search` | Read-only | Search intelligence read models for candidate media. |
| `media_query` | Read-only | Run a bounded Ferrex media query with user/library scope. |
| `item_context` | Read-only | Context, related items, artifacts, and grounding for one media item. |
| `related_context` | Read-only | Related media context around a seed item. |
| `watch_context` | Read-only | User-scoped watch-state context. |
| `artifact_search` | Read-only | Search active artifact summaries. |
| `artifact_detail_sample` | Read-only | Sample artifact summaries without raw payload bodies. |
| `artifact_facets` | Read-only | Build facet counts from artifact summary samples. |
| `create_draft` | Draft write only | Create one scoped draft artifact with provenance sources. |
| `final_response` | Read-only runtime action | Finish the run after required tool/draft work is complete. |

## Grounding and draft semantics

The runtime maintains a grounding ledger from the seed request and every successful tool execution. Model output may only cite media ids or artifact ids that came from that ledger. Draft sources must cite the active run, known tool calls, or ledger-visible media/artifacts. Hallucinated media ids, invisible artifacts, or source edges from another run are rejected before `create_draft` executes.

`create_draft` writes `intelligence_artifacts.status = 'draft'` and persists `intelligence_artifact_sources`; it does not publish, promote, or update active artifacts. Drafts are user-scoped when a user starts the run and are only returned through the draft routes to that owner.

## Run lifecycle

1. `POST /runs` validates the prompt, checks provider readiness, creates a queued run, and starts the local runtime task.
2. The runtime records `queued`, `started`, tool, draft, completion/failure/cancellation events in sequence.
3. Clients poll `GET /runs/{run_id}` for terminal status or stream `GET /runs/{run_id}/events` with SSE resume support.
4. `POST /runs/{run_id}:cancel` requests cancellation; active model/tool calls receive a cancellation token and stale in-flight runs are marked terminal on server restart.
5. Draft ids from runtime events/status can be fetched through `GET /drafts/{artifact_id}`.

## Configuration

The runtime is disabled by default. Enable it only with a local or trusted OpenAI-compatible provider:

```bash
FERREX_INTELLIGENCE_ENABLED=true
FERREX_INTELLIGENCE_BASE_URL=http://localhost:8081/v1
FERREX_INTELLIGENCE_MODEL=gemma-4-12b
# Optional for providers that require it; omitted local providers use sk-noop.
FERREX_INTELLIGENCE_API_KEY=
```

Budget knobs are milliseconds/counts/bytes: `FERREX_INTELLIGENCE_MODEL_TIMEOUT_MS`, `FERREX_INTELLIGENCE_TOOL_TIMEOUT_MS`, `FERREX_INTELLIGENCE_TOTAL_TIMEOUT_MS`, `FERREX_INTELLIGENCE_MAX_STEPS`, `FERREX_INTELLIGENCE_MAX_TOOL_CALLS`, `FERREX_INTELLIGENCE_MAX_OUTPUT_BYTES`, `FERREX_INTELLIGENCE_MAX_TOOL_RESULT_BYTES`, `FERREX_INTELLIGENCE_MAX_RETRIES`, and `FERREX_INTELLIGENCE_PER_USER_CONCURRENCY`.

## Local `gemma-4-12b` smoke setup

Run an OpenAI-compatible `llama.cpp` server on the default Ferrex URL:

```bash
# Install/use your local llama.cpp build, then point at your GGUF path.
llama-server \
  --host 127.0.0.1 \
  --port 8081 \
  -m /path/to/gemma-4-12b-it.gguf \
  --ctx-size 8192

curl http://127.0.0.1:8081/v1/models
```

Then start Ferrex with `FERREX_INTELLIGENCE_ENABLED=true` and check `GET /api/v1/intelligence/provider/status` as an authenticated user. Keep real model smoke tests local; committed tests use deterministic fake providers.

## Transcript operations

Transcript extraction is an opt-in evidence source controlled by `[orchestrator.transcript_indexing]` in scanner config. Operators can enable sidecar and/or embedded extraction, restrict languages, bound subtitle bytes/segments/segment characters/snippet characters, set extraction timeout/concurrency, and configure built-in plus custom regex redaction. Runtime routes support:

- `POST /api/v1/media/{movie|episode}/{id}/refresh-transcripts` to enqueue a retry/refresh.
- `POST /api/v1/libraries/{library_id}/media/{movie|episode}/{id}/transcripts:purge` to remove transcript segment text and mark transcript source artifacts deleted without deleting media files or unrelated intelligence artifacts.
- `POST /api/v1/libraries/{library_id}/media/{movie|episode}/{id}/transcripts:rebuild` to purge and enqueue a rebuild when the media file is available.

## Internal storage and repository surfaces

The schema lives in `crates/ferrex-core/migrations/007_intelligence_foundation.sql`, `008_intelligence_runtime_ports.sql`, `009_timed_text_corpus.sql`, and `010_transcript_extract_jobs.sql`:

- `intelligence_media_context` and `intelligence_search_documents` hold bounded read-model context.
- `intelligence_artifacts` and `intelligence_artifact_sources` hold global/user artifacts, draft payloads, transcript source artifacts, and provenance edges.
- `intelligence_runs`, `intelligence_tool_calls`, and `intelligence_run_events` hold durable audit and replay state.
- `transcript_processing_status`, `transcript_sources`, `transcript_segments`, and `transcript_extract_jobs` hold redacted timed-text state, source manifests, searchable segments, and retryable extraction work.

Repository access is behind `crates/ferrex-core/src/database/repository_ports/intelligence.rs` and `crates/ferrex-core/src/database/repository_ports/transcripts.rs`; Postgres behavior is implemented in `crates/ferrex-core/src/database/repositories/intelligence.rs` and `crates/ferrex-core/src/database/repositories/transcripts.rs`. Important internal operations include read-model refresh (`refresh_library_read_models`, `refresh_media_read_model`), catalog invalidation (`invalidate_media_catalog_change`), artifact upsert/invalidation, candidate search, timed-text snippet search, transcript status updates, purge/invalidate, and run/tool-call audit reads.

## Validation commands

```bash
cargo fmt --all --check
nix develop .#ferrex-player --command env cargo check --workspace --all-targets
nix develop .#ferrex-player --command env cargo test -p ferrex-core --lib

# Server route contracts are DB-backed; use the per-worktree disposable SQLx database.
nix develop .#ferrex-player --command ./scripts/dev/sqlx-db.sh start
set -a; source .env.sqlx; set +a
nix develop .#ferrex-player --command env SQLX_OFFLINE=true DATABASE_URL="$DATABASE_URL_ADMIN" cargo test -p ferrex-server --test intelligence_routes -- --test-threads=1
```

Focused contract coverage lives in `ferrex-core` unit tests for provider fallback/malformed output, fake-provider queues, runtime success/failure, grounding, budgets, cancellation, redaction, draft/source persistence, transcript repositories, and scanner transcript orchestration, plus DB-backed `ferrex-server` route tests for authenticated start/status/SSE/cancel/draft flows, user-scope isolation, bounded transcript snippet search, and purge/rebuild behavior.

## Deferred work

The following remain outside this runtime/timed-text slice: `pgvector`/embedding ranking, semantic transcript ranking, client/UI presentation, generated transcript/OCR pipelines, and active-artifact promotion workflows.
