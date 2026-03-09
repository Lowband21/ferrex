#!/usr/bin/env bash
set -euo pipefail

# DEPRECATED: This script is a thin wrapper around `ferrexctl package release`.
# Use `ferrexctl package release` directly for new workflows.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

version="${1:-}"
shift || true

if [[ -z $version ]]; then
  echo "usage: scripts/release/release-workspace.sh <version> [ferrexctl-args...]" >&2
  echo "" >&2
  echo "This script delegates to: ferrexctl package release" >&2
  echo "For full options, run: ferrexctl package release --help" >&2
  exit 2
fi

echo "Delegating to: ferrexctl package release --version $version $*"
exec cargo run -p ferrexctl -- package release --version "$version" "$@"
