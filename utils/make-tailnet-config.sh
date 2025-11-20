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
SRC_TOML="$FROM_DIR/ferrex.toml"
SRC_SECRETS_DIR="$FROM_DIR/secrets"

[[ -s "$SRC_ENV" ]] || { echo "Missing source env: $SRC_ENV" >&2; exit 1; }
[[ -s "$SRC_TOML" ]] || { echo "Missing source config: $SRC_TOML" >&2; exit 1; }

if [[ -e "$TO_DIR" && "$FORCE" -ne 1 ]]; then
  echo "Destination exists: $TO_DIR (use --force to overwrite)" >&2
  exit 1
fi

mkdir -p "$TO_DIR"

# Copy base artifacts
cp -f "$SRC_ENV" "$TO_DIR/.env"
cp -f "$SRC_TOML" "$TO_DIR/ferrex.toml"

# Copy secrets if present
if [[ -d "$SRC_SECRETS_DIR" ]]; then
  mkdir -p "$TO_DIR/secrets"
  # Preserve file modes where possible
  cp -a "$SRC_SECRETS_DIR/." "$TO_DIR/secrets/"
fi

DEST_ENV="$TO_DIR/.env"
DEST_TOML="$TO_DIR/ferrex.toml"
DEST_SECRETS_DIR="$TO_DIR/secrets"
DEST_RUNTIME_ENV="$TO_DIR/.env.runtime"

# Patch .env for new directory and Tailnet endpoints
ferrex_python - "$DEST_ENV" "$TO_DIR" "$DEST_SECRETS_DIR" <<'PY'
import os
import sys
from pathlib import Path

env_path = Path(sys.argv[1])
dest_dir = Path(sys.argv[2]).resolve()
secrets_dir = Path(sys.argv[3]).resolve()

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

# Derive existing basics
user = data.get('FERREX_APP_USER', 'ferrex_app').strip('"')
pwd = data.get('FERREX_APP_PASSWORD', '').strip('"')
dbn = data.get('FERREX_DB', 'ferrex').strip('"')
port = (data.get('DATABASE_INTERNAL_PORT') or data.get('POSTGRES_INTERNAL_PORT') or '5432').strip('"')

# Update directory-bound variables
data['FERREX_CONFIG_DIR'] = str(dest_dir)
data['FERREX_RUNTIME_ENV_FILE'] = str(dest_dir / '.env.runtime')
data['FERREX_SECRETS_DIR'] = str(secrets_dir)
data['POSTGRES_PASSWORD_SECRET_FILE'] = str(secrets_dir / 'postgres_superuser_password')
data['POSTGRES_PASSWORD_FILE'] = data['POSTGRES_PASSWORD_SECRET_FILE']
data['FERREX_APP_PASSWORD_SECRET_FILE'] = str(secrets_dir / 'ferrex_app_password')
data['FERREX_APP_PASSWORD_FILE'] = data['FERREX_APP_PASSWORD_SECRET_FILE']
data['DATABASE_PASSWORD_FILE'] = data['FERREX_APP_PASSWORD_SECRET_FILE']

# Update Tailnet-specific connection targets
data['POSTGRES_INTERNAL_HOST'] = '127.0.0.1'
data['DATABASE_INTERNAL_HOST'] = '127.0.0.1'
data['REDIS_URL_CONTAINER'] = 'redis://127.0.0.1:6379'

# Update container-facing DSN for helpers
if pwd:
    data['DATABASE_URL_CONTAINER'] = f'postgresql://{user}:{pwd}@127.0.0.1:{port}/{dbn}'
else:
    data['DATABASE_URL_CONTAINER'] = f'postgresql://{user}@127.0.0.1:{port}/{dbn}'

# Preserve other variables as-is
write_env(env_path, data)
PY

# Rewrite database host in ferrex.toml to 127.0.0.1
ferrex_python - "$DEST_TOML" <<'PY'
import re
import sys
from urllib.parse import urlsplit, urlunsplit

path = sys.argv[1]
text = open(path, 'r', encoding='utf-8').read()

section_re = re.compile(r"(?ms)^\[database\](.*?)(?:^\[|\Z)")
url_re = re.compile(r"(?m)^\s*url\s*=\s*\"([^\"]*)\"")

def replace_host_in_url(url: str, new_host: str) -> str:
    p = urlsplit(url)
    netloc = p.netloc
    if '@' in netloc:
        userinfo, hostport = netloc.rsplit('@', 1)
        if ':' in hostport:
            _host, port = hostport.split(':', 1)
            hostport = f"{new_host}:{port}"
        else:
            hostport = new_host
        new_netloc = f"{userinfo}@{hostport}"
    else:
        if ':' in netloc:
            _host, port = netloc.split(':', 1)
            new_netloc = f"{new_host}:{port}"
        else:
            new_netloc = new_host
    return urlunsplit((p.scheme, new_netloc, p.path, p.query, p.fragment))

def patch_section(sec: str) -> str:
    m = url_re.search(sec)
    if not m:
        return sec
    old_url = m.group(1)
    new_url = replace_host_in_url(old_url, '127.0.0.1')
    return sec[:m.start(1)] + new_url + sec[m.end(1):]

m = section_re.search(text)
if m:
    patched = patch_section(m.group(1))
    text = text[:m.start(1)] + patched + text[m.end(1):]

with open(path, 'w', encoding='utf-8') as fh:
    fh.write(text)
PY

# Remove stale runtime file so the launcher regenerates a sanitized one
rm -f "$DEST_RUNTIME_ENV" || true

cat <<EOF
Tailnet config prepared at: $TO_DIR
- ferrex.toml updated to use 127.0.0.1 for PostgreSQL
- .env rewritten for target directory and Tailnet endpoints
- secrets copied to: $DEST_SECRETS_DIR (if present in source)

Start Tailnet stack with:
  just start --mode tailscale --config-dir "$(printf '%s' "$TO_DIR")"
EOF
