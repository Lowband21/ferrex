# Collections API integration notes

Collections use the shared DTOs in `ferrex_core::api::types::collections` and the route constants in `ferrex_core::api::routes::v1::collections`.

## Auth and routing

All collection endpoints are protected by the normal v1 authenticated route layer.

- `GET /api/v1/collections` lists collection summaries.
- `POST /api/v1/collections` creates a manual/dynamic collection definition. Non-admin requests are limited to private, user-scoped collections owned by the authenticated user; user-scoped requests that omit an owner are assigned to the authenticated user.
- `GET /api/v1/collections/{collection_id}` loads detail expansions.
- `PUT /api/v1/collections/{collection_id}` updates metadata/rules.
- `DELETE /api/v1/collections/{collection_id}` deletes a definition with a JSON `DeleteCollectionRequest` body.
- `POST /api/v1/collections/{collection_id}/archive` archives or restores a definition.
- `GET /api/v1/collections/{collection_id}/items` lists members.
- `POST /api/v1/collections/{collection_id}/items:manual-add` adds manual members.
- `POST /api/v1/collections/{collection_id}/items:manual-remove` removes manual members.
- `POST /api/v1/collections/{collection_id}/items:reorder` persists manual ordering.
- `POST /api/v1/collections/rules:validate` validates rule DSL payloads.
- `POST /api/v1/collections/rules:preview` previews dynamic rule results.
- `POST /api/v1/collections/{collection_id}/rule:refresh` refreshes materialized dynamic collections.
- `GET /api/v1/shelves/placements` lists shelf placements with flat query fields.
- `POST /api/v1/shelves/placements:pin` pins or unpins a collection.
- `POST /api/v1/shelves/placements:reorder` reorders shelf placements.
- `GET /api/v1/collections/tmdb/lists` lists local TMDB collection/list candidates with flat query fields.
- `POST /api/v1/collections/tmdb/import` imports or refreshes a TMDB-backed collection and requires admin access.

## Query contract

List requests use flat URL query fields so reqwest clients and Axum handlers agree:

```text
GET /api/v1/collections?cursor=25&limit=10&kind=manual&media_type=movie&include_item_counts=true
GET /api/v1/collections/{collection_id}/items?cursor=1&limit=25&availability=available
GET /api/v1/shelves/placements?surface=home&shelf_key=home.collections&include_unpinned=false
GET /api/v1/collections/tmdb/lists?cursor=0&limit=25&import_kind=collection
```

Responses return `page.total`, `page.limit`, and optional `page.next_cursor`. Cursor values are zero-based offsets encoded as strings.

## Revision and error contract

Mutating requests accept `expected_revision`. A stale revision maps to HTTP `409 Conflict`; invalid manual-operation payloads map to `400 Bad Request`; missing collections map to `404 Not Found`. Desktop callers should reload detail/items after a conflict and retry from the current revision.

## Availability and data preservation

Manual membership rows intentionally do not cascade to media files. Listing in edit/admin contexts preserves unavailable, missing, tombstoned, and archived members so the UI can show recovery/removal choices instead of silently dropping user-managed rows. Normal read contexts filter to available members unless an explicit availability filter is supplied.

## Validation evidence

- SQLx offline metadata is checked in for the collection queries added by the stack.
- Core repository coverage exercises rule validation, deterministic sorting/limits, availability filtering, preserved unavailable members, TMDB/person filtering, pagination, materialization, and revision conflicts.
- Server route coverage exercises auth, create/add/duplicate/list pagination, availability filters, stale revision conflicts, and archive routing when a PostgreSQL test database with create-database permissions is available.
- Desktop UI coverage is tracked in `docs/player-collections-tab-visual-qa.md`.
