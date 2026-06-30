---
title: "Smart-shelf MVP QA"
description: "MVP boundaries, local provider setup, deterministic fake-provider testing, screenshots, and excluded surfaces for desktop smart shelves."
sidebar:
  order: 8
---

Smart shelves are a desktop Ferrex Player MVP surface for turning a grounded intelligence draft into a private manual collection. The MVP is intentionally narrow: a user opens the composer, starts a local-provider run, reviews a grounded draft, optionally locks/replaces items, saves the draft, and lands on the saved collection detail.

## MVP boundaries

Included in this MVP:

- Desktop player smart-shelf composer, provider fallback, running/progress, draft review, alternates/replacement, save, and saved collection detail states.
- Grounded draft items with media ids, reasons, source chips, and validation issues surfaced before save.
- Saving a valid draft as a private manual collection with `generated_by = "smart_shelf"` provenance.
- Empty and error collection states after save so recovery is visible instead of blank or data-wipe-class failure.

Excluded from this MVP:

- Android and Android TV behavior.
- Home pinning, dynamic rails, chatbot surfaces, and playback queue mutation.
- Server-driven collection promotion beyond the private manual collection created by accepting a draft.
- Live-model visual baselines in committed tests.

## Local provider setup expectations

The real runtime is local-provider first and disabled unless an operator opts in. For a local OpenAI-compatible provider, run a trusted local server such as `llama.cpp` and configure Ferrex with the same expectations documented in the intelligence foundation:

```bash
FERREX_INTELLIGENCE_ENABLED=true
FERREX_INTELLIGENCE_BASE_URL=http://127.0.0.1:8081/v1
FERREX_INTELLIGENCE_MODEL=gemma-4-12b-it
# Optional for providers that require it; local providers can leave this empty.
FERREX_INTELLIGENCE_API_KEY=
```

Before using the composer against a live server, confirm `GET /api/v1/intelligence/provider/status` reports a ready provider for the authenticated user. Provider failures must remain recoverable through the smart-shelf provider fallback; users should be able to edit the prompt or retry readiness without clearing app data.

## Deterministic fake-provider testing

Committed tests and screenshot presets do not require a live model. They use deterministic fake-provider fixtures with stable run ids, artifact ids, media ids, draft content, alternates, validation, save responses, and collection detail rows.

Focused Rust coverage:

```bash
cargo test -p ferrex-player-app --test smart_shelf_mvp
cargo test -p ferrex-player-app app::presets::tests::smart_shelf_mvp_scenarios_seed_visual_qa_states
cargo test -p ferrex-player-app screenshot::visual_qa::tests::smart_shelf_mvp_matrix_covers_required_tags
```

The `smart_shelf_mvp_start_draft_save_opens_collection_detail_fixture` integration test exercises the deterministic start -> run status -> draft -> save -> collection detail path without a live provider.

## Screenshot presets and visual QA matrix

The player screenshot harness exposes one preset per MVP visual state:

| State | Preset | Default artifact path |
| --- | --- | --- |
| Composer | `SmartShelfComposer` | `target/ui-screenshots/smart-shelf-mvp/01-smart-shelf-composer.png` |
| Running/progress | `SmartShelfRunningProgress` | `target/ui-screenshots/smart-shelf-mvp/02-smart-shelf-running-progress.png` |
| Draft ready | `SmartShelfDraftReady` | `target/ui-screenshots/smart-shelf-mvp/03-smart-shelf-draft-ready.png` |
| Alternates/replacement | `SmartShelfAlternatesReplacement` | `target/ui-screenshots/smart-shelf-mvp/04-smart-shelf-alternates-replacement.png` |
| Provider unavailable | `SmartShelfProviderUnavailable` | `target/ui-screenshots/smart-shelf-mvp/05-smart-shelf-provider-unavailable.png` |
| Saved collection detail | `SmartShelfSavedCollectionDetail` | `target/ui-screenshots/smart-shelf-mvp/06-smart-shelf-saved-collection-detail.png` |
| Collection empty | `SmartShelfCollectionEmpty` | `target/ui-screenshots/smart-shelf-mvp/07-smart-shelf-collection-empty.png` |
| Collection error | `SmartShelfCollectionError` | `target/ui-screenshots/smart-shelf-mvp/08-smart-shelf-collection-error.png` |

Capture the full MVP matrix when a headless renderer is available:

```bash
cargo run -p ferrex-player --profile priority -- screenshot matrix smart-shelf \
  --output-dir target/ui-screenshots/smart-shelf-mvp
```

The command writes PNGs plus `target/ui-screenshots/smart-shelf-mvp/smart-shelf-mvp-visual-qa-matrix.json`. Use `--dry-run` or `list` for non-renderer metadata checks:

```bash
cargo run -p ferrex-player --profile priority -- screenshot matrix smart-shelf list
cargo run -p ferrex-player --profile priority -- screenshot matrix smart-shelf --dry-run --only state:collection-error
```

## QA checklist

For each capture, verify:

- The visible UI matches only the desktop smart-shelf/Collections MVP boundaries.
- Provider unavailable, empty collection, and collection error states include retry/edit/recovery copy.
- Draft items, replacement badges, alternates, source chips, save affordance, and saved collection provenance are legible.
- No Android/TV, Home pinning, chatbot, dynamic rail, or playback queue behavior appears in code, copy, or screenshots.
