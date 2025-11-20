# FAQ

Common questions about platforms, playback, and workflows.

## Why is HDR Wayland‑only today?

Ferrex’s native zero‑copy HDR path depends on Wayland subsurfaces and recent HDR metadata handling in GStreamer. This combination enables passing HDR surfaces to the compositor without expensive copies. Other platforms currently lack an equivalent path in this project.

See platform specifics and player notes in `ferrex-player/README.md`.

## Will HDR come to Windows/macOS?

That’s a goal. A cross‑platform native HDR path will require platform‑specific work and maturing dependencies. Until then, Windows/macOS can use the cross‑platform backend or the “Open with MPV” hand‑off.

## How does MPV hand‑off preserve position and status?

The player communicates with mpv via IPC (Unix) or a named pipe (Windows) and keeps watch state synchronized with the server. You can override the mpv path on Windows using `FERREX_MPV_PATH` if auto‑detection fails.

## What’s the default server port?

`3000` (HTTP). Configure via `FERREX_BIND` in `config/.env`. See `docs/configuration.md` for more.

## Where do I configure environment variables?

`config/.env`. Generate or refresh it with `just config` or `just start`. A reference lives at `config/.env.example`.

## How do I adjust server logging verbosity?

Use `--rust-log` when starting the stack, e.g.:

```bash
just start --rust-log 'sqlx=trace,ferrex=debug'
```

Or set `RUST_LOG` in `config/.env`.

## Is there a quick way to try Ferrex without real media?

Yes. Use the feature‑gated Demo Mode to seed a disposable library. See `docs/demo-mode.md` for enabling flags and env.

## How do I record and run UI tests?

The player ships with a tester overlay and a headless emulator. See `docs/ui-testing-workflow.md` for the full workflow.

## Where can I report issues or check known issues?

Use GitHub Issues. For transient caveats and ongoing problems, check open issues with relevant labels (e.g., `bug`, `known-issues`).
