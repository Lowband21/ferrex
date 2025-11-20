#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: show-setup-token.sh <config-path>" >&2
  exit 1
fi

CONFIG_PATH="$1"

if [[ ! -f "$CONFIG_PATH" ]]; then
  echo "Config missing: $CONFIG_PATH. Run: just init-config" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/utils/lib/python.sh"

if ! ferrex_require_python; then
  exit 1
fi

if ! SETUP_TOKEN="$(ferrex_python - "$CONFIG_PATH" <<'PY'
import sys
from pathlib import Path
try:
    import tomllib
except ModuleNotFoundError:  # python <3.11 without stdlib toml parser
    import tomli as tomllib  # type: ignore[import-not-found]

config_path = Path(sys.argv[1]).expanduser()
if not config_path.exists():
    print(f"{config_path} does not exist", file=sys.stderr)
    raise SystemExit(1)

with config_path.open('rb') as handle:
    data = tomllib.load(handle)

token = (data.get('auth') or {}).get('setup_token')
if not token or not str(token).strip():
    print(f"No setup token configured in {config_path.resolve()}", file=sys.stderr)
    raise SystemExit(1)

print(token)
PY
)"; then
  exit 1
fi

printf '[auth].setup_token from %s:\n%s\n' "$CONFIG_PATH" "$SETUP_TOKEN"
