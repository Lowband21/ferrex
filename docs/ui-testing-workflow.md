# Ferrex UI Tests

Ferrex Player has headless UI flow tests recorded as `.ice` scripts under
`ferrex-player/tests/ui/`.

## Run locally

```bash
cargo test -p ferrex-player --test ui_end_to_end
```

This test discovers and replays every `.ice` script in `ferrex-player/tests/ui/`
via `iced_test`. The harness enables local test stubs to avoid network
dependencies.

## Notes

- If you add/change `.ice` scripts, keep them small and stable.
- If you want interactive recording, treat it as experimental for now (the
  default `ferrex-player` binary is daemon-based and does not currently expose a
  dedicated "record" mode).
