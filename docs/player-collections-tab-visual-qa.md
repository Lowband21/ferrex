# Desktop Collections tab visual QA notes

Issue: LOW-647 — Add desktop Collections tab navigation and listing

## Automated coverage

- `ferrex-player-ui` integration test `collections_tab.rs` builds the Collections tab from `TestApiService` summaries, verifies tab selection state transitions, and renders the loaded list/detail surfaces with stubbed collection data.

## Manual desktop QA checklist

Use a desktop player session connected to a server with at least one manual or imported collection.

1. Header navigation
   - The top header shows `Home`, `Collections`, and enabled library tabs.
   - Selecting `Collections` highlights only that tab and does not disturb Home/library scroll state.
   - Selecting Home or a library tab restores that tab's previous scroll position.
2. Collections listing
   - Loading, empty, error, retry, and refreshing states are visible and readable.
   - Collection rows show title, description, media scope, kind/source/visibility/status badges, presentation, duplicate policy, item count, artwork/theme text, and materialization/stale state.
   - Keyboard focus reaches the Collections tab and collection rows via normal button traversal.
3. Detail navigation
   - Activating a collection row opens a read-only detail view.
   - Back returns to the Collections listing.
   - Detail shows metadata, materialization status, shelf/rule status, and item preview without exposing manual editing controls.

## Current visual QA status

Automated render smoke coverage is in place. Full manual visual QA still requires a live desktop player session with server-side collection fixtures.
