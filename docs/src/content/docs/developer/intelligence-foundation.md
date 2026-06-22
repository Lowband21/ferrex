---
title: "Phase 1 intelligence foundation"
description: "Backend-only intelligence read models, bounded DTO contracts, and audit storage for future Ferrex LLM features."
sidebar:
  order: 7
---

> **Drift-prevention:** This Starlight page is the canonical docs-site version. The legacy `docs/intelligence-foundation.md` path now points here instead of carrying a second copy.

Phase 1 adds backend-only intelligence read models, bounded DTO contracts, and audit storage that future LLM features can use safely. It is intentionally a **non-model** foundation: no model provider is invoked, no prompt/tool loop is executed, and no UI surface is introduced in this phase.

## Public backend contract

All public payloads are defined in `crates/ferrex-core/src/api/types/intelligence.rs` and routed by `crates/ferrex-server/src/handlers/intelligence.rs` / `crates/ferrex-server/src/routes/v1.rs`.

| Surface | Purpose |
| --- | --- |
| `POST /api/v1/intelligence/libraries/overview` | Bounded per-library counts, summaries, facets, and artifact ids. |
| `POST /api/v1/intelligence/facets` | Same bounded overview payload focused on facet consumers. |
| `POST /api/v1/intelligence/candidates:search` | Lexical candidate media search with grounding references and optional artifact ids. |
| `POST /api/v1/intelligence/artifacts` | Bounded artifact summary search. |
| `POST /api/v1/intelligence/artifacts:search` | Alias for bounded artifact summary search. |
| `GET /api/v1/intelligence/artifacts/{artifact_id}` | Bounded artifact detail summary; raw artifact `content` is not returned. |
| `POST /api/v1/intelligence/items/{media_id}/context` | Bounded item context packet with related items, artifacts, and grounding. |
| `POST /api/v1/intelligence/items/{media_id}/related` | Bounded related-item context for a seed media item. |
| `POST /api/v1/intelligence/runs/{run_id}/audit` | Bounded run/tool-call audit summaries. |

`{media_id}` path parameters are encoded as `movie:<uuid>`, `series:<uuid>`, `season:<uuid>`, or `episode:<uuid>` (the handler also accepts plural and parenthesized variants for compatibility).

## Safety bounds

Phase 1 responses are designed for model-ready context assembly without exposing unbounded database or provider payloads:

- `IntelligencePagination` and `IntelligenceCaps` clamp page sizes, candidates, facets, related items, artifacts, grounding references, tool calls, and summary lengths.
- `IntelligenceSummary` truncates on character boundaries and records whether truncation happened.
- Artifact APIs return `IntelligenceArtifactSummary` plus provenance/grounding; the raw `intelligence_artifacts.content` JSON body remains out-of-band.
- User-scoped artifacts, watch-state rows, and run audits are only visible to the owning user; global rows remain visible to authenticated users.
- Catalog/watch-state refresh and invalidation paths mark stale read-model rows and dependent artifacts invalidated instead of serving stale context.

## Internal storage and repository surfaces

The schema lives in `crates/ferrex-core/migrations/007_intelligence_foundation.sql` and creates:

- `intelligence_media_context` for bounded per-media context rows.
- `intelligence_search_documents` for bounded lexical search documents (`tsvector` + `pg_trgm`).
- `intelligence_artifacts` and `intelligence_artifact_sources` for global/user artifacts and provenance edges.
- `intelligence_runs` and `intelligence_tool_calls` for durable audit skeletons.

Repository access is behind `crates/ferrex-core/src/database/repository_ports/intelligence.rs`; Postgres behavior is implemented in `crates/ferrex-core/src/database/repositories/intelligence.rs`. Important internal operations include read-model refresh (`refresh_library_read_models`, `refresh_media_read_model`), catalog invalidation (`invalidate_media_catalog_change`), artifact upsert/invalidation, candidate search, item/related context, and run/tool-call audit reads.

## Deferred work

The following are explicitly deferred beyond Phase 1:

- `pgvector`/embedding storage and vector ranking.
- Transcript segment persistence and transcript-derived artifacts.
- Model-provider configuration, prompt execution, streaming responses, and tool-loop orchestration.
- Client/UI presentation for intelligence features.

Until those land, Phase 1 remains a bounded backend contract and storage foundation that can be validated through core SQLx fixtures and server route wiring.
