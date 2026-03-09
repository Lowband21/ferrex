# Flathub Submission Guide

This guide documents how to submit Ferrex Player to Flathub for distribution.

## Prerequisites

- Flatpak manifest is already configured: `flatpak/io.github.lowband21.FerrexPlayer.yml`
- AppStream metadata: `flatpak/io.github.lowband21.FerrexPlayer.metainfo.xml`
- Desktop entry: `flatpak/io.github.lowband21.FerrexPlayer.desktop`
- Icons: 128x128, 192x192, 512x512 in `flatpak/icons/`

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
