#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

die() {
  echo "error: $*" >&2
  exit 1
}
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
  cat <<EOF
Local pre-release checks (no network by default).

Usage:
  scripts/release/preflight.sh --scope <workspace|init> [--offline]

Notes:
- --scope workspace runs checks relevant to the workspace release tag vX.Y.Z.
- --scope init runs checks relevant to the ferrexctl release tag ferrexctl-vX.Y.Z.
- --offline adds cargo --offline where applicable and avoids network-y tools.
EOF
}

scope=""
offline=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --scope)
      scope="${2:-}"
      shift 2
      ;;
    --offline)
      offline=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) die "unknown arg: $1" ;;
  esac
done

[[ $scope == "workspace" || $scope == "init" ]] || die "missing/invalid --scope (workspace|init)"

if ! have cargo; then
  die "missing cargo; run inside nix develop or install Rust toolchain"
fi

offline_flags=()
if [[ $offline -eq 1 ]]; then
  offline_flags+=(--offline)
fi

echo "== preflight: fmt =="
if have nix && [[ ${FERREX_RELEASE_USE_NIX:-1} == "1" ]]; then
  nix develop .#ferrex-player -c cargo fmt --all --check
else
  cargo fmt --all --check
fi

echo "== preflight: clippy =="
if [[ $scope == "init" ]]; then
  if have nix && [[ ${FERREX_RELEASE_USE_NIX:-1} == "1" ]]; then
    nix develop .#ferrex-player -c cargo clippy -p ferrexctl "${offline_flags[@]}" --all-targets --all-features -- -D warnings
  else
    cargo clippy -p ferrexctl "${offline_flags[@]}" --all-targets --all-features -- -D warnings
  fi
else
  # Workspace release includes server + player artifacts; keep scope narrow but relevant.
  if have nix && [[ ${FERREX_RELEASE_USE_NIX:-1} == "1" ]]; then
    nix develop .#ferrex-player -c cargo clippy -p ferrex-server -p ferrex-player -p ferrexctl "${offline_flags[@]}" --all-targets --all-features -- -D warnings
  else
    cargo clippy -p ferrex-server -p ferrex-player -p ferrexctl "${offline_flags[@]}" --all-targets --all-features -- -D warnings
  fi
fi

echo "== preflight: tests (ferrexctl) =="
if have nix && [[ ${FERREX_RELEASE_USE_NIX:-1} == "1" ]]; then
  nix develop .#ferrex-player -c cargo test -p ferrexctl "${offline_flags[@]}"
else
  cargo test -p ferrexctl "${offline_flags[@]}"
fi

if have cargo-deny && [[ $offline -eq 0 ]]; then
  echo "== preflight: cargo deny =="
  cargo deny check
fi

if have cargo-audit; then
  if [[ $offline -eq 1 ]]; then
    # Avoid network; only run if the local advisory DB is present.
    if [[ -d "${CARGO_HOME:-$HOME/.cargo}/advisory-db" ]]; then
      echo "== preflight: cargo audit (offline) =="
      cargo audit --no-fetch
    else
      echo "== preflight: cargo audit (offline) skipped: advisory DB not present =="
    fi
  else
    echo "== preflight: cargo audit =="
    cargo audit
  fi
fi

echo "== preflight: ok =="
