#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

source "$ROOT_DIR/utils/lib/python.sh"

usage() {
  cat <<'EOF'
Usage: stack-up.sh [options] [-- <config-init-args>...]

Options:
  --mode <local|tailscale>      Stack mode to launch (default: local)
  --profile <cargo-profile>     Cargo build profile for server image (default: release)
  --config-dir <path>           Host directory for config artifacts (default: ./config)
  --rust-log <level(s)>         Override RUST_LOG for the runtime containers (default: leave unchanged)
  --wild                        Force-enable the wild linker (default: enabled)
  --no-wild                     Disable the wild linker (overrides default)
  --clean                       Remove existing stack containers before starting
  --interactive                 Run init-config prompts interactively (default: non-interactive)
  --force-init                  Force re-running the config wizard (implies --interactive unless overridden)
  --reset-db                    Force a database volume reset before reinitialising
  -h, --help                    Show this message and exit

Any arguments after `--` are passed straight to `utils/init-config.sh`,
which forwards them to `ferrex-server config init`.
EOF
}

MODE="local"
PROFILE="${FERREX_BUILD_PROFILE:-release}"
CONFIG_DIR_OVERRIDE=""
INTERACTIVE=0
FORCE_INIT=0
FORCE_DB_RESET=0
CLEAN=0
RUST_LOG_VALUE="${RUST_LOG:-}"
ENABLE_WILD="${FERREX_ENABLE_WILD:-1}"
INIT_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      [[ $# -ge 2 ]] || { echo "Missing value for --mode" >&2; usage; exit 1; }
      MODE="$2"
      shift 2
      ;;
    --profile)
      [[ $# -ge 2 ]] || { echo "Missing value for --profile" >&2; usage; exit 1; }
      PROFILE="$2"
      shift 2
      ;;
    --config-dir)
      [[ $# -ge 2 ]] || { echo "Missing value for --config-dir" >&2; usage; exit 1; }
      CONFIG_DIR_OVERRIDE="$2"
      shift 2
      ;;
    --rust-log)
      [[ $# -ge 2 ]] || { echo "Missing value for --rust-log" >&2; usage; exit 1; }
      RUST_LOG_VALUE="$2"
      shift 2
      ;;
    --clean)
      CLEAN=1
      shift
      ;;
    --wild)
      ENABLE_WILD=1
      shift
      ;;
    --no-wild)
      ENABLE_WILD=0
      shift
      ;;
    --interactive)
      INTERACTIVE=1
      shift
      ;;
    --force-init)
      FORCE_INIT=1
      shift
      ;;
    --reset-db)
      FORCE_DB_RESET=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      INIT_ARGS+=("$@")
      break
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

case "$MODE" in
  local|tailscale) ;;
  *)
    echo "Unsupported mode: $MODE (expected 'local' or 'tailscale')" >&2
    exit 1
    ;;
esac

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

CONFIG_DIR_RAW="${CONFIG_DIR_OVERRIDE:-$ROOT_DIR/config}"
CONFIG_DIR="$(resolve_path "$CONFIG_DIR_RAW")"
ENV_FILE="$CONFIG_DIR/.env"

mkdir -p "$CONFIG_DIR"

run_bootstrap() {
  local include_force="${1:-0}"
  local env_vars=(
    "FERREX_CONFIG_DIR=$CONFIG_DIR"
    "FERREX_INIT_NON_INTERACTIVE=$INIT_NON_INTERACTIVE"
    "FERREX_ENABLE_WILD=$ENABLE_WILD"
    "FERREX_INIT_MODE=${FERREX_INIT_MODE:-host}"
  )
  if [[ "$MODE" = "tailscale" ]]; then
    env_vars+=("FERREX_INIT_TAILSCALE=1")
  fi

  if [[ "$include_force" = "force" ]]; then
    if [[ "$FORCE_INIT" -eq 1 || "$FORCE_DB_RESET" -eq 1 ]]; then
      env_vars+=("FERREX_INIT_FORCE_CONFIG=1")
    fi
    if [[ "$FORCE_DB_RESET" -eq 1 ]]; then
      env_vars+=("FERREX_INIT_FORCE_DB_RESET=1")
    fi
  fi

  echo "Running configuration bootstrap via utils/init-config.sh..."
  (
    cd "$ROOT_DIR"
    env "${env_vars[@]}" utils/init-config.sh "${INIT_ARGS[@]}"
  )
}

