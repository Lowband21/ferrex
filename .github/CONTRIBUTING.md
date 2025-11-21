# Contributing to Ferrex

Thanks for your interest in improving Ferrex! This document explains how to get set up, how we work, and what we expect in pull requests so contributions are smooth and productive.

Ferrex is a Rust workspace with several crates (server, player, core, models, contracts). We value focused changes, clear communication, and a fast feedback loop.

## Quick Expectations

- Be considerate and constructive. We follow the spirit of the Contributor Covenant (see “Code of Conduct”).
- Sign every commit with the DCO trailer (`Signed-off-by:`). See “DCO (Required)”.
- Contributions are under MIT OR Apache‑2.0. See “License of Contributions”.
- Report security issues privately. See `SECURITY.md`.

## Ways to Contribute

1. File an issue to report a bug or discuss a feature.
2. Open a discussion for design questions or larger proposals.
3. Submit a pull request for a focused improvement.

For substantial changes, please open an issue or discussion first so we can align on approach.

## Local Setup

Prerequisites:

- Rust toolchain (stable, `rustc 1.90+`, edition 2024)
- just (command runner): https://github.com/casey/just
- Docker + Docker Compose (for DB/Redis and full stack)
- pre-commit (to run tracked git hooks)

Recommended tool installs:

```bash
cargo install just # or: your preferred/system package manager
rustup component add rustfmt clippy
pipx install pre-commit   # or: pip install --user pre-commit

# Tools used by hooks and checks
cargo install sqlx-cli       # for SQLx prepare/check
cargo install cargo-deny     # for license/advisory checks
```

Enable the repository’s tracked hooks so `pre-commit` and `pre-push` run automatically:

```bash
git config core.hooksPath .githooks
pre-commit run --all-files   # optional warmup
```

## Running the Project

Generate configuration and start the local stack (Postgres, Redis, server):

```bash
# from repo root
just config      # interactive generator for .env in project root
just start            # boots DB/Redis + ferrex-server
```

Common development tasks:

```bash
just run-server       # run server (debug)
just run-player       # run player (priority profile)
just fmt              # format
just lint             # clippy (workspace)
just test             # tests (workspace)
```

You can also use plain cargo:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -D warnings
cargo test --workspace --all-features --all-targets --no-fail-fast
```

## Database and SQLx

Parts of the codebase use SQLx with offline metadata under `./.sqlx/`.

- To verify queries without network/DB access, run the offline check:

  ```bash
  SQLX_OFFLINE=true cargo sqlx prepare --workspace --check
  ```

- If you change queries or migrations, update the metadata (requires a DB):

  ```bash
  # ensure the stack is up (just start) and DATABASE_URL is set via .env
  just prepare
  # or
  cargo sqlx prepare --workspace -- --all-features --all-targets
  ```

## Git Hooks (pre-commit)

We keep fast checks in `pre-commit` and heavier checks in `pre-push`. After enabling hooks (`git config core.hooksPath .githooks`):

- Pre-commit (quick):
  - Trailing whitespace / EOF / line endings
  - YAML / TOML / JSON syntax checks
  - Private key and case-conflict detection
  - Shell formatting (shfmt) and lint (shellcheck)
  - rustfmt (check)

- Pre-push (heavier):
  - cargo clippy (workspace, all targets/features, `-D warnings`)
  - SQLx offline check (`SQLX_OFFLINE=true cargo sqlx prepare --workspace --check`)
  - cargo-deny (per `deny.toml`)
  - hadolint (Dockerfiles)

Manual runs:

```bash
pre-commit run --all-files --verbose
pre-commit run --hook-stage pre-push --all-files --verbose
```

Keep hooks fresh:

```bash
pre-commit autoupdate
pre-commit migrate-config   # when prompted by pre-commit
```

Note: Direct commits to `main` are disabled.

## Development Guidelines

- Keep PRs scoped and reviewable. Prefer a series of small changes over one large one.
- Add or update tests when behavior changes or new behavior is introduced.
- Run formatting, linting, and tests locally before pushing.
- Update documentation (README, comments) when public behavior or workflows change.
- Avoid unrelated refactors in the same PR unless they’re mechanical and low-risk.

## Pull Requests

Please include:

- Motivation and context for the change (link related issues/discussions).
- Summary of the approach and any trade-offs.
- Test coverage or a manual test plan.
- Notes on migration or operational impact (if any).

CI builds the workspace on Linux/macOS/Windows and runs format, advisories, deny, and clippy. Some DB-dependent tests may not run in CI; please run them locally.

## Dependency Updates

Dependabot maintains third‑party dependencies with small, predictable batches.

- Schedule: weekly on Mondays at 04:00 UTC.
- Scope: Cargo workspace, GitHub Actions, and Dockerfiles under `docker/`.
- Grouping:
  - Cargo: one PR for all patch updates, one for all minor updates (majors are separate).
  - Actions/Docker: one PR grouping patch + minor updates.
- Labels: `dependencies`. Reviewer: `@Lowband21`.
- Commit conventions: Conventional Commits (e.g., `chore(deps): …`, `chore(actions): …`).

Review/merge guidelines:

- Let CI run. For Cargo updates, ensure `cargo build`, `cargo deny`, and `cargo audit` pass.
- Skim release notes for non‑trivial changes or transitive advisory fixes.
- Prefer squash merge with the generated title. Keep one PR per group for a clean history.

If a dependency needs to be pinned or ignored, propose a change to `.github/dependabot.yml` with rationale.

## DCO (Required)

Ferrex uses the Developer Certificate of Origin (DCO) 1.1. Sign off every commit to certify you wrote the code or otherwise have the right to submit it.

Add the sign-off automatically with `-s`:

```bash
git commit -s -m "Your commit message"
```

This adds a trailer like:

```
Signed-off-by: Your Name <you@example.com>
```

Learn more: https://developercertificate.org/

## License of Contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Ferrex by you shall be dual-licensed under MIT or Apache‑2.0, at your option, without additional terms or conditions. See `LICENSE-MIT` and `LICENSE-APACHE`.

## Code of Conduct

We strive to provide a welcoming, harassment‑free experience for everyone. Please read and follow our local Code of Conduct:

`.github/CODE_OF_CONDUCT.md`

If you experience or witness unacceptable behavior, contact ferrex@lowband.me.

## Security

Please report security issues privately as described in `SECURITY.md`. Do not create public issues for vulnerabilities.

## Governance

Ferrex is currently maintained by a single maintainer (Grayson Hieb) on a best‑effort basis. As the project grows, we may evolve governance and invite additional maintainers.

- For large or controversial changes, open an issue or discussion first.
- Response times may vary based on availability.
- Final decisions on scope and release readiness rest with the maintainer.

## Trademarks

“Ferrex” is a trademark of Grayson Hieb. Use of the name or any logos must not imply endorsement or affiliation. See `TRADEMARKS.md` for guidance.

---

Thank you again for contributing! If anything in this document is unclear or missing, please open an issue or discussion so we can improve it.
