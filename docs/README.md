# Ferrex documentation site

This directory contains the Ferrex documentation app built with [Astro Starlight](https://starlight.astro.build/). It is configured for the custom-domain-ready canonical URL `https://ferrexmedia.org/` with root base-path semantics (`base: '/'`).

## Local commands

Recommended Nix workflow from the repository root:

```bash
nix develop .#server
just docs-install
just docs-check
just docs-build
just docs-dev      # local authoring server at http://127.0.0.1:4321/
just docs-preview  # preview the last static build at http://127.0.0.1:4321/
```

The default `.#ferrex-player` shell exposes the same documentation tooling. Both dev shells include Node.js 24 and Corepack; Corepack uses the PNPM version pinned by the `packageManager` field in `docs/package.json`.

Non-Nix fallback:

```bash
corepack pnpm --dir docs install --frozen-lockfile
corepack pnpm --dir docs run check
corepack pnpm --dir docs run build
corepack pnpm --dir docs run dev
corepack pnpm --dir docs run preview
```

If your Node.js distribution does not include Corepack, install Node.js `>=22.12.0` plus PNPM `>=11.8.0` and run the same commands as `pnpm --dir docs ...`.

## Checks and link validation

- `just docs-check` runs `astro check` for the Starlight/TypeScript content model.
- `just docs-build` runs `astro build`; the build fails on invalid internal Starlight links and produces static output under `docs/dist/`.
- `just docs-link-check` is a documented alias for the build-time internal link validation.
- External-link crawling is not configured in this build-only workflow; verify external URLs manually when adding or changing them.

## Deployment scope

This app intentionally contains no deployment workflow, CNAME file, GitHub Pages settings, Cloudflare Pages setup, DNS record, or secret-dependent domain configuration. Deployment wiring should be added separately after the hosting target is approved.

## Content layout

Starlight content lives in `src/content/docs/` and is grouped by reader intent:

- `start/` explains how the documentation is organized and how to add pages.
- `operator/` covers configuration, authentication/security, Unraid, demo mode, and common operator questions.
- `developer/` covers architecture, the backend intelligence foundation, player crate boundaries, UI testing, and SQLx/database workflows.
- `reference/mobile/` covers generated mobile contract references.
- `reference/qa/` preserves Android/TV QA packets, playback/auth evidence, visual/a11y runbooks, and stale/manual-hardware status labels.
- `reference/` also links canonical GitHub policies, crate READMEs, and legacy Markdown pointer paths.
- `release/` covers packaging references such as Flathub submission.

Legacy flat Markdown notes under `docs/*.md` are lightweight pointers to these pages; update Starlight content rather than restoring duplicate long-form docs at those paths.