force_init_needed() {
  [[ "$FORCE_INIT" -eq 1 ]] && return 0
  [[ "$FORCE_DB_RESET" -eq 1 ]] && return 0
  [[ ! -s "$ENV_FILE" ]] && return 0
  return 1
}

if [[ "$INTERACTIVE" -eq 1 ]]; then
  INIT_NON_INTERACTIVE=0
else
  INIT_NON_INTERACTIVE=1
fi

export FERREX_ENABLE_WILD="$ENABLE_WILD"

if force_init_needed; then
  run_bootstrap force
fi

if [[ ! -s "$ENV_FILE" ]]; then
  echo "Configuration file $ENV_FILE is missing or empty even after bootstrap attempt." >&2
  exit 1
fi

set -a
source "$ENV_FILE"
set +a

export FERREX_CONFIG_DIR="${FERREX_CONFIG_DIR:-$CONFIG_DIR}"
export FERREX_ENV_FILE="$ENV_FILE"

config_basename="$(basename "$FERREX_CONFIG_DIR")"
config_slug="$(printf '%s' "$config_basename" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-')"
if [[ -z "$config_slug" ]]; then
  config_slug="default"
fi
if [[ "$config_slug" = "config" ]]; then
  project_name="ferrex"
else
  project_name="ferrex-${config_slug}"
fi
export COMPOSE_PROJECT_NAME="$project_name"


export FERREX_BUILD_PROFILE="$PROFILE"

compose_files=("$ROOT_DIR/docker-compose.yml")
if [[ "$MODE" = "tailscale" ]]; then
  compose_files+=("$ROOT_DIR/docker-compose.tailscale.yml")
fi

compose_args=()
for file in "${compose_files[@]}"; do
  compose_args+=(-f "$file")
done

if [[ "$CLEAN" -eq 1 ]]; then
  echo "Cleaning existing stack containers..."
  containers=(ferrex_media_db ferrex_media_cache ferrex_media_server)
  if [[ "$MODE" = "tailscale" ]]; then
    containers+=("ferrex_tailscale")
  fi
  docker rm -f "${containers[@]}" >/dev/null 2>&1 || true

  echo "Building ferrex/server:local (profile=$PROFILE, wild=$ENABLE_WILD)..."
  docker build -f "$ROOT_DIR/docker/Dockerfile.prod" \
    --build-arg BUILD_PROFILE="$PROFILE" \
    --build-arg ENABLE_WILD="$ENABLE_WILD" \
    -t ferrex/server:local "$ROOT_DIR"
fi

if [[ -n "$RUST_LOG_VALUE" ]]; then
  export RUST_LOG="$RUST_LOG_VALUE"
fi

echo "Bringing up stack (mode=$MODE, profile=$PROFILE, config=$FERREX_CONFIG_DIR, project=$COMPOSE_PROJECT_NAME)..."
docker compose "${compose_args[@]}" --env-file "$ENV_FILE" up -d

if [[ "$MODE" = "tailscale" ]]; then
  echo "Ensuring Tailscale Serve proxies https:// to http://127.0.0.1:3000..."
  serve_log="$(mktemp -t tailscale-serve.XXXXXX)"
  if docker compose "${compose_args[@]}" --env-file "$ENV_FILE" exec -T tailscale tailscale serve --bg http://127.0.0.1:3000 >"$serve_log" 2>&1; then
    tail -n +1 "$serve_log"
    rm -f "$serve_log"
  else
    echo "Warning: failed to configure Tailscale Serve automatically." >&2
    echo "You may need to authenticate the Tailscale sidecar, then run:" >&2
    echo "  docker compose ${compose_args[*]} --env-file \"$ENV_FILE\" exec tailscale tailscale serve --bg http://127.0.0.1:3000" >&2
    echo "Detailed error output:" >&2
    cat "$serve_log" >&2 || true
    rm -f "$serve_log"
  fi
fi

echo "Stack is running. Useful commands:"
echo "  docker compose ${compose_args[*]} --env-file \"$ENV_FILE\" ps"
echo "  docker compose ${compose_args[*]} --env-file \"$ENV_FILE\" logs -f ferrex"
