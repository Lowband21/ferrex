# Ferrex documentation site

This directory contains the Ferrex documentation app built with [Astro Starlight](https://starlight.astro.build/). It is configured for the custom-domain-ready canonical URL `https://ferrexmedia.org/` with root base-path semantics (`base: '/'`).

## Local commands

Use Corepack or a locally installed PNPM matching the `packageManager` field in `package.json`.

```bash
pnpm --dir docs install --frozen-lockfile
pnpm --dir docs run check
pnpm --dir docs run build
```

Helpful development commands:

```bash
pnpm --dir docs run dev
pnpm --dir docs run preview
```

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
