# Ferrex UI tests and screenshots

Ferrex Player has deterministic app-shell UI flows recorded as `.ice` scripts
under `crates/ferrex-player-app/tests/ui/` and a headless screenshot CLI built
into the `ferrex-player` binary.

Generated screenshots are developer artifacts. Write them to `target/` (the
`just` shortcuts default to `target/ui-screenshots/`) or to an explicit caller
path, and do not commit them as visual baselines unless a future issue adds a
reviewed baseline policy.

## Discover scenarios

List the deterministic presets that the screenshot harness can boot:

```bash
cargo run -p ferrex-player --profile priority -- screenshot list
# or
just screenshot-list
```

The list is produced from the same preset registry used by the app and includes
names such as `FirstRunAuth`, `DesktopLibraryHome`, `SettingsDevices`,
`TenFootHome`, `TenFootDetail`, and `PlayerLoadingOverlay`.

## Capture a screenshot

A direct capture chooses a preset, logical viewport, scale factor, emulator mode,
settle time, and output path:

```bash
cargo run -p ferrex-player --profile priority -- screenshot \
  --preset DesktopLibraryHome \
  --viewport 1280x720 \
  --scale-factor 1 \
  --mode Immediate \
  --settle-ms 200 \
  --output target/ui-screenshots/desktop-library-1280x720.png
```

Each capture writes the PNG plus a JSON sidecar next to it, for example
`desktop-library-1280x720.metadata.json`. The sidecar records the preset,
logical viewport, physical PNG size, scale factor, mode, settle time, and any
`.ice` metadata used for replay.

Options:

- `--preset <NAME>` boots a deterministic app state. Use `screenshot list` for
  names and descriptions.
- `--viewport <WIDTHxHEIGHT>` sets the logical app/emulator viewport.
- `--scale-factor <N>` controls the physical PNG size. For example, `1280x720`
  at `--scale-factor 2` writes a `2560x1440` PNG while layout still uses a
  `1280x720` viewport.
- `--mode <Zen|Patient|Immediate>` selects the iced-test runtime strategy.
  `Immediate` is fastest for static preset captures; use `Patient` or metadata
  from an `.ice` file when replaying recorded flows that wait on UI events.
- `--settle-ms <MS>` drains runtime actions for a short period before capture.
- `--output <PATH>` is required for captures. Parent directories are created.

## Replay a `.ice` script before capture

Use `--ice` to run a recorded UI script before taking the screenshot:

```bash
cargo run -p ferrex-player --profile priority -- screenshot \
  --ice crates/ferrex-player-app/tests/ui/register_admin.ice \
  --output target/ui-screenshots/register-admin-after-flow.png
```

The harness reads `.ice` metadata (`viewport`, `mode`, and optional `preset`) and
uses it as defaults. If you pass explicit `--preset`, `--viewport`, or `--mode`,
they must match the script metadata; mismatches fail early so agents do not
capture the wrong layout.

## Short `just` wrappers

Common capture commands are available from any subdirectory:

```bash
just screenshot-desktop-720
just screenshot-first-run-900
just screenshot-tv-1080
just screenshot-tv-800p
```

They cover the review viewports currently expected by Ferrex UI work:

| Shortcut | Preset | Viewport | Default artifact |
| --- | --- | --- | --- |
| `just screenshot-desktop-720` | `DesktopLibraryHome` | `1280x720` | `target/ui-screenshots/desktop-library-1280x720.png` |
| `just screenshot-first-run-900` | `FirstRunAuth` | `1280x900` | `target/ui-screenshots/first-run-1280x900.png` |
| `just screenshot-tv-1080` | `TenFootHome` | `1920x1080` | `target/ui-screenshots/tenfoot-home-1920x1080.png` |
| `just screenshot-tv-800p` | `TenFootHome` | `1280x800` | `target/ui-screenshots/tenfoot-home-1280x800.png` |

Override the artifact path by passing the recipe argument:

```bash
just screenshot-tv-1080 /tmp/ferrex-tv-home.png
```

For one-off captures, the generic wrapper forwards arguments to the CLI:

```bash
just screenshot --preset TenFootDetail --viewport 1920x1080 \
  --scale-factor 1 --mode Immediate --output target/ui-screenshots/detail.png
```

## Run UI tests and smoke tests

Replay every committed `.ice` script:

```bash
cargo test -p ferrex-player-app --test ui_end_to_end
```

Run the renderer-dependent screenshot smoke test:

```bash
cargo test -p ferrex-player-app --test screenshot_smoke -- --nocapture
```

When a headless renderer is available, the smoke test captures one small
`FirstRunAuth` PNG in a temporary directory and verifies the PNG plus metadata
sidecar were written. When iced/WGPU cannot initialize a headless adapter, it
skips with the same actionable renderer-unavailable reason as the `.ice` replay
test.

## Renderer troubleshooting

The tests and CLI both use `iced_test::Emulator`, so they require a usable WGPU
adapter even though no display server is required.

Existing skip behavior:

- `ui_end_to_end` catches the known iced-test renderer initialization panic and
  prints an actionable skip reason instead of failing unrelated validation.
- `screenshot_smoke` captures when possible and prints the same skip reason when
  the renderer cannot initialize.
- A skipped renderer-dependent test means the current machine could not provide
  the emulator renderer; it does not prove the UI flow or screenshot succeeded.

The screenshot CLI differs from tests: `ferrex-player screenshot ...` is an
artifact-producing command, so renderer initialization failure exits non-zero and
prints the WGPU/headless troubleshooting hint rather than silently skipping.

Useful diagnostics and workarounds:

```bash
FERREX_SCREENSHOT_TRACE=1 cargo run -p ferrex-player --profile priority -- screenshot ...
WGPU_BACKEND=vulkan cargo test -p ferrex-player-app --test screenshot_smoke -- --nocapture
WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 WGPU_ADAPTER_NAME=llvmpipe \
  cargo test -p ferrex-player-app --test screenshot_smoke -- --nocapture
```

If those still fail, confirm Mesa/Vulkan software rendering libraries are
available in the shell. Native platforms can also try `WGPU_BACKEND=metal` or
`WGPU_BACKEND=dx12` as appropriate.

## Player crate graph checks

When UI extraction work changes player crate dependencies or domain/update
boundaries, run the dependency guard before broader validation:

```bash
./scripts/check-player-crate-boundaries.sh
```

Pair it with focused player checks as needed:

```bash
cargo check -p ferrex-player-app --all-targets
cargo test -p ferrex-player-app --test ui_end_to_end
cargo test -p ferrex-player-app --test screenshot_smoke -- --nocapture
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
- Prefer screenshots under `target/ui-screenshots/` for PR evidence so cleanup is
  automatic with other build artifacts.
- Interactive recording remains experimental; the default `ferrex-player` binary
  is daemon-based and does not currently expose a dedicated record mode.
