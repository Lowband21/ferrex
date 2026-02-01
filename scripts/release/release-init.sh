#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

source scripts/release/lib.sh

usage() {
  cat <<EOF
Release ferrexctl as a draft GitHub Release and push image to GHCR.

Usage:
  scripts/release/release-init.sh <version> [--no-image] [--tag-latest] [--dry-run] [--skip-preflight] [--offline-preflight] [--skip-build]

Tag:
  ferrexctl-v<version>
EOF
}

version="${1:-}"
[[ -n $version ]] || {
  usage
  exit 2
}
shift || true

no_image=0
tag_latest=0
dry_run=0
skip_preflight=0
offline_preflight=0
skip_build=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-image)
      no_image=1
      shift
      ;;
    --tag-latest)
      tag_latest=1
      shift
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    --skip-preflight)
      skip_preflight=1
      shift
      ;;
    --offline-preflight)
      offline_preflight=1
      shift
      ;;
    --skip-build)
      skip_build=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) die "unknown arg: $1" ;;
  esac
done

want="$version"
have="$(ferrexctl_version)"
[[ -n $have ]] || die "failed to read ferrexctl version from ferrexctl/Cargo.toml"
[[ $have == "$want" ]] || die "ferrexctl version mismatch: ferrexctl/Cargo.toml=$have requested=$want"

tag="ferrexctl-v${version}"

if [[ $dry_run -eq 0 ]]; then
  ensure_clean_tree
fi
if [[ $skip_preflight -eq 0 ]]; then
  if [[ $offline_preflight -eq 1 ]]; then
    scripts/release/preflight.sh --scope init --offline
  else
    scripts/release/preflight.sh --scope init
  fi
fi
if [[ $dry_run -eq 0 ]]; then
  ensure_gh
fi

repo="$(git_repo_slug)"
dist="$(make_dist_dir "$tag")"

assets=()

echo "Building ferrexctl binary tarball → ${dist}/ferrexctl_linux_x86_64_${version}.tar.gz"
if [[ $skip_build -eq 1 ]]; then
  echo "[skip-build] would build tarball → ${dist}/ferrexctl_linux_x86_64_${version}.tar.gz"
else
  scripts/build/ferrexctl-binary.sh "$version" "${dist}/ferrexctl_linux_x86_64_${version}.tar.gz"
  assets+=("${dist}/ferrexctl_linux_x86_64_${version}.tar.gz")
fi

if [[ $no_image -eq 0 ]]; then
  ensure_docker
  img="ghcr.io/${repo%%/*}/ferrexctl:${version}"
  if [[ $dry_run -eq 1 ]]; then
    echo "[dry-run] would build/push image: $img"
  else
    ghcr_login
    echo "Building/pushing image: $img"
    docker buildx build \
      --platform linux/amd64 \
      -f docker/Dockerfile.init \
      -t "$img" \
      "$([[ $tag_latest -eq 1 ]] && echo \"-t ghcr.io/"${repo%%/*}"/ferrexctl:latest\")" \
      --push \
      .
  fi
fi

write_manifest "${dist}/manifest.json" "$tag" "$version" "init" "${assets[@]}"
write_sha256sums "$dist"
assets+=("${dist}/manifest.json" "${dist}/SHA256SUMS")

if [[ $dry_run -eq 1 ]]; then
  echo "[dry-run] would create draft GitHub Release: $tag (repo=$repo) with assets:"
  printf '  - %s\n' "${assets[@]}"
  echo "[dry-run] would rely on CI (tag push) to verify/publish"
  exit 0
fi

echo "Creating draft GitHub Release with tag ${tag} in ${repo}"
if gh release view "$tag" >/dev/null 2>&1; then
  die "release/tag already exists: $tag (refusing to overwrite)"
fi

gh release create "$tag" \
  --repo "$repo" \
  --draft \
  --title "ferrexctl ${tag}" \
  --notes "" \
  --target HEAD \
  "${assets[@]}"

echo "Done. Draft release created for tag: $tag"
