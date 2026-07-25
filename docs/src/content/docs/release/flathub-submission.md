---
title: "Flathub submission guide"
description: "How to prepare and submit the Ferrex Player Flatpak manifest to Flathub and maintain post-merge updates."
sidebar:
  order: 2
---

> **Drift-prevention:** This Starlight page is the canonical Flathub submission guide. The legacy `packaging/flathub/README.md` path now points here to prevent duplicate instructions.

This guide documents how to submit Ferrex Player to Flathub for distribution. The Git commands below apply to the separate upstream Flathub repository clone; use Jujutsu for Ferrex repository work.

## Prerequisites

- Flatpak manifest is already configured: `flatpak/io.github.lowband21.FerrexPlayer.yml`
- AppStream metadata: `flatpak/io.github.lowband21.FerrexPlayer.metainfo.xml`
- Desktop entry: `flatpak/io.github.lowband21.FerrexPlayer.desktop`
- Icons: 128x128, 192x192, 512x512 in `flatpak/icons/`

## Native playback packaging profile

The manifest builds Ferrex with the `mpv` feature and bundles pinned mpv
0.41.0, FFmpeg 8.1.2, libplacebo, libass, and LuaJIT. Its configure checks fail
the build if mpv resolves `gpl=true`, if FFmpeg enables GPL/nonfree/version-3
code, or if the required Wayland/Vulkan/dmabuf feature set is missing. The final
binary must have a direct `libmpv.so.2` dependency, and the exact license files
and effective build profiles are installed below
`/app/share/licenses/io.github.lowband21.FerrexPlayer/`.

mpv 0.41 gates its X11 VO on GPL sources. The reviewed Flatpak profile does not
ship those sources: Wayland can use the explicit native-window mpv backend,
while X11 retains the integrated GStreamer backend or the separate external
player action. Do not enable mpv's GPL option merely to make the X11 feature
check pass.

Validate the finished artifact locally with:

```bash
flatpak-builder --user --force-clean --repo=target/flatpak-repo \
  target/flatpak-build flatpak/io.github.lowband21.FerrexPlayer.yml
flatpak build-bundle target/flatpak-repo target/ferrex-player.flatpak \
  io.github.lowband21.FerrexPlayer
```

Install the bundle and verify that `ldd /app/bin/ferrex-player` resolves
`libmpv.so.2`, FFmpeg, libplacebo, libass, and LuaJIT from `/app/lib` before
publishing it.

## Submission Steps

### 1. Fork the Flathub Repository

```bash
gh repo fork flathub/flathub --clone=true
cd flathub
```

### 2. Create Your App Branch

```bash
git checkout -b io.github.lowband21.FerrexPlayer
```

### 3. Add Your Manifest

Copy your manifest to the repository root:

```bash
cp /path/to/ferrex/flatpak/io.github.lowband21.FerrexPlayer.yml .
```

### 4. Create flathub.json (Optional)

If you need special build options, create `flathub.json`:

```json
{
  "only-arches": ["x86_64"],
  "skip-icons-check": false
}
```

### 5. Submit Pull Request

```bash
git add .
git commit -m "Add io.github.lowband21.FerrexPlayer"
git push origin io.github.lowband21.FerrexPlayer
gh pr create --title "Add io.github.lowband21.FerrexPlayer" --body "Ferrex Player - Native media player with zero-copy HDR on Wayland"
```

### 6. Wait for Review

The Flathub team will review your PR. Common checks:
- AppStream metadata validation
- Desktop file validation
- Build succeeds on Flathub infrastructure
- Security review

### 7. Post-Merge

After merge, your app will be:
- Built automatically on Flathub's infrastructure
- Published to https://flathub.org/apps/io.github.lowband21.FerrexPlayer
- Available via `flatpak install flathub io.github.lowband21.FerrexPlayer`

## Maintenance

After initial submission, updates are handled via:
- x-data-checker (automatic version updates)
- Manual PRs for major changes
- Flathub's build system monitors your releases

## References

- [Flathub App Submission Guide](https://github.com/flathub/flathub/wiki/App-Submission)
- [Flatpak Manifest Documentation](https://docs.flatpak.org/en/latest/manifests.html)
- [AppStream Metadata Guide](https://www.freedesktop.org/software/appstream/docs/)
