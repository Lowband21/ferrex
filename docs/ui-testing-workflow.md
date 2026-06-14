# Ferrex UI Tests

Ferrex Player has headless app-shell UI flow tests recorded as `.ice` scripts
under `crates/ferrex-player-app/tests/ui/`.

## Run locally

```bash
cargo test -p ferrex-player-app --test ui_end_to_end
```

This test discovers and replays every `.ice` script in `crates/ferrex-player-app/tests/ui/`
via `iced_test`. The harness enables local test stubs to avoid network
dependencies while exercising the same shell wiring used by the installed
`ferrex-player` binary.

## Player crate graph checks

When UI extraction work changes player crate dependencies or domain/update
boundaries, run the dependency guard before broader validation:

```bash
./scripts/check-player-crate-boundaries.sh
```

The guard verifies that non-UI player crates do not import or transitively pull
in Iced/subwave runtime crates. Pair it with focused player checks as needed:

```bash
cargo check -p ferrex-player-app --all-targets
cargo test -p ferrex-player-app --test ui_end_to_end
cargo test -p ferrex-player-library -p ferrex-player-media -p ferrex-player-search
```

## Focused visual QA checklist

Use this checklist when validating UI extraction work or follow-up visual polish.
Record the app mode, viewport, server/demo data source, and any screenshots or
regressions in the PR/release note.

### Desktop surfaces

- Library/home: poster grids and virtual carousels render images, text, hover or
  keyboard focus, sort/filter controls, and loading/error states without clipped
  content.
- Detail: movie and TV detail routes show poster/backdrop art, cast cards,
  technical details, play/resume actions, and watch-state badges.
- Auth: first-run setup, user selection, credential entry, PIN setup/login,
  loading, and retry/error flows preserve focus and do not require clearing app
  data to recover.
- Settings: sidebar navigation plus profile, security, devices, libraries,
  display, playback, performance, server, theme, and users sections render and
  retain input/focus state while switching sections.

### 10-foot surfaces

- Home: TV rails, poster focus rings, context menu close/restore behavior, and
  vertical navigation across empty/non-empty rails work at 1920x1080 and a small
  800p-style viewport.
- Detail: hero content, two-row focus window, resume/start-over labels, related
  media columns, and D-pad movement stay visible in TV mode.
- Player overlay: transparent video container, command focusables, progress bar,
  time labels, hidden-control filtering, and spatial navigation remain inside the
  viewport.

## Notes

- If you add/change `.ice` scripts, keep them small and stable.
- In headless environments where `iced_test` cannot create its emulator renderer,
  the replay harness reports the environment skip instead of failing unrelated
  crate validation.
- If you want interactive recording, treat it as experimental for now (the
  default `ferrex-player` binary is daemon-based and does not currently expose a
  dedicated "record" mode).
