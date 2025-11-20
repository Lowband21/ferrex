# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and this project adheres to Semantic Versioning once releases begin.

## [Unreleased]

- No notable changes yet.

## [0.1.0-alpha] - 2025-11-03

Initial pre-release. This captures all noteworthy changes so far, grouped by Conventional Commit type. Expect breaking changes and rapid iteration.

### Breaking
- player: integrate new Wayland subsurface player crate (!). (146bc10a)
- core: complete scan rewrite with durable queue and expanded metadata (!). (11d11461)
- server: fix image cache race via atomic writes; incompatible on-disk format change (!). (1bcf8632)
- player: new virtual carousel and motion controller powering home overhaul (!). (c1b88685)
- rkyv/serialization: major performance improvement with client-side data handling rewrite; large internal API changes (!). Highlights: sub-8ms latency spikes, drastically reduced idle CPU and memory usage. (9fb7fcac)

### Added
- Metadata and library
  - Implement TMDB-backed metadata integration. (ed2188ea)
  - Library improvements, optimization, and scanning changes. (835d0472, a8b7b81e)
  - Series support and scanning refinements. (48ae92cc, b1b1e9a0)
  - Functional sort and filter; grid/image loading improvements. (3eecf8d7, 4176e717)
- Player
  - Subtitles and smooth playback. (cf4afd2f)
  - Custom poster widget with rounded corners; opacity/flip animations. (30bd6230, 5918317a)
  - External mpv handoff and watch-status integration. (c5f870e7)
  - Unified Subwave player and domain updates; migration to unified backend. (38b00f70, b6ea45cf)
  - Rebase `iced_ferrex` on upstream and reimplement batching. (83e08c89)
  - Virtual carousel + motion controller for home experience. (c1b88685)
- Server/Core
  - Scanning and parsing updates; folder inventory and HSTS middleware. (4f004aab, f5edddb4)
  - Refactors across server/core; domain-driven design groundwork. (0841dc06, c21cbb51)
  - Server claim flow and auth; setup and automation. (b0ee9bc2, 52d4b05e)
  - Mid-scan media streaming. (d0925240)
  - Major refactor of Postgres backend module. (b5414eb9)
  - Break out crates from `ferrex-core`; extensive cleanup. (f0415543)
  - Search reintegration with global keypress listening; dedicated search window. (a1e0dfea, 8c92613e)
  - Iced focus traversal. (9a7beb37)
- Ops/Packaging/Docs
  - Demo mode; setup simplification; validation. (11dfb6fe, e50fd5cd)
  - Basic Windows packaging and initial CI. (bbf62fba)
  - README updates and documentation polish, including config table and `.env` example; added screenshots/demo placeholders. (4553da01)
  - Dependabot for Cargo, Actions, Docker; release scaffolding via cargo-dist. (dependabot/cargo-dist setup)

### Changed
- Player architecture refactors toward DDD-inspired design; loading changes and media domain refactor. (c3ff7a59, 0351a981)
- Continued core refactors; server config; warning cleanup. (07450f1c, 18125deb)
- Server updates with UI integration. (28a4b76f)
- Auto-config and stack management (functional, pending simplification). (0f5efc11)
- Crate reorganization and extensive cleanup. (f0415543)
- Capitalization/wording polish (e.g., macOS; mpv handoff). (docs polish)

### Fixed
- Harden image handling across player/server; image types refactor. (df4e2a47, 4ed3551a)
- Resolve TMDB reference upsert edge cases. (3b6e61b7)
- Player library loading race hardening. (b9963a18)
- Restrict transparency to Subwave Wayland backend; add auth view background. (21c99183)
- Remove unused subtitle overlay after Subwave changes. (8b906588)
- Remove dead tests and refactor others. (7ccb0345)
- Project housekeeping: update iced version; crate renames. (86986ec2)

### Performance
- Player image loading/rendering rework with priority queue. (54a8d447)
- Flexible profiling infrastructure for the player. (135b33ec)
- Major rkyv-backed performance improvements; memory and CPU reductions. (9fb7fcac)

### Removed
- Legacy/unused tests and overlays. (7ccb0345, 8b906588)
- Old README badges/TODOs as part of docs refresh. (docs cleanup)

### Tooling/Chore
- Dependabot: enable updates for Cargo workspace, GitHub Actions, and Dockerfiles under `docker/` via `.github/dependabot.yml`; weekly schedule (Mon 04:00 UTC), labels, reviewer, and grouping (Cargo patch/minor; Actions/Docker patch+minor).
- Release automation scaffolding via cargo-dist (GitHub Actions workflows).
- Dependency trims; pre-commit hooks; formatting. (390e77fa, 457c8c72)
- Update poster widget batching for iced rebase. (c813027c)

### Notes
- This is a preview release; APIs, storage formats, and behaviors may change.
