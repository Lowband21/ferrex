# Desktop Collections tab visual QA notes

Issue coverage: LOW-647 added the base Collections tab; LOW-649 added manual editing flows; LOW-650 validates the integrated stack.

## Automated coverage

- `ferrex-player-ui` integration test `collections_tab.rs` builds the Collections tab from `TestApiService` summaries, verifies tab selection state transitions, and renders loaded list/detail surfaces with stubbed collection data.
- The same test module covers detail item pagination, unavailable-member presentation, rule/materialization status summaries, retry/error states, manual create/update/add/remove/reorder/archive actions, duplicate handling, stale revision conflicts, and conflict recovery reloads.
- `ferrex-player-api` collection tests cover shared route constants, DTO round trips, and in-memory stub behavior used by desktop UI tests.

## Manual desktop QA checklist

Use a desktop player session connected to a server with at least one manual collection and at least one collection containing unavailable or tombstoned members.

1. Header navigation
   - The top header shows `Home`, `Collections`, and enabled library tabs.
   - Selecting `Collections` highlights only that tab and does not disturb Home/library scroll state.
   - Selecting Home or a library tab restores that tab's previous scroll position.
2. Collections listing
   - Loading, empty, error, retry, and refreshing states are visible and readable.
   - Collection rows show title, description, media scope, kind/source/visibility/status badges, presentation, duplicate policy, item count, artwork/theme text, and materialization/stale state.
   - Keyboard focus reaches the Collections tab and collection rows via normal button traversal.
3. Detail navigation
   - Activating a collection row opens the detail view.
   - Back returns to the Collections listing.
   - Detail shows metadata, materialization status, shelf/rule status, item preview, and unavailable-member badges without dropping preserved memberships.
4. Manual editing
   - Creating a manual collection opens a collection row/detail with revision `0` and readable empty state.
   - Metadata edits save title/description/media-scope changes, then refresh the detail revision.
   - Adding an existing media item reports a duplicate result instead of creating a second row.
   - Removing and reordering items update the detail list without losing keyboard focus context.
   - Stale revision conflicts show the recovery action, reload detail/items, and clear conflict flags after fresh data arrives.
   - Archiving removes the collection from the default list and appears only when archived collections are included.

## Current visual QA status

Automated render smoke and interaction coverage is in place. Full manual visual QA still requires a live desktop player session with server-side collection fixtures and preserved unavailable/tombstoned membership data.
