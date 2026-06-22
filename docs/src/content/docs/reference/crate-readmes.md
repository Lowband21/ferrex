---
title: "Crate README inventory"
description: "Package-facing Ferrex crate README files that remain canonical and are linked from the docs site to avoid duplicate package metadata."
sidebar:
  order: 5
---

Crate and package README files remain canonical at their package paths because crates.io, IDEs, and repository browsers expect them there. The docs site links them and hosts cross-cutting guides separately.

| README | Purpose |
| --- | --- |
| [`crates/ferrex-contracts/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrex-contracts/README.md) | Trait surfaces and domain contracts built atop `ferrex-model`. |
| [`crates/ferrex-model/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrex-model/README.md) | Shared media, library, user/auth, and metadata models. |
| [`crates/ferrexctl/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrexctl/README.md) | Configuration bootstrapper and packaging CLI usage. |
| [`crates/ferrex-player/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrex-player/README.md) | Desktop player facade, runtime notes, screenshots, validation, HDR, and Flatpak notes. |
| [`crates/ferrex-player-foundation/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrex-player-foundation/README.md) | Dependency-light player primitives and boundary policy link. |
| [`crates/ferrex-player-repository/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrex-player-repository/README.md) | Player-side media repository overlays and server-scoped disk cache support. |
| [`crates/ferrex-player-ui/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrex-player-ui/README.md) | Desktop presentation layer during player crate decomposition. |
| [`crates/ferrex-core/tests/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrex-core/tests/README.md) | Core test suite and scanner runtime regression commands. |
| [`crates/ferrex-server/tests/README.md`](https://github.com/Lowband21/ferrex/blob/dev/crates/ferrex-server/tests/README.md) | Server E2E and scanner regression-suite commands. |

For cross-crate architecture, use [Architecture](/developer/architecture/). For player dependency direction and guard policy, use [Player dependency boundaries](/developer/player-dependency-boundaries/).
