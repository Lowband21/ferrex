# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and this project adheres to Semantic Versioning once releases begin.

## [0.1.2-alpha](https://github.com/Lowband21/ferrex/compare/v0.1.1-alpha...v0.1.2-alpha) (2026-03-31)


### Features

* nix crane migration, GStreamer 1.28.1 upgrade, and full subtitle support ([#32](https://github.com/Lowband21/ferrex/issues/32)) ([6ae162d](https://github.com/Lowband21/ferrex/commit/6ae162dc05d58ccd0b8edf29b68a1c061fefc616))

## [0.1.1-alpha](https://github.com/Lowband21/ferrex/compare/v0.1.0-alpha...v0.1.1-alpha) (2026-03-10)


### Bug Fixes

* **ci:** update dependencies for cargo aduit and fix release flow ([95b43a2](https://github.com/Lowband21/ferrex/commit/95b43a27a85cb648ea8b7347a1c48cfd9726a936))

## 0.1.0-alpha (2026-03-09)


### ⚠ BREAKING CHANGES

* **player:** new virtual carousel and motion controller facilitating home overhaul
* **server:** server image cache race causing currupted images resolved by atomic writes
* **core:** complete scan rewrite with durable queue and expanded metadata
* achieve major rkyv performance improvement
* **player:** integrate new wayland subsurface player crate

### Features

* achieve major rkyv performance improvement ([3878301](https://github.com/Lowband21/ferrex/commit/3878301fcaae51a2709a7961849bd7859a8c9ebd))
* **backend:** identity-based TV watch tracking and APIs ([cc844db](https://github.com/Lowband21/ferrex/commit/cc844dbfd9506fbfec92508585c36e86d5ce6821))
* basic windows packaging and untested ci ([80b49cf](https://github.com/Lowband21/ferrex/commit/80b49cf231a7bcad5e54ad98191f8b85e50302a6))
* break out crates from ferrex-core and extensive cleanup ([da25bd4](https://github.com/Lowband21/ferrex/commit/da25bd44ad2ef719360054b82b71011c1bb5afd6))
* complete major refactor of postgres backend module ([d064a4b](https://github.com/Lowband21/ferrex/commit/d064a4bb68ccde7c92aabfcd21b4a0ff42887ac6))
* **config:** ferrex-config edge cases and safeties ([c67259a](https://github.com/Lowband21/ferrex/commit/c67259a7a5d4b94621ffc30a5e65e24039cb9f8e))
* **config:** initial ferrix config crate implementation and testing to parody with scripts ([8692a8e](https://github.com/Lowband21/ferrex/commit/8692a8e438e6d58ae59ef341e57319b292bc6a8e))
* **config:** simplify and address auto configuration edge cases plus new .env.example ([0211273](https://github.com/Lowband21/ferrex/commit/02112736d76304111def86a587184bb61eca0697))
* continue core refactor ([40052ec](https://github.com/Lowband21/ferrex/commit/40052ec2b3a04c3e718de957b1b79bd1758e6b89))
* **core:** complete scan rewrite with durable queue and expanded metadata ([7853bcb](https://github.com/Lowband21/ferrex/commit/7853bcb904888713ff9b108aa2cd652da49cd4c1))
* **core:** series scanning refinements and hardening ([6f2702a](https://github.com/Lowband21/ferrex/commit/6f2702a5f5a3fce0a36c33d2927997ef68c4ac90))
* dedicated search window and associated key handling ([b3a2f0e](https://github.com/Lowband21/ferrex/commit/b3a2f0e285a80617a3841723c7a92975f228be4b))
* demo mode changes and control ([205536c](https://github.com/Lowband21/ferrex/commit/205536c4b23f6f40d55693015756863cc307fb0c))
* **docs:** simplify and update docs ([b9fac51](https://github.com/Lowband21/ferrex/commit/b9fac51770325f6f7207e260db910ac609e7c739))
* functional but overcomplicated auto config and stack management ([15ba138](https://github.com/Lowband21/ferrex/commit/15ba138338d973d2b1ababe1be11115a576e842f))
* functional sort and filter ([44d3ea4](https://github.com/Lowband21/ferrex/commit/44d3ea4387a0e4a3bc5dba55c05c575e31927aef))
* get all tests passing plus server claim finalization and auth work ([a4734a9](https://github.com/Lowband21/ferrex/commit/a4734a991bb9373d81580e154a218e70b7b098e4))
* iced focus traversal ([89c9cd4](https://github.com/Lowband21/ferrex/commit/89c9cd4462623046d448c4ad0475f31b1f6e986a))
* iced_aw menu and usage brought up to latest 0.14-dev ([05d458a](https://github.com/Lowband21/ferrex/commit/05d458a2620a51ce17002d2293b8e0b99abf00e4))
* image handling improvements and hardening plus kinetic scrolling and general ui polish ([620a514](https://github.com/Lowband21/ferrex/commit/620a514f4702b9c615130b2beb48de84ef8c36c3))
* image restructuring and theme infrastructure ([224f40e](https://github.com/Lowband21/ferrex/commit/224f40e6c7262cd036b93b99d6f2f329b9d48240))
* implement metadata system through tmdb api integration ([9f9bd06](https://github.com/Lowband21/ferrex/commit/9f9bd0629b81a8e9470e4a50d8ff73ec9c6e458e))
* initial core and server domain driven design refactor ([a4dd34f](https://github.com/Lowband21/ferrex/commit/a4dd34f4d6372c3c4da9d3cfdacac2b1dcc82b7c))
* initial series addition and player side changes to accommodate ([0651f6d](https://github.com/Lowband21/ferrex/commit/0651f6d368edf4546203b4d749a65c23501e7e91))
* library improvements and metadata loading updates ([a3e763d](https://github.com/Lowband21/ferrex/commit/a3e763d92a2c5e667d68d9125ab19ff56603c9e8))
* library optimization and scanning changes ([77564bd](https://github.com/Lowband21/ferrex/commit/77564bdf25d4b62fdf29d7bb281a47edf54bbc60))
* many small improvements across sorting, filtering, grid view, and image loading ([e3d20b0](https://github.com/Lowband21/ferrex/commit/e3d20b0a6ac399075aa97453e4283fce4d9601f2))
* mid scan media streaming ([3e64680](https://github.com/Lowband21/ferrex/commit/3e6468058519cb79d51f446d071acd99a5d4576d))
* **ops:** setup simplification plus demo mode improved and validated ([0bc4425](https://github.com/Lowband21/ferrex/commit/0bc4425fc4bbdf103fd87d0a94bc627c9d85b9b1))
* packaging infrastructure, postgres auto-tuning, and release pipeline ([#14](https://github.com/Lowband21/ferrex/issues/14)) ([723d521](https://github.com/Lowband21/ferrex/commit/723d521107578e4748a5d7777c4354a6a47edb40))
* **player:** add windows mpv handoff and initial details view menu path ([657febe](https://github.com/Lowband21/ferrex/commit/657febe8c904c1bba12c6e0a085304e5168640dd))
* **player:** changes to loading and finish refactor of media domain ([ce6ab2a](https://github.com/Lowband21/ferrex/commit/ce6ab2abe41ccc0fe70efb6ac65c7fb2db6e62f9))
* **player:** custom poster widget with rounded corners ([d7ac25d](https://github.com/Lowband21/ferrex/commit/d7ac25d8fba3c6210078dd8f54ac06757d1a6a5c))
* **player:** enhanced setup wizard and first-run authentication flow ([9dfc374](https://github.com/Lowband21/ferrex/commit/9dfc37407549d25aa7b66f8301665de1afef2837))
* **player:** external mpv player and watch status player integration ([5f5e7e1](https://github.com/Lowband21/ferrex/commit/5f5e7e1c17f3722f4bbf5904a36681f4c9133164))
* **player:** flip menu physics rewrite/tune and sdf text atlas ([9aac485](https://github.com/Lowband21/ferrex/commit/9aac4854fffbb3090f508563813623e642309b9d))
* **player:** identity based episode playback navigation ([a80af12](https://github.com/Lowband21/ferrex/commit/a80af12bd0422344984247245eb3b5ac85a9ceef))
* **player:** implement dynamic texture upload budget to improve latency ([7dc8e50](https://github.com/Lowband21/ferrex/commit/7dc8e5017833ad64b7e6a2f68fc3795b55d47f46))
* **player:** initial attempt at back face buttons ([1741199](https://github.com/Lowband21/ferrex/commit/1741199d7fbe33976b816a29aa4dc0ee182507f2))
* **player:** initial settings implementation with modular and extensible sections ([7fc7ee7](https://github.com/Lowband21/ferrex/commit/7fc7ee70eee4be0393eadecbf387635421e69aaa))
* **player:** initial unified application scaling of layout and fonts ([541a0b4](https://github.com/Lowband21/ferrex/commit/541a0b443178b94d74fbcacd810b256f638a2612))
* **player:** integrate new wayland subsurface player crate ([8cf59d2](https://github.com/Lowband21/ferrex/commit/8cf59d21a93374bd9f8669986b4736078ac1f6ac))
* **player:** new flip on right click and back face ready for a menu ([f1f4cb7](https://github.com/Lowband21/ferrex/commit/f1f4cb738af4f86fb3f5f1b370b5eee01e8f22c5))
* **player:** new virtual carousel and motion controller facilitating home overhaul ([e42ae00](https://github.com/Lowband21/ferrex/commit/e42ae001b0d22a4c3a56f8baed25e701c608bbe1))
* **player:** player image/media caching and search ui update ([1ae84a7](https://github.com/Lowband21/ferrex/commit/1ae84a7e7e1a1e07d88ee83c769ae715dff74e07))
* **player:** player migration to subwave unified backend and detail view work ([7e40460](https://github.com/Lowband21/ferrex/commit/7e404601b6ef4e1260380aa02f000e701a186dae))
* **player:** poster opacity and flip animations ([46218d5](https://github.com/Lowband21/ferrex/commit/46218d512ae58cac8f857c022e307b5c1d9c3a47))
* **player:** primitive trait batching through iced_wgpu crate changes ([be3e82d](https://github.com/Lowband21/ferrex/commit/be3e82db1944214d5f9b214eeb6892d1966776f1))
* **player:** readme and other docs plus ci changes ([c4f5578](https://github.com/Lowband21/ferrex/commit/c4f5578b10851d1fb0b3e398d1cbb238f5886800))
* **player:** rebase iced_ferrex on upstream and reimplement batching ([26305f6](https://github.com/Lowband21/ferrex/commit/26305f6beb5d02b89820c9bcdeb1d7b6fe49ff9f))
* **player:** refactor player modules toward a domain driven design inspired architecture ([9bea527](https://github.com/Lowband21/ferrex/commit/9bea5278e146d9bd03f37546281d4ea1ed78699d))
* **player:** sdf poster title and metadata text ([0ab045d](https://github.com/Lowband21/ferrex/commit/0ab045d6ebd18d96fcd96c9cc70f6f37305c1dfb))
* **player:** subtitles and smooth playback ([e14a53e](https://github.com/Lowband21/ferrex/commit/e14a53e93101b12be6078833d9906c9e09df0b7c))
* **player:** tweaked poster fetching to improve performance scaling ([5eb06ce](https://github.com/Lowband21/ferrex/commit/5eb06ce3bbaa263a6943457afe083010c6b52494))
* reintegrate search and add global keyboard listening to search on keypress ([347279e](https://github.com/Lowband21/ferrex/commit/347279e08008425b9005c2371aa7402e1e9de46c))
* rework image pipeline to be event-based instead of poll ([#11](https://github.com/Lowband21/ferrex/issues/11)) ([5ad7134](https://github.com/Lowband21/ferrex/commit/5ad71348ec0a89f9dcdc1812c16513bba0595f75))
* server claim functionality plus refactored core changes and integration ([ea94edd](https://github.com/Lowband21/ferrex/commit/ea94edda8071805e4fe95dcd0f840265bbb73362))
* server config and address many warnings ([4c30bee](https://github.com/Lowband21/ferrex/commit/4c30bee00926e59b59b5219a99e8b0b1f511ae1c))
* server setup and auth automation plus docs ([898f591](https://github.com/Lowband21/ferrex/commit/898f5913499ca8b5f2ec1b6fa9c9712dca4a21ff))
* server updates and ui integration ([0bda5f6](https://github.com/Lowband21/ferrex/commit/0bda5f6bd6cd584846034225ff2e054b597b2e93))
* **server:** changes to folder inventory and HSTS middleware ([1d4c738](https://github.com/Lowband21/ferrex/commit/1d4c738dd1e3e92bbb3a4c64dabec2d20f2806d3))
* **server:** default ENFORCE_HTTPS to true in non-dev; fail-fast when behind proxy without TRUST_PROXY_HEADERS ([585a8b0](https://github.com/Lowband21/ferrex/commit/585a8b036ee5e4d2e1d1f1fed9dd6e3e2072522b))
* **server:** default TLS 1.3; min version/suites; cert reload ([c2687f4](https://github.com/Lowband21/ferrex/commit/c2687f4dfdcadacb977b49f41e9b3334f787b778))
* **server:** media batching endpoints and handler reorg ([f6298df](https://github.com/Lowband21/ferrex/commit/f6298dfa3307e2697735e43e5d1f548d02883900))
* **server:** refactored across server/core and reworked disabled features like scanning ([07a9ef4](https://github.com/Lowband21/ferrex/commit/07a9ef413e06eed6a2d0278ef84069b61ec957d5))
* **server:** scanning and parsing updates ([142a944](https://github.com/Lowband21/ferrex/commit/142a944cb41da488715bd84fea600c0e1a888084))
* **stream:** secure media file streaming for subwave and mpv ([c189e57](https://github.com/Lowband21/ferrex/commit/c189e5759858396c8e3da6f292462efe5c41d422))
* unified subwave player and other player domain changes ([cf46440](https://github.com/Lowband21/ferrex/commit/cf464407910d7e3def419903cee6fe5aa92f99ae))


### Bug Fixes

* additional hardening of image handling across player and server ([046edb7](https://github.com/Lowband21/ferrex/commit/046edb713b5edd392e92a8eb76c60ed8736d8a9e))
* **ci:** add exception and refactor dep usage ([a21ebd9](https://github.com/Lowband21/ferrex/commit/a21ebd9b55c57c294d2b55a081cc7e71c5fb0328))
* **ci:** refine pre-commit hooks ([a0daa3c](https://github.com/Lowband21/ferrex/commit/a0daa3c14df92b52da1d0f0885230a692665d03a))
* **config:** host server mode with clean env and improve services up ([d716d79](https://github.com/Lowband21/ferrex/commit/d716d79214a96817551fb7c6ad48bc5adde189ca))
* **config:** no longer imply clean on reset-db flag ([7f9bcf9](https://github.com/Lowband21/ferrex/commit/7f9bcf904c2821dff5f86af9df33ebb1d7a43fef))
* harden tmdb reference upserts ([83bcef3](https://github.com/Lowband21/ferrex/commit/83bcef396dd1d56abe0f6d499ed918f562e78e0e))
* image types refactor and auth improvements ([796d83c](https://github.com/Lowband21/ferrex/commit/796d83c6a22a1f93cf1f691f68f3d2de75b6662c))
* **player:** disable texture preloader until utility is proven ([ca87ff2](https://github.com/Lowband21/ferrex/commit/ca87ff2abc342b7a493cd6544cb63b23f9085d6f))
* **player:** fix player poster deduplication preventing valid multiple instance ([e82884e](https://github.com/Lowband21/ferrex/commit/e82884e0efb6ab9c58196c3e5d47eba6f42da14e))
* **player:** library loading race fix and hardening ([52f6a54](https://github.com/Lowband21/ferrex/commit/52f6a54f2018dcc5a721c53992de73d27abf6206))
* **player:** rename mock and stub auth service implementations and update comments for clarity ([6e64090](https://github.com/Lowband21/ferrex/commit/6e6409041353cec32f0dbede2c312334cecd96e5))
* **player:** resolve cross view click through bug by triggering on release ([5ccc410](https://github.com/Lowband21/ferrex/commit/5ccc4109de762f0dcd19e5315015565aa1256821))
* project housekeeping, update iced version and rename crates ([3dd863b](https://github.com/Lowband21/ferrex/commit/3dd863b481d96e49a7c7596f0f3d6bedfb9883af))
* remove dead tests and refactor others ([98e1328](https://github.com/Lowband21/ferrex/commit/98e13280c77dab124d4c5fcb58404b5e0fc40208))
* remove unused subtitle overlay after removing from subwave dep ([2771449](https://github.com/Lowband21/ferrex/commit/2771449c53b9154286a029823532e847496b09ca))
* restrict transparency to subwave wayland backend and add background widget to auth views ([dfeae3c](https://github.com/Lowband21/ferrex/commit/dfeae3c142101504466e168a975b84749156581c))
* **server:** atomic image reads and write-once caching ([d3ae905](https://github.com/Lowband21/ferrex/commit/d3ae905b0d06dcf1d54e9c7432d92d8add9bde7b))
* **server:** server image cache race causing currupted images resolved by atomic writes ([73b8290](https://github.com/Lowband21/ferrex/commit/73b8290e35feb386c1f344d69af3702d34b0dc00))


### Performance

* **player:** priority queue addition and rework to image loading and rendering ([f24400f](https://github.com/Lowband21/ferrex/commit/f24400f2b0f7440eac28376faa958feb991a01ef))
* **profiling:** add flexible performance profiling infrastructure to player ([81ab0e0](https://github.com/Lowband21/ferrex/commit/81ab0e0bf0d5933ce6b2db43ccfed2044a798092))


### Refactoring

* **core:** domain/database reorg and media schema revision ([b439b88](https://github.com/Lowband21/ferrex/commit/b439b88f5f9cfc6ebfd07a16a0afe7f0eba8c825))
* **player:** simplify and change animation defaults to only flip in detail views ([11ffd5d](https://github.com/Lowband21/ferrex/commit/11ffd5de1f21ca04b10fc369d575aed8d88d1789))
* **player:** ui domain refactor ([9eeca23](https://github.com/Lowband21/ferrex/commit/9eeca238271b782a306cfaaacfc16553bc59e1c3))
* **player:** use explicitly named Message enums across domains ([437eab5](https://github.com/Lowband21/ferrex/commit/437eab506f0997d153e9e9e389176a7e53483048))


### Documentation

* initial docs publish workflow and metadata added ([10732e7](https://github.com/Lowband21/ferrex/commit/10732e71ba19a4aca0853cf039c73286fcc70f45))
* initial docs publish workflow and metadata added ([aa481c6](https://github.com/Lowband21/ferrex/commit/aa481c69e6f5dbb65baffccdd386772fd6baf1cc))

## [Unreleased]

### Added
- Flatpak packaging with desktop integration (desktop file, metainfo)
- release-please workflow for automated releases on PR merge
- crates.io publishing workflow for ferrex-model, ferrex-contracts, ferrexctl
- Flatpak CI workflow for automated builds
- **ferrexctl packaging commands**:
  - `ferrexctl package preflight` - Run pre-release checks (fmt, clippy, tests, deny, audit)
  - `ferrexctl package release-init` - Create GitHub releases with binaries and Docker images
  - `ferrexctl package windows` - Build Windows portable packages with GStreamer bundling
- **Postgres performance presets** - Configure postgres tuning via `FERREX_POSTGRES_PRESET` (small, medium, large, custom)
- **Unraid Community Apps template** - Template for easy Unraid deployment

### Changed
- Renamed ferrex-config/ferrex-init to ferrexctl throughout
- Updated Docker images workflow to use ferrexctl naming
- Version bumped to 0.1.0-alpha for initial release

### Fixed
- Fixed stale references to ferrex-config package (now ferrexctl)
- Fixed Docker init container to use correct binary name

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
