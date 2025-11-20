#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: show-setup-token.sh <env-file>" >&2
  exit 1
fi

ENV_FILE="$1"

if [[ ! -f "$ENV_FILE" ]]; then
  echo "Env missing: $ENV_FILE. Run: just init-config" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/utils/lib/python.sh"

if ! ferrex_require_python; then
  exit 1
fi

if ! SETUP_TOKEN="$(ferrex_python - "$ENV_FILE" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1]).expanduser()
if not path.exists():
    print(f"{path} does not exist", file=sys.stderr)
    raise SystemExit(1)

token = ''
with path.open('r', encoding='utf-8') as fh:
    for raw in fh:
        line = raw.strip()
        if not line or line.startswith('#'):
            continue
        if line.startswith('FERREX_SETUP_TOKEN='):
            token = line.split('=', 1)[1]
            if token.startswith('"') and token.endswith('"'):
                token = token[1:-1]
            break

if not token:
    print(f"No FERREX_SETUP_TOKEN configured in {path}", file=sys.stderr)
    raise SystemExit(1)

print(token)
PY
)"; then
  exit 1
fi

printf 'FERREX_SETUP_TOKEN from %s:\n%s\n' "$ENV_FILE" "$SETUP_TOKEN"
