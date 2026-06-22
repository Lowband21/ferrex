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

- `start/` explains how the documentation is organized and where new readers should begin.
- `operator/` covers installation, configuration, security, media-library operations, and recovery.
- `developer/` covers repository setup, architecture, testing, client work, and contribution workflow.
- `reference/` indexes durable specs, workflows, and legacy in-repo notes until they are migrated.
- `release/` covers packaging, publishing, and release validation once those guides are promoted into the site.
