#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

source "$ROOT_DIR/utils/lib/python.sh"

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
    # Fallback: use subshell cd; requires path to exist.
    (cd "$input" >/dev/null 2>&1 && pwd) || printf '%s\n' "$input"
  fi
}

relative_to_root() {
  local path="$1"
  case "$path" in
    "$ROOT_DIR"/*)
      printf '%s\n' "${path#$ROOT_DIR/}"
      ;;
    *)
      printf '%s\n' "$path"
      ;;
  esac
}

CONFIG_DIR_INPUT="${FERREX_CONFIG_DIR:-$ROOT_DIR/config}"
CONFIG_DIR="$(resolve_path "$CONFIG_DIR_INPUT")"
ENV_FILE="$CONFIG_DIR/.env"
CONFIG_FILE="$CONFIG_DIR/ferrex.toml"

if [ -n "${FERREX_RUNTIME_ENV_FILE:-}" ]; then
  RUNTIME_ENV_FILE="$(resolve_path "${FERREX_RUNTIME_ENV_FILE}")"
else
  RUNTIME_ENV_FILE="$CONFIG_DIR/.env.runtime"
fi
ENV_FILE_DISPLAY="$(relative_to_root "$ENV_FILE")"
RUNTIME_ENV_FILE_DISPLAY="$(relative_to_root "$RUNTIME_ENV_FILE")"
CONFIG_FILE_DISPLAY="$(relative_to_root "$CONFIG_FILE")"
IMAGE="${FERREX_INIT_IMAGE:-ferrex/server:local}"
DOCKERFILE="${FERREX_INIT_DOCKERFILE:-docker/Dockerfile.prod}"
ENABLE_WILD="${FERREX_ENABLE_WILD:-1}"

# Select how to run the config wizard: 'host' (default) or 'docker'.
FERREX_INIT_MODE="${FERREX_INIT_MODE:-host}"

# Compute safe bind-mount suffix only when running via docker.
DETECTED_MOUNT_SUFFIX=""
if [ "$FERREX_INIT_MODE" = "docker" ]; then
  # Rationale:
  # - SELinux hosts need a relabel (':z' or ':Z') to avoid EPERM.
  # - Podman (often rootless) may need ':U' so the mapped UID can write.
  # - Allow explicit override via FERREX_DOCKER_MOUNT_SUFFIX.
  _is_podman=0
  if command -v docker >/dev/null 2>&1; then
    docker_path="$(command -v docker)"
    resolved_docker="$(readlink -f "$docker_path" 2>/dev/null || echo "$docker_path")"
    case "$resolved_docker" in
      *podman*) _is_podman=1 ;;
    esac
  fi

  if [ "${FERREX_DOCKER_MOUNT_SUFFIX+set}" = "set" ]; then
    case "${FERREX_DOCKER_MOUNT_SUFFIX}" in
      :*) DETECTED_MOUNT_SUFFIX="${FERREX_DOCKER_MOUNT_SUFFIX}" ;;
      "") DETECTED_MOUNT_SUFFIX="" ;;
      *) DETECTED_MOUNT_SUFFIX=":${FERREX_DOCKER_MOUNT_SUFFIX}" ;;
    esac
  else
    mount_opts=()
    if command -v getenforce >/dev/null 2>&1; then
      se_mode="$(getenforce 2>/dev/null || echo Disabled)"
      case "$se_mode" in
        Enforcing|Permissive) mount_opts+=("z") ;;
      esac
    elif [ -d "/sys/fs/selinux" ]; then
      mount_opts+=("z")
    fi
    if [ "$_is_podman" = "1" ]; then
      mount_opts+=("U")
    fi
    if [ ${#mount_opts[@]} -gt 0 ]; then
      IFS=, read -r joined <<< "${mount_opts[*]}"
      DETECTED_MOUNT_SUFFIX=":${joined}"
    fi
  fi
fi

if ! ferrex_require_python; then
  exit 1
fi

# Container user mapping (used only in docker mode)
RUN_AS_USER="${FERREX_INIT_RUN_AS_USER:-$(id -u):$(id -g)}"

mkdir -p "$CONFIG_DIR"
touch "$ENV_FILE"

SECRETS_DIR="$CONFIG_DIR/secrets"
POSTGRES_PASSWORD_SECRET_FILE="$SECRETS_DIR/postgres_superuser_password"
FERREX_APP_PASSWORD_SECRET_FILE="$SECRETS_DIR/ferrex_app_password"
SECRETS_DIR_DISPLAY="$(relative_to_root "$SECRETS_DIR")"
POSTGRES_PASSWORD_SECRET_FILE_DISPLAY="$(relative_to_root "$POSTGRES_PASSWORD_SECRET_FILE")"
FERREX_APP_PASSWORD_SECRET_FILE_DISPLAY="$(relative_to_root "$FERREX_APP_PASSWORD_SECRET_FILE")"

if [ "$FERREX_INIT_MODE" = "docker" ]; then
  if [ "${FERREX_INIT_SKIP_BUILD:-0}" = "1" ]; then
    echo "Skipping image build because FERREX_INIT_SKIP_BUILD=1"
  else
    echo "Building Docker image $IMAGE (wild=$ENABLE_WILD, uses cache when unchanged)..."
    docker build -f "$DOCKERFILE" --build-arg ENABLE_WILD="$ENABLE_WILD" -t "$IMAGE" "$ROOT_DIR"
  fi
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
  ferrex_python - "$ENV_FILE" "$key" "$value" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
key = sys.argv[2]
value = sys.argv[3]

lines = []
if path.exists():
    with path.open('r', encoding='utf-8') as fh:
        lines = [line.rstrip('\n') for line in fh]

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

value_to_write = value
if needs_quotes(value):
    value_to_write = escape_value(value)

prefix = f"{key}="
for idx, line in enumerate(lines):
    if line.startswith(prefix):
        lines[idx] = f"{key}={value_to_write}"
        break
else:
    lines.append(f"{key}={value_to_write}")

with path.open('w', encoding='utf-8') as fh:
    for line in lines:
        fh.write(line + '\n')
PY
}

remove_env_var() {
  local key="$1"
  ferrex_python - "$ENV_FILE" "$key" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
key = sys.argv[2]

if not path.exists():
    sys.exit(0)

lines = []
with path.open('r', encoding='utf-8') as fh:
    lines = [line.rstrip('\n') for line in fh]

prefix = f"{key}="
lines = [line for line in lines if not line.startswith(prefix)]

with path.open('w', encoding='utf-8') as fh:
    for line in lines:
        fh.write(line + '\n')
PY
}

sanitize_runtime_env() {
  local source="$1"
  local target="$2"
  local exclude_regex='^(POSTGRES_PASSWORD|POSTGRES_PASSWORD_SECRET_FILE|POSTGRES_PASSWORD_FILE|POSTGRES_USER|POSTGRES_HOST_LOCAL|POSTGRES_INTERNAL_HOST|POSTGRES_PORT|POSTGRES_INTERNAL_PORT|DATABASE_ADMIN_URL|FERREX_APP_PASSWORD|FERREX_APP_PASSWORD_SECRET_FILE|FERREX_APP_PASSWORD_FILE|FERREX_SECRETS_DIR|DATABASE_PASSWORD|DATABASE_PASSWORD_FILE|DATABASE_URL|DATABASE_URL_CONTAINER|DATABASE_URL_HOST)='

  if [ ! -f "$source" ]; then
    rm -f "$target"
    return
  fi

  mkdir -p "$(dirname "$target")"

  {
    echo "# Auto-generated runtime environment (excludes superuser credentials)"
    while IFS= read -r line || [ -n "$line" ]; do
      if [[ -z "$line" ]]; then
        echo "$line"
        continue
      fi
      case "$line" in
        \#*)
          echo "$line"
          continue
        ;;
      esac
      if [[ "$line" =~ $exclude_regex ]]; then
        continue
      fi
      echo "$line"
    done < "$source"
  } > "$target"
}

generate_password() {
  if ferrex_detect_python; then
    ferrex_python - <<'PY'
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
  local new_pwd=""
  local confirm_pwd=""
  local non_interactive="${FERREX_INIT_NON_INTERACTIVE:-0}"
  local rotate="${FERREX_INIT_FORCE_PASSWORD_ROTATION:-0}"

  if [ "$rotate" = "1" ] && [ -n "$current" ]; then
    printf "Rotating existing %s password as requested.\n" "$label"
    current=""
  fi

  if [ -n "$current" ]; then
    if [ "$non_interactive" = "1" ]; then
      printf "Keeping existing %s password (non-interactive mode).\n" "$label"
      return
    fi
    printf "%s password already set in %s.\n" "$label" "$ENV_FILE_DISPLAY"
    local resp=""
    read -p "Keep existing $label password? [Y/n] " resp || true
    resp=${resp:-y}
    if [[ "$resp" =~ ^[Yy]$ ]]; then
      printf "Keeping existing %s password.\n" "$label"
      return
    fi
  fi

  if [ "$non_interactive" = "1" ]; then
    new_pwd="$(generate_password)"
    printf "Generated secure %s password automatically. Stored in %s. Please keep this file safe.\n" "$label" "$ENV_FILE_DISPLAY"
  else
    local choice=""
    read -p "Generate a secure $label password automatically? [Y/n] " choice || true
    choice=${choice:-y}
    if [[ "$choice" =~ ^[Yy]$ ]]; then
      new_pwd="$(generate_password)"
      printf "Generated secure %s password. Stored in %s. Please keep this file safe.\n" "$label" "$ENV_FILE_DISPLAY"
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
      printf "%s password stored in %s. Please keep this file safe.\n" "$label" "$ENV_FILE_DISPLAY"
    fi
  fi

  printf -v "$var_name" '%s' "$new_pwd"
}

maybe_prompt_database_reset() {
  local project_name="${COMPOSE_PROJECT_NAME:-ferrex}"
  local volume_name="${project_name}_postgres-data"

  if ! docker volume inspect "$volume_name" >/dev/null 2>&1; then
    return
  fi

  echo "Detected existing PostgreSQL volume '$volume_name'."
  local reset="no"

  if [ "${FERREX_INIT_FORCE_DB_RESET:-0}" = "1" ]; then
    reset="yes"
    echo "Resetting database volume as requested (FERREX_INIT_FORCE_DB_RESET=1)."
  elif [ "${FERREX_INIT_NON_INTERACTIVE:-0}" = "1" ]; then
    echo "Non-interactive mode; keeping existing database volume."
  else
    local resp=""
    read -p "Reset this database volume? This will permanently delete its contents. [y/N] " resp || true
    resp=${resp:-n}
    if [[ "$resp" =~ ^[Yy]$ ]]; then
      reset="yes"
    fi
  fi

  if [ "$reset" != "yes" ]; then
    echo "Keeping existing database volume."
    return
  fi

  echo "Stopping Docker stack (if running) before reset..."
  COMPOSE_PROJECT_NAME="$project_name" docker compose --env-file "$ENV_FILE" down >/dev/null 2>&1 || true

  if docker volume rm "$volume_name" >/dev/null 2>&1; then
    echo "Removed volume '$volume_name'."
  else
    echo "Warning: failed to remove volume '$volume_name'." >&2
  fi
}

POSTGRES_USER=${POSTGRES_USER:-postgres}
POSTGRES_INITDB_ARGS=${POSTGRES_INITDB_ARGS:-"--auth-host=scram-sha-256 --auth-local=scram-sha-256"}
FERREX_DB=${FERREX_DB:-ferrex}
FERREX_APP_USER=${FERREX_APP_USER:-ferrex_app}
MEDIA_ROOT=${MEDIA_ROOT:-/media}
POSTGRES_HOST_LOCAL=${POSTGRES_HOST_LOCAL:-localhost}
POSTGRES_PORT=${POSTGRES_PORT:-5432}
POSTGRES_INTERNAL_HOST_DEFAULT="db"
POSTGRES_INTERNAL_PORT=${POSTGRES_INTERNAL_PORT:-5432}
REDIS_URL_HOST=${REDIS_URL_HOST:-redis://127.0.0.1:6379}
REDIS_URL_CONTAINER_DEFAULT="redis://cache:6379"

# In tailscale mode, services share the tailscale network namespace and must use 127.0.0.1
if [ "${FERREX_INIT_TAILSCALE:-0}" = "1" ]; then
  POSTGRES_INTERNAL_HOST="127.0.0.1"
  REDIS_URL_CONTAINER="redis://127.0.0.1:6379"
else
  POSTGRES_INTERNAL_HOST="${POSTGRES_INTERNAL_HOST:-$POSTGRES_INTERNAL_HOST_DEFAULT}"
  REDIS_URL_CONTAINER="${REDIS_URL_CONTAINER:-$REDIS_URL_CONTAINER_DEFAULT}"
fi
SERVER_HOST=${SERVER_HOST:-0.0.0.0}
SERVER_PORT=${SERVER_PORT:-3000}
FERREX_SERVER_URL_DEFAULT="http://localhost:${SERVER_PORT}"
SERVER_HOST_INITIAL="$SERVER_HOST"

set_env_var FERREX_CONFIG_DIR "$CONFIG_DIR"
set_env_var FERREX_RUNTIME_ENV_FILE "$RUNTIME_ENV_FILE"
set_env_var POSTGRES_USER "$POSTGRES_USER"
set_env_var POSTGRES_INITDB_ARGS "$POSTGRES_INITDB_ARGS"
set_env_var FERREX_DB "$FERREX_DB"
set_env_var FERREX_APP_USER "$FERREX_APP_USER"
set_env_var MEDIA_ROOT "$MEDIA_ROOT"
set_env_var POSTGRES_HOST_LOCAL "$POSTGRES_HOST_LOCAL"
set_env_var POSTGRES_INTERNAL_HOST "$POSTGRES_INTERNAL_HOST"
set_env_var POSTGRES_PORT "$POSTGRES_PORT"
set_env_var POSTGRES_INTERNAL_PORT "$POSTGRES_INTERNAL_PORT"
set_env_var REDIS_URL "$REDIS_URL_HOST"
set_env_var REDIS_URL_CONTAINER "$REDIS_URL_CONTAINER"
set_env_var SERVER_HOST "$SERVER_HOST"
set_env_var SERVER_PORT "$SERVER_PORT"
set_env_var FERREX_SERVER_URL "$FERREX_SERVER_URL_DEFAULT"
set_env_var FERREX_SECRETS_DIR "$SECRETS_DIR"
set_env_var POSTGRES_PASSWORD_SECRET_FILE "$POSTGRES_PASSWORD_SECRET_FILE"
set_env_var POSTGRES_PASSWORD_FILE "$POSTGRES_PASSWORD_SECRET_FILE"
set_env_var FERREX_APP_PASSWORD_SECRET_FILE "$FERREX_APP_PASSWORD_SECRET_FILE"
set_env_var FERREX_APP_PASSWORD_FILE "$FERREX_APP_PASSWORD_SECRET_FILE"
set_env_var DATABASE_PASSWORD_FILE "$FERREX_APP_PASSWORD_SECRET_FILE"
set_env_var DATABASE_HOST "$POSTGRES_HOST_LOCAL"
set_env_var DATABASE_PORT "$POSTGRES_PORT"
set_env_var DATABASE_NAME "$FERREX_DB"
set_env_var DATABASE_USER "$FERREX_APP_USER"
set_env_var DATABASE_INTERNAL_HOST "$POSTGRES_INTERNAL_HOST"
set_env_var DATABASE_INTERNAL_PORT "$POSTGRES_INTERNAL_PORT"

write_secret_file() {
  local path="$1"
  local value="$2"

  mkdir -p "$(dirname "$path")"
  local original_umask
  original_umask=$(umask)
  umask 0077
  printf '%s\n' "$value" > "$path"
  umask "$original_umask"
  # Allow containers that drop root (e.g., postgres) to read the secrets while
  # keeping write access restricted to the host user.
  chmod 0644 "$path" >/dev/null 2>&1 || true
}

prompt_password POSTGRES_PASSWORD "Postgres superuser"
prompt_password FERREX_APP_PASSWORD "Ferrex application"

write_secret_file "$POSTGRES_PASSWORD_SECRET_FILE" "$POSTGRES_PASSWORD"
write_secret_file "$FERREX_APP_PASSWORD_SECRET_FILE" "$FERREX_APP_PASSWORD"

set_env_var POSTGRES_PASSWORD "$POSTGRES_PASSWORD"
set_env_var FERREX_APP_PASSWORD "$FERREX_APP_PASSWORD"
set_env_var DATABASE_PASSWORD "$FERREX_APP_PASSWORD"

echo "Secret files stored in $SECRETS_DIR_DISPLAY:"
echo "  - Superuser password: $POSTGRES_PASSWORD_SECRET_FILE_DISPLAY"
echo "  - Application password: $FERREX_APP_PASSWORD_SECRET_FILE_DISPLAY"

DATABASE_URL_HOST="postgresql://${FERREX_APP_USER}:${FERREX_APP_PASSWORD}@${POSTGRES_HOST_LOCAL}:${POSTGRES_PORT}/${FERREX_DB}"
DATABASE_URL_CONTAINER="postgresql://${FERREX_APP_USER}:${FERREX_APP_PASSWORD}@${POSTGRES_INTERNAL_HOST}:${POSTGRES_INTERNAL_PORT}/${FERREX_DB}"
set_env_var DATABASE_URL "$DATABASE_URL_HOST"
set_env_var DATABASE_URL_CONTAINER "$DATABASE_URL_CONTAINER"

export FERREX_CONFIG_INIT_DATABASE_URL="$DATABASE_URL_CONTAINER"
export FERREX_CONFIG_INIT_HOST_DATABASE_URL="$DATABASE_URL_HOST"
export FERREX_CONFIG_INIT_REDIS_URL="$REDIS_URL_CONTAINER"
export FERREX_CONFIG_INIT_HOST_REDIS_URL="$REDIS_URL_HOST"

unset FERREX_APP_PASSWORD
unset POSTGRES_PASSWORD

echo "Configuration secrets stored in $ENV_FILE_DISPLAY. Back up this file securely."

SHOULD_RUN_WIZARD=true
FORCE_FLAG=""
if [ -f "$CONFIG_FILE" ]; then
  if [ "${FERREX_INIT_FORCE_CONFIG:-0}" = "1" ]; then
    FORCE_FLAG="--force"
  elif [ "${FERREX_INIT_NON_INTERACTIVE:-0}" = "1" ]; then
    SHOULD_RUN_WIZARD=false
  else
    local_resp=""
    read -p "$CONFIG_FILE_DISPLAY already exists. Re-run interactive wizard to overwrite/update it? [y/N] " local_resp || true
    local_resp=${local_resp:-n}
    if [[ ! "$local_resp" =~ ^[Yy]$ ]]; then
      SHOULD_RUN_WIZARD=false
    else
      FORCE_FLAG="--force"
    fi
  fi
fi

if [ "$SHOULD_RUN_WIZARD" = true ]; then
  ENV_TMP="$CONFIG_DIR/.env.generated"
  rm -f "$ENV_TMP"

  if [ "$FORCE_FLAG" = "--force" ]; then
    maybe_prompt_database_reset
  fi

  NON_INTERACTIVE_FLAG=""
  if [ "${FERREX_INIT_NON_INTERACTIVE:-0}" = "1" ]; then
    NON_INTERACTIVE_FLAG="--non-interactive"
  fi

  if [ "$FERREX_INIT_MODE" = "docker" ]; then
    DOCKER_TTY_ARGS=()
    if [ "${FERREX_INIT_NON_INTERACTIVE:-0}" != "1" ]; then
      DOCKER_TTY_ARGS=(-it)
    fi

    echo "Running ferrex-server config init in an isolated container with safe mount flags ($DETECTED_MOUNT_SUFFIX)..."
    docker run --rm "${DOCKER_TTY_ARGS[@]}" \
      --user "$RUN_AS_USER" \
      --entrypoint /usr/local/bin/ferrex-server \
      -v "$CONFIG_DIR":/app/config${DETECTED_MOUNT_SUFFIX} \
      -e FERREX_CONFIG_INIT_DATABASE_URL="$FERREX_CONFIG_INIT_DATABASE_URL" \
      -e FERREX_CONFIG_INIT_HOST_DATABASE_URL="$FERREX_CONFIG_INIT_HOST_DATABASE_URL" \
      -e FERREX_CONFIG_INIT_REDIS_URL="$FERREX_CONFIG_INIT_REDIS_URL" \
      -e FERREX_CONFIG_INIT_HOST_REDIS_URL="$FERREX_CONFIG_INIT_HOST_REDIS_URL" \
      "$IMAGE" \
      config init --config-path /app/config/ferrex.toml --env-path /app/config/.env.generated $FORCE_FLAG $NON_INTERACTIVE_FLAG "$@"
  else
    if ! command -v cargo >/dev/null 2>&1; then
      echo "Error: cargo is required for host-native init (install Rust toolchain) or set FERREX_INIT_MODE=docker." >&2
      exit 1
    fi
    echo "Running ferrex-server config init natively on host..."
    # Use explicit manifest path to avoid CWD assumptions.
    env \
      FERREX_CONFIG_INIT_DATABASE_URL="$FERREX_CONFIG_INIT_DATABASE_URL" \
      FERREX_CONFIG_INIT_HOST_DATABASE_URL="$FERREX_CONFIG_INIT_HOST_DATABASE_URL" \
      FERREX_CONFIG_INIT_REDIS_URL="$FERREX_CONFIG_INIT_REDIS_URL" \
      FERREX_CONFIG_INIT_HOST_REDIS_URL="$FERREX_CONFIG_INIT_HOST_REDIS_URL" \
      cargo run --manifest-path "$ROOT_DIR/Cargo.toml" -p ferrex-server -- \
        config init --config-path "$CONFIG_FILE" --env-path "$ENV_TMP" $FORCE_FLAG $NON_INTERACTIVE_FLAG "$@"
  fi

  if [ -f "$ENV_TMP" ]; then
    while IFS= read -r line; do
      [ -z "$line" ] && continue
      case "$line" in
        \#*) continue ;;
      esac
      key="${line%%=*}"
      value="${line#*=}"
      if [ "$key" = "SERVER_HOST" ]; then
        normalized="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]' | tr -d ' ')"
        case "$normalized" in
          127.*|localhost|::1|\[::1\])
            if [ -n "$SERVER_HOST_INITIAL" ]; then
              initial_normalized="$(printf '%s' "$SERVER_HOST_INITIAL" | tr '[:upper:]' '[:lower:]' | tr -d ' ')"
              case "$initial_normalized" in
                127.*|localhost|::1|\[::1\])
                  set_env_var "$key" "$value"
                  SERVER_HOST="$value"
                ;;
                *)
                  echo "Keeping existing SERVER_HOST=$SERVER_HOST_INITIAL to remain reachable (wizard suggested loopback '$value')."
                  set_env_var "$key" "$SERVER_HOST_INITIAL"
                  SERVER_HOST="$SERVER_HOST_INITIAL"
                ;;
              esac
            else
              set_env_var "$key" "$value"
              SERVER_HOST="$value"
            fi
            continue
          ;;
        esac
      fi
      set_env_var "$key" "$value"
      if [ "$key" = "SERVER_HOST" ]; then
        SERVER_HOST="$value"
      fi
    done < "$ENV_TMP"
    rm -f "$ENV_TMP"
  fi

  echo "Configuration wizard complete."

  SERVER_PORT_ACTUAL="$SERVER_PORT"
  if ferrex_detect_python; then
    if ! SERVER_PORT_ACTUAL=$(ferrex_python - "$CONFIG_FILE" <<'PY'
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
    ); then
      SERVER_PORT_ACTUAL="$SERVER_PORT"
    fi
  fi
  SERVER_PORT_ACTUAL="${SERVER_PORT_ACTUAL//$'\n'/}"
  if [ -z "$SERVER_PORT_ACTUAL" ]; then
    SERVER_PORT_ACTUAL="$SERVER_PORT"
  fi

  if grep -q "enforce_https = true" "$CONFIG_FILE"; then
    set_env_var FERREX_SERVER_URL "https://localhost:${SERVER_PORT_ACTUAL}"
  else
    set_env_var FERREX_SERVER_URL "http://localhost:${SERVER_PORT_ACTUAL}"
  fi

  if [ ! -s "$CONFIG_FILE" ]; then
    echo "Error: ferrex-server produced an empty config at $CONFIG_FILE" >&2
    exit 1
  fi
  # Ensure container runtime uses the container path for config resolution regardless of init mode.
  # This avoids leaking host absolute paths into container env and breaking config loading.
  set_env_var FERREX_CONFIG_PATH "/app/config/ferrex.toml"
else
  echo "Skipping config wizard; existing $CONFIG_FILE_DISPLAY preserved."
fi

sanitize_runtime_env "$ENV_FILE" "$RUNTIME_ENV_FILE"

SETUP_TOKEN_VALUE=""
if ferrex_detect_python; then
  SETUP_TOKEN_VALUE="$(
    ferrex_python - "$CONFIG_FILE" <<'PY'
import sys
from pathlib import Path
try:
    import tomllib
except ModuleNotFoundError:  # python <3.11 without stdlib toml parser
    import tomli as tomllib  # type: ignore[import-not-found]

config_path = Path(sys.argv[1])
if not config_path.exists():
    raise SystemExit(0)

with config_path.open('rb') as handle:
    data = tomllib.load(handle)

token = (data.get('auth') or {}).get('setup_token') or ''
print(token)
PY
  )"
fi

SETUP_TOKEN_VALUE="$(printf '%s' "$SETUP_TOKEN_VALUE" | tr -d '\r\n')"
if [ -n "$SETUP_TOKEN_VALUE" ]; then
  echo "Setup token stored in $CONFIG_FILE_DISPLAY ([auth].setup_token):"
  echo "  $SETUP_TOKEN_VALUE"
  echo "Re-run 'just show-setup-token' later if you need to display it again."
fi

echo "Runtime environment stored at $RUNTIME_ENV_FILE_DISPLAY (superuser credentials excluded)."

cat <<'EOF'

Next steps:
  - Run `just start` for a local stack or `just start --mode tailscale` to include the Tailnet sidecar. Add `--config-dir ...` to target an alternate configuration directory.
  - Run `just rebuild-server` if you have updated the server sources since the last build.
  - Use `just tailscale-serve` after the tailnet stack is running to enable HTTPS proxying.
  - Use `just check-config` to validate connectivity when needed.
  - Update `FERREX_SERVER_URL` in "$ENV_FILE_DISPLAY" if your Tailnet hostname differs from localhost.
  - The server binds to 0.0.0.0 by default so containers and Tailnet access stay reachable; change SERVER_HOST to 127.0.0.1 if you explicitly want localhost-only.
EOF
