#!/usr/bin/env bash
set -euo pipefail

# Create a dedicated Tailnet config directory by cloning an existing config
# and rewriting connection endpoints to localhost for Tailscale shared namespace.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

source "$ROOT_DIR/utils/lib/python.sh"

usage() {
  cat <<'EOF'
Usage: make-tailnet-config.sh [--from <path>] [--to <path>] [--force]

Creates a Tailnet-focused config directory by copying an existing config
and updating the database host to 127.0.0.1 (and Redis URL accordingly).

Options:
  --from <path>   Source config directory (default: ./config)
  --to <path>     Destination config directory (default: ./config/tailnet)
  --force         Overwrite destination if it already exists
  -h, --help      Show this help

After running, start the stack via:
  just start --mode tailscale --config-dir <dest>
EOF
}

FROM_DIR="$ROOT_DIR/config"
TO_DIR="$ROOT_DIR/config/tailnet"
FORCE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --from)
      [[ $# -ge 2 ]] || { echo "Missing value for --from" >&2; usage; exit 1; }
      FROM_DIR="$2"; shift 2
      ;;
    --to)
      [[ $# -ge 2 ]] || { echo "Missing value for --to" >&2; usage; exit 1; }
      TO_DIR="$2"; shift 2
      ;;
    --force)
      FORCE=1; shift
      ;;
    -h|--help)
      usage; exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2; usage; exit 1
      ;;
  esac
done

if ! ferrex_require_python; then
  exit 1
fi

resolve_path() {
  local input="$1"
  if ferrex_detect_python; then
    ferrex_python - "$input" <<'PY'
import sys
from pathlib import Path
print(Path(sys.argv[1]).expanduser().resolve())
PY
  elif command -v realpath >/dev/null 2>&1; then
    realpath "$input"
  else
    (cd "$input" >/dev/null 2>&1 && pwd) || printf '%s\n' "$input"
  fi
}

FROM_DIR="$(resolve_path "$FROM_DIR")"
TO_DIR="$(resolve_path "$TO_DIR")"

SRC_ENV="$FROM_DIR/.env"
[[ -s "$SRC_ENV" ]] || { echo "Missing source env: $SRC_ENV" >&2; exit 1; }

if [[ -e "$TO_DIR" && "$FORCE" -ne 1 ]]; then
  echo "Destination exists: $TO_DIR (use --force to overwrite)" >&2
  exit 1
fi

mkdir -p "$TO_DIR"

cp -f "$SRC_ENV" "$TO_DIR/.env"

DEST_ENV="$TO_DIR/.env"
DEST_TOML=""
DEST_SECRETS_DIR=""

# Patch .env for new directory and Tailnet endpoints
ferrex_python - "$DEST_ENV" "$TO_DIR" <<'PY'
import os
import sys
from pathlib import Path

env_path = Path(sys.argv[1])
dest_dir = Path(sys.argv[2]).resolve()

def load_env_lines(path: Path):
    lines = []
    if path.exists():
        with path.open('r', encoding='utf-8') as fh:
            lines = [line.rstrip('\n') for line in fh]
    return lines

def parse_env(lines):
    data = {}
    for line in lines:
        if not line or line.startswith('#') or '=' not in line:
            continue
        k, v = line.split('=', 1)
        data[k] = v
    return data

def write_env(path: Path, kv):
    def needs_quotes(text: str) -> bool:
        if text.startswith('"') and text.endswith('"'):
            return False
        if not text:
            return True
        if any(ch.isspace() for ch in text):
            return True
        if text.startswith('#'):
            return True
        if any(ch in text for ch in ('"', "'")):
            return True
        return False

    def escape_value(text: str) -> str:
        escaped = text.replace('\\', '\\\\').replace('"', '\\"')
        return f'"{escaped}"'

    lines = []
    for k, v in kv.items():
        value_to_write = v
        if needs_quotes(v):
            value_to_write = escape_value(v)
        lines.append(f"{k}={value_to_write}")
    with path.open('w', encoding='utf-8') as fh:
        for line in lines:
            fh.write(line + '\n')

lines = load_env_lines(env_path)
data = parse_env(lines)

data['FERREX_CONFIG_DIR'] = str(dest_dir)
data['DATABASE_HOST_CONTAINER'] = '127.0.0.1'
data['REDIS_URL_CONTAINER'] = 'redis://127.0.0.1:6379'

# Preserve other variables as-is
write_env(env_path, data)
PY

## No need to rewrite a config file; environment overrides in Tailscale mode

cat <<EOF
Tailnet config prepared at: $TO_DIR
- .env rewritten for target directory and Tailnet endpoints

Start Tailnet stack with:
  just start --mode tailscale --config-dir "$(printf '%s' "$TO_DIR")"
EOF
