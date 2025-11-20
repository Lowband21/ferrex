#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_DIR="$ROOT_DIR/config"
ENV_FILE="$CONFIG_DIR/.env"
IMAGE="${FERREX_INIT_IMAGE:-ferrex/server:local}"
DOCKERFILE="${FERREX_INIT_DOCKERFILE:-docker/Dockerfile.prod}"

mkdir -p "$CONFIG_DIR"
touch "$ENV_FILE"

if [ "${FERREX_INIT_SKIP_BUILD:-0}" != "1" ]; then
  echo "Building Docker image $IMAGE (uses cache when unchanged)..."
  docker build -f "$DOCKERFILE" -t "$IMAGE" "$ROOT_DIR"
else
  echo "Skipping image build because FERREX_INIT_SKIP_BUILD=1"
fi

echo "Preparing configuration environment at $CONFIG_DIR"

if [ -s "$ENV_FILE" ]; then
  set -a
  source "$ENV_FILE"
  set +a
fi

set_env_var() {
  local key="$1"
  local value="$2"
  python3 - "$ENV_FILE" "$key" "$value" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
key = sys.argv[2]
value = sys.argv[3]

lines = []
if path.exists():
    with path.open('r', encoding='utf-8') as fh:
        lines = [line.rstrip('\n') for line in fh]

prefix = f"{key}="
for idx, line in enumerate(lines):
    if line.startswith(prefix):
        lines[idx] = f"{key}={value}"
        break
else:
    lines.append(f"{key}={value}")

with path.open('w', encoding='utf-8') as fh:
    for line in lines:
        fh.write(line + '\n')
PY
}

generate_password() {
  if command -v python3 >/dev/null 2>&1; then
    python3 - <<'PY'
import secrets
print(secrets.token_urlsafe(32))
PY
  elif command -v python >/dev/null 2>&1; then
    python - <<'PY'
import secrets
print(secrets.token_urlsafe(32))
PY
  elif command -v openssl >/dev/null 2>&1; then
    openssl rand -base64 32 | tr -d '\n'
  else
    head -c 48 /dev/urandom | base64 | tr -d '\n' | cut -c1-43
  fi
}

prompt_password() {
  local var_name="$1"
  local label="$2"
  local current="${!var_name:-}"
  local new_pwd="$current"
  local confirm_pwd=""

  if [ -n "$current" ]; then
    printf "%s password already set in config/.env.\n" "$label"
    local resp=""
    read -p "Keep existing $label password? [Y/n] " resp || true
    resp=${resp:-y}
    if [[ "$resp" =~ ^[Yy]$ ]]; then
      printf "Keeping existing %s password.\n" "$label"
      return
    fi
  fi

  local choice=""
  read -p "Generate a secure $label password automatically? [Y/n] " choice || true
  choice=${choice:-y}
  if [[ "$choice" =~ ^[Yy]$ ]]; then
    new_pwd="$(generate_password)"
    printf "Generated secure %s password. Stored in config/.env. Please keep this file safe.\n" "$label"
  else
    while true; do
      read -s -p "Enter $label password: " new_pwd || true
      echo
      read -s -p "Confirm $label password: " confirm_pwd || true
      echo
      if [ -z "$new_pwd" ]; then
        echo "Password cannot be empty."
      elif [ "$new_pwd" != "$confirm_pwd" ]; then
        echo "Passwords do not match. Try again."
      else
        break
      fi
    done
    unset confirm_pwd
    printf "%s password stored in config/.env. Please keep this file safe.\n" "$label"
  fi

  printf -v "$var_name" '%s' "$new_pwd"
  set_env_var "$var_name" "$new_pwd"
}

