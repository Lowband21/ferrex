#!/usr/bin/env bash
set -euo pipefail

# Thin shim to the Rust-based stack orchestration.
# Accepts either:
#   stack-up.sh [options]          -> defaults to `stack up`
#   stack-up.sh up|down [options]  -> explicit action

ACTION="up"
if [[ "${1:-}" == "up" || "${1:-}" == "down" ]]; then
  ACTION="$1"
  shift
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if command -v ferrex-init >/dev/null 2>&1; then
  exec ferrex-init stack "$ACTION" "$@"
else
  cd "$ROOT_DIR"
  exec cargo run -q -p ferrex-config --bin ferrex-init -- stack "$ACTION" "$@"
fi
