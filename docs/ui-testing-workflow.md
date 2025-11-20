# Ferrex UI Testing Workflow

This guide documents the workflow for capturing, storing, and running
end-to-end UI tests with Iced's tester tooling inside Ferrex.

## 1. Prerequisites
- Build and run the player with offline stubs so recordings stay deterministic:
  ```bash
  cargo run -p ferrex-player --features testing
  ```

## 2. Recording a Flow
1. Launch the player using the command above.
2. Press `F12` to toggle the tester overlay.
3. Choose a preset that matches the scenario (e.g. `FirstRun`,
   `AuthenticatedWithDevices`). Presets ensure the app bootstraps into a known
   state before the recording begins.
4. Click **Record** and exercise the UI exactly as the intended user would.
5. Add expectations while recording (e.g. "expect \"Device revoked\"") to
   assert UI state.
6. Stop the recording and use the instruction editor to clean up labels or add
   extra expectations.
7. Export the script and save it under `ferrex-player/tests/ui/<name>.ice`.
   Commit the `.ice` file alongside any code changes that rely on it.

## 3. Managing Test Assets
- Keep one `.ice` file per high-value flow. Use descriptive filenames such as
  `first_run_admin_setup.ice` or `device_revoke_flow.ice`.
- If a flow needs new fixtures, add a preset in `app::presets` and re-record.
- When a change breaks an existing script, re-record or edit the `.ice` file to
  reflect the new behaviour before merging.

## 4. Running Tests Locally
Execute the headless emulator harness at any time:
```bash
cargo test -p ferrex-player --features testing --test ui_end_to_end
```
The harness automatically discovers every `.ice` file in `ferrex-player/tests/ui`
and replays them with the emulator. Failures surface as regular test failures.

### Useful Flags
- `-- --nocapture` keeps emulator log output visible during the run.
- `TEST_LOG=debug` (or similar) can be used to increase logging detail.

## 5. Tips for Reliable Recordings
- Assign stable `widget::Id`s to interactive controls before recording to make
  selectors resilient to layout tweaks.
- Prefer presets that enable test stubs so end-to-end runs avoid network trips
  or external state.
- Keep recordings focused: avoid unnecessary steps and ensure expectations cover
  the critical assertions for the flow.
- Re-run the harness after each `.ice` change to confirm the script still passes
  before opening a pull request.

## 6. Preparing for CI
- UI tests run entirely headless, so CI only needs to execute the same cargo
  test command.
- Ensure any new presets or stubs are added to the codebase before pushing a
  new `.ice` file; otherwise the emulator run will fail when CI starts from a
  clean state.

Following this workflow keeps the tester overlay, `.ice` assets, and emulator
suite aligned so Ferrex UI regressions are caught early.
