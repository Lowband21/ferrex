# ferrex-player-foundation

Dependency-light player foundation primitives shared by current and planned Ferrex
clients.

This crate intentionally contains only non-UI contracts and helpers:

- repository result/error types,
- byte/unit helpers,
- authentication policy/setup-status DTOs and PIN policy validation helpers,
- generic domain update/event helper containers.

It must not depend on `ferrex-player`, Iced, subwave, or Ferrex domain crates. See
`../docs/player-dependency-boundaries.md` for the dependency direction policy.
