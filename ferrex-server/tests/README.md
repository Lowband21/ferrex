Ferrex Server Tests

End-to-end HTTP tests require a running server and external resources. They are gated behind the `e2e` feature and ignored by default.

Run E2E tests explicitly:

```bash
cargo test -p ferrex-server --features e2e -- --ignored
```

Notes:
- Tests target `http://localhost:3000` by default; ensure the server is running and matches expected routes.
- Keep `--ignored` so unit tests don’t block while E2E runs.