POSTGRES_USER=${POSTGRES_USER:-postgres}
FERREX_DB=${FERREX_DB:-ferrex}
FERREX_APP_USER=${FERREX_APP_USER:-ferrex_app}
MEDIA_ROOT=${MEDIA_ROOT:-/media}
REDIS_URL=${REDIS_URL:-redis://cache:6379}
SERVER_HOST=${SERVER_HOST:-0.0.0.0}
SERVER_PORT=${SERVER_PORT:-3000}
FERREX_SERVER_URL_DEFAULT="http://localhost:${SERVER_PORT}"

set_env_var POSTGRES_USER "$POSTGRES_USER"
set_env_var FERREX_DB "$FERREX_DB"
set_env_var FERREX_APP_USER "$FERREX_APP_USER"
set_env_var MEDIA_ROOT "$MEDIA_ROOT"
set_env_var REDIS_URL "$REDIS_URL"
set_env_var SERVER_HOST "$SERVER_HOST"
set_env_var SERVER_PORT "$SERVER_PORT"
set_env_var FERREX_SERVER_URL "$FERREX_SERVER_URL_DEFAULT"

prompt_password POSTGRES_PASSWORD "Postgres superuser"
prompt_password FERREX_APP_PASSWORD "Ferrex application"

DATABASE_URL="postgresql://${FERREX_APP_USER}:${FERREX_APP_PASSWORD}@db:5432/${FERREX_DB}"
set_env_var DATABASE_URL "$DATABASE_URL"

echo "Configuration secrets stored in config/.env. Back up this file securely."

SHOULD_RUN_WIZARD=true
FORCE_FLAG=""
if [ -f "$CONFIG_DIR/ferrex.toml" ]; then
  local_resp=""
  read -p "config/ferrex.toml already exists. Re-run interactive wizard to overwrite/update it? [y/N] " local_resp || true
  local_resp=${local_resp:-n}
  if [[ ! "$local_resp" =~ ^[Yy]$ ]]; then
    SHOULD_RUN_WIZARD=false
  else
    FORCE_FLAG="--force"
  fi
fi

if [ "$SHOULD_RUN_WIZARD" = true ]; then
  ENV_TMP="$CONFIG_DIR/.env.generated"
  rm -f "$ENV_TMP"

  echo "Running ferrex-server config init inside container..."
  FERREX_CONTAINER="${FERREX_CONTAINER_NAME:-ferrex_media_server}"
  if docker ps --format '{{.Names}}' | grep -Fxq "$FERREX_CONTAINER"; then
    echo "Reusing running container $FERREX_CONTAINER for config init."
    docker exec -it \
      --user 0:0 \
      -e DATABASE_URL="$DATABASE_URL" \
      -e REDIS_URL="$REDIS_URL" \
      -e MEDIA_ROOT="$MEDIA_ROOT" \
      -e SERVER_HOST="$SERVER_HOST" \
      -e SERVER_PORT="$SERVER_PORT" \
      -e FERREX_CONFIG_INIT_DATABASE_URL="$DATABASE_URL" \
      -e FERREX_CONFIG_INIT_REDIS_URL="$REDIS_URL" \
      "$FERREX_CONTAINER" \
      ferrex-server config init --config-path /app/config/ferrex.toml --env-path /app/config/.env.generated $FORCE_FLAG "$@"
  else
    docker run --rm -it \
      --user 0:0 \
      -v "$CONFIG_DIR":/app/config \
      -e DATABASE_URL="$DATABASE_URL" \
      -e REDIS_URL="$REDIS_URL" \
      -e MEDIA_ROOT="$MEDIA_ROOT" \
      -e SERVER_HOST="$SERVER_HOST" \
      -e SERVER_PORT="$SERVER_PORT" \
      -e FERREX_CONFIG_INIT_DATABASE_URL="$DATABASE_URL" \
      -e FERREX_CONFIG_INIT_REDIS_URL="$REDIS_URL" \
      "$IMAGE" \
      config init --config-path /app/config/ferrex.toml --env-path /app/config/.env.generated $FORCE_FLAG "$@"
  fi

  if [ -f "$ENV_TMP" ]; then
    while IFS= read -r line; do
      [ -z "$line" ] && continue
      case "$line" in
        \#*) continue ;;
      esac
      key="${line%%=*}"
      value="${line#*=}"
      if ! grep -q "^${key}=" "$ENV_FILE"; then
        set_env_var "$key" "$value"
      fi
    done < "$ENV_TMP"
    rm -f "$ENV_TMP"
  fi

  echo "Configuration wizard complete."

  SERVER_PORT_ACTUAL="$SERVER_PORT"
  if command -v python3 >/dev/null 2>&1; then
    SERVER_PORT_ACTUAL=$(python3 - "$CONFIG_DIR/ferrex.toml" <<'PY' || echo "$SERVER_PORT")
import sys
try:
    import tomllib
except ModuleNotFoundError:  # python <3.11 without stdlib toml parser
    import tomli as tomllib  # type: ignore[import-not-found]

path = sys.argv[1]
with open(path, 'rb') as handle:
    data = tomllib.load(handle)
print(data.get('server', {}).get('port', 3000))
PY
  fi
  SERVER_PORT_ACTUAL="${SERVER_PORT_ACTUAL//$'\n'/}"
  if [ -z "$SERVER_PORT_ACTUAL" ]; then
    SERVER_PORT_ACTUAL="$SERVER_PORT"
  fi

  if grep -q "enforce_https = true" "$CONFIG_DIR/ferrex.toml"; then
    set_env_var FERREX_SERVER_URL "https://localhost:${SERVER_PORT_ACTUAL}"
  else
    set_env_var FERREX_SERVER_URL "http://localhost:${SERVER_PORT_ACTUAL}"
  fi

  if [ ! -s "$CONFIG_DIR/ferrex.toml" ]; then
    echo "Error: ferrex-server produced an empty config at $CONFIG_DIR/ferrex.toml" >&2
    exit 1
  fi
else
  echo "Skipping config wizard; existing config/ferrex.toml preserved."
fi

cat <<'EOF'

Next steps:
  - Run `just rebuild-server` if you have updated the server sources since the last build.
  - Run `just up` for a local stack or `just up-tailscale` to include the Tailnet sidecar.
  - Use `just tailscale-serve` after the tailnet stack is running to enable HTTPS proxying.
  - Use `just check-config` to validate connectivity when needed.
  - Update `FERREX_SERVER_URL` in config/.env if your Tailnet hostname differs from localhost.
EOF
