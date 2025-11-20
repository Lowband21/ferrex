# This file can be detected and used directly from subfolders
# Useful for keeping in sync with multiple projects/worktrees
# Uses the .env from it's containing folder

set dotenv-path := "config/.env"
set dotenv-override := true

########################################
# Docker Compose shortcuts (stack + cfg)
########################################

[no-cd]
[no-exit-message]
down:
    #if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    @docker stop -f ferrex_media_db ferrex_media_cache ferrex_media_server || true
    @docker rm -f ferrex_media_db ferrex_media_cache ferrex_media_server || true
    @COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env down || true
    @docker network rm -f ferrex_default || true

[no-cd]
[no-exit-message]
down-tailscale:
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    # Remove containers up front to avoid Podman pod teardown issues and noisy errors.
    @docker rm -f ferrex_media_db ferrex_media_cache ferrex_media_server ferrex_tailscale >/dev/null 2>&1 || true
    # Compose down after hard removal to clean up network/resources; suppress output.
    @COMPOSE_PROJECT_NAME=ferrex docker compose -f docker-compose.yml -f docker-compose.tailscale.yml --env-file config/.env down >/dev/null 2>&1 || true
    # Best-effort project network cleanup.
    @docker network rm ferrex_default >/dev/null 2>&1 || true

[no-cd]
logs service="ferrex":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env logs -f {{ service }}

# Override the default Serve mapping configured during stack startup.
[no-cd]
tailscale-serve target="http://127.0.0.1:3000":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    COMPOSE_PROJECT_NAME=ferrex docker compose -f docker-compose.yml -f docker-compose.tailscale.yml --env-file config/.env exec tailscale tailscale serve --bg {{ target }}

[no-cd]
start *args:
    utils/stack-up.sh {{ args }}

# Interactive config init (host-native by default).

# Writes config/ferrex.toml and config/.env on the host via volume mounts.
[no-cd]
init-config args="" FERREX_INIT_SKIP_BUILD="0" FERREX_INIT_MODE="host":
    FERREX_INIT_MODE={{ FERREX_INIT_MODE }} utils/init-config.sh {{ args }}

[no-cd]
config args="" FERREX_INIT_SKIP_BUILD="1" FERREX_INIT_MODE="host":
    FERREX_INIT_MODE={{ FERREX_INIT_MODE }} utils/init-config.sh {{ args }}

[no-cd]
config-tailnet from_dir="config" to_dir="config/tailnet" args="":
    bash utils/make-tailnet-config.sh --from {{ from_dir }} --to {{ to_dir }} {{ args }}

[no-cd]
show-setup-token config_file="config/ferrex.toml":
    if [ ! -f {{ config_file }} ]; then \
        echo "Config missing: {{ config_file }}. Run: just init-config" >&2; \
        exit 1; \
    else \
        utils/show-setup-token.sh {{ config_file }}; \
    fi

[no-cd]
rebuild-server profile="release" wild="1":
    docker build -f docker/Dockerfile.prod --build-arg BUILD_PROFILE={{ profile }} --build-arg ENABLE_WILD={{ wild }} -t ferrex/server:local .

# Validate configuration (will also sanity-check DB/Redis connectivity if present)
[no-cd]
check-config profile="release" args="" wild="1":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    docker rm -f ferrex_media_db ferrex_media_cache ferrex_media_server >/dev/null 2>&1 || true
    FERREX_BUILD_PROFILE={{ profile }} COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env up -d db cache
    # Always build with the requested linker mode; BuildKit will reuse cache.
    docker build -f docker/Dockerfile.prod --build-arg BUILD_PROFILE={{ profile }} --build-arg ENABLE_WILD={{ wild }} -t ferrex/server:local .
    if [ -d "$PWD/config/secrets" ]; then \
      docker run --rm \
        --network ferrex_default \
        -v "$PWD/config":/app/config \
        -v "$PWD/config/secrets":/run/ferrex-secrets:ro \
        -e FERREX_APP_PASSWORD_FILE=/run/ferrex-secrets/ferrex_app_password \
        -e DATABASE_PASSWORD_FILE=/run/ferrex-secrets/ferrex_app_password \
        -e POSTGRES_PASSWORD_FILE=/run/ferrex-secrets/postgres_superuser_password \
        -e DATABASE_HOST="${POSTGRES_INTERNAL_HOST:-db}" \
        -e DATABASE_PORT="${POSTGRES_INTERNAL_PORT:-5432}" \
        -e DATABASE_NAME="${FERREX_DB:-ferrex}" \
        -e DATABASE_USER="${FERREX_APP_USER:-ferrex_app}" \
        ferrex/server:local \
        config check --config-path /app/config/ferrex.toml --env-file /app/config/.env.runtime {{ args }}; \
    else \
      docker run --rm \
        --network ferrex_default \
        -v "$PWD/config":/app/config \
        -e DATABASE_HOST="${POSTGRES_INTERNAL_HOST:-db}" \
        -e DATABASE_PORT="${POSTGRES_INTERNAL_PORT:-5432}" \
        -e DATABASE_NAME="${FERREX_DB:-ferrex}" \
        -e DATABASE_USER="${FERREX_APP_USER:-ferrex_app}" \
        ferrex/server:local \
        config check --config-path /app/config/ferrex.toml --env-file /app/config/.env.runtime {{ args }}; \
    fi

[no-cd]
db-preflight profile="release" args="" wild="1":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    docker rm -f ferrex_media_db ferrex_media_cache ferrex_media_server >/dev/null 2>&1 || true
    FERREX_BUILD_PROFILE={{ profile }} COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env up -d db
    docker build -f docker/Dockerfile.prod --build-arg BUILD_PROFILE={{ profile }} --build-arg ENABLE_WILD={{ wild }} -t ferrex/server:local .
    if [ -d "$PWD/config/secrets" ]; then \
      docker run --rm \
        --network ferrex_default \
        -v "$PWD/config":/app/config \
        -v "$PWD/config/secrets":/run/ferrex-secrets:ro \
        -e FERREX_APP_PASSWORD_FILE=/run/ferrex-secrets/ferrex_app_password \
        -e DATABASE_PASSWORD_FILE=/run/ferrex-secrets/ferrex_app_password \
        -e POSTGRES_PASSWORD_FILE=/run/ferrex-secrets/postgres_superuser_password \
        -e DATABASE_HOST="${POSTGRES_INTERNAL_HOST:-db}" \
        -e DATABASE_PORT="${POSTGRES_INTERNAL_PORT:-5432}" \
        -e DATABASE_NAME="${FERREX_DB:-ferrex}" \
        -e DATABASE_USER="${FERREX_APP_USER:-ferrex_app}" \
        ferrex/server:local \
        db preflight {{ args }}; \
    else \
      docker run --rm \
        --network ferrex_default \
        -v "$PWD/config":/app/config \
        -e DATABASE_HOST="${POSTGRES_INTERNAL_HOST:-db}" \
        -e DATABASE_PORT="${POSTGRES_INTERNAL_PORT:-5432}" \
        -e DATABASE_NAME="${FERREX_DB:-ferrex}" \
        -e DATABASE_USER="${FERREX_APP_USER:-ferrex_app}" \
        ferrex/server:local \
        db preflight {{ args }}; \
    fi

[no-cd]
db-migrate profile="release" args="" wild="1":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    docker rm -f ferrex_media_db ferrex_media_cache ferrex_media_server >/dev/null 2>&1 || true
    FERREX_BUILD_PROFILE={{ profile }} COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env up -d db
    docker build -f docker/Dockerfile.prod --build-arg BUILD_PROFILE={{ profile }} --build-arg ENABLE_WILD={{ wild }} -t ferrex/server:local .
    if [ -d "$PWD/config/secrets" ]; then \
      docker run --rm \
        --network ferrex_default \
        -v "$PWD/config":/app/config \
        -v "$PWD/config/secrets":/run/ferrex-secrets:ro \
        -e FERREX_APP_PASSWORD_FILE=/run/ferrex-secrets/ferrex_app_password \
        -e DATABASE_PASSWORD_FILE=/run/ferrex-secrets/ferrex_app_password \
        -e POSTGRES_PASSWORD_FILE=/run/ferrex-secrets/postgres_superuser_password \
        -e DATABASE_HOST="${POSTGRES_INTERNAL_HOST:-db}" \
        -e DATABASE_PORT="${POSTGRES_INTERNAL_PORT:-5432}" \
        -e DATABASE_NAME="${FERREX_DB:-ferrex}" \
        -e DATABASE_USER="${FERREX_APP_USER:-ferrex_app}" \
        ferrex/server:local \
        db migrate {{ args }}; \
    else \
      docker run --rm \
        --network ferrex_default \
        -v "$PWD/config":/app/config \
        -e DATABASE_HOST="${POSTGRES_INTERNAL_HOST:-db}" \
        -e DATABASE_PORT="${POSTGRES_INTERNAL_PORT:-5432}" \
        -e DATABASE_NAME="${FERREX_DB:-ferrex}" \
        -e DATABASE_USER="${FERREX_APP_USER:-ferrex_app}" \
        ferrex/server:local \
        db migrate {{ args }}; \
    fi

#######################
# Development shortcuts
#######################

alias c := check
alias cq := check-quiet
alias ca := check
alias caq := check-quiet
alias rp := run-player
alias rpr := run-player-release
alias rs := run-server
alias rsr := run-server-release
alias gs := gstat

default:
    @just --list

[no-cd]
dev: check fmt lint
    @echo "✅ All checks passed!"

# # Check
[no-cd]
check args="":
    cargo check --workspace {{ args }}

[no-cd]
check-quiet:
    RUSTFLAGS=-Awarnings cargo check --workspace --quiet

[no-cd]
check_all args="":
    RUSTFLAGS=-Awarnings cargo check --workspace --all-features --all-targets {{ args }}

[no-cd]
check-all-quiet:
    cargo check --workspace --quiet

[no-cd]
check-player:
    cargo check -p ferrex-player

[no-cd]
check-server:
    cargo check -p ferrex-server

[no-cd]
check-core:
    cargo check -p ferrex-core

# Test
[no-cd]
test args="" pt_args="":
    RUSTFLAGS=-Awarnings cargo test --workspace --all-features --all-targets --no-fail-fast --quiet {{ args }} -- {{ pt_args }}

[no-cd]
test-player:
    RUSTFLAGS=-Awarnings cargo test -p ferrex-player --no-fail-fast

[no-cd]
test-server:
    RUSTFLAGS=-Awarnings cargo test -p ferrex-server --no-fail-fast

[no-cd]
test-core:
    RUSTFLAGS=-Awarnings cargo test -p ferrex-core --no-fail-fast

# Format
[no-cd]
fmt:
    cargo fmt --all

[no-cd]
fmt-player:
    cargo fmt -p ferrex-player

[no-cd]
fmt-server:
    cargo fmt -p ferrex-server

[no-cd]
fmt-core:
    cargo fmt -p ferrex-core

# Clippy
[no-cd]
lint:
    cargo clippy --workspace --all-targets --all-features

[no-cd]
lint-player:
    cargo clippy --all-targets --all-features -p ferrex-player

# Fix
[no-cd]
fix: fmt
    cargo fix --edition-idioms --all-targets --all-features --workspace --message-format short
    cargo clippy --fix --all-targets --all-features --workspace --message-format short

[no-cd]
fix-dirty: fmt
    cargo fix --edition-idioms --all-targets --all-features --workspace --message-format short --allow-dirty --allow-staged
    cargo clippy --fix --all-targets --all-features --workspace --message-format short --allow-dirty --allow-staged

# Run
[no-cd]
run-player PROFILE="release":
    cargo run -p ferrex-player --profile {{ PROFILE }}

[no-cd]
run-player-release:
    cargo run --release -p ferrex-player

[no-cd]
run-server:
    cargo run -p ferrex-server

[no-cd]
run-server-release:
    cargo run --release -p ferrex-server

# sqlx
[no-cd]
prepare $SQLX_OFFLINE="false":
    cargo sqlx prepare --workspace -- --all-features --all-targets

[confirm]
[no-cd]
migrate:
    cd ferrex-core && cargo sqlx migrate run

[no-cd]
reset:
    cd ferrex-core && cargo sqlx database reset

# Git
[no-cd]
gstat:
    git status

wtadd relative-path branch:
    git worktree add ./{{ relative-path }} -b {{ branch }}
    cp -r ./.cargo ./.env ./{{ relative-path }}

##########################
# Compilation benchmarking
##########################

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

INCR_FILE := "ferrex-core/src/lib.rs"
LEAF_FILE := "ferrex-player/src/main.rs"
RESULTS_DIR := "docs/bench_results"
BASE := "env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u RUSTC_WRAPPER \
    -u CARGO_PROFILE_RELEASE_INCREMENTAL"
OPT_BASE := "-u CARGO_PROFILE_RELEASE_OPT_LEVEL -u CARGO_PROFILE_DEBUG_OPT_LEVEL"
WILD := "RUSTFLAGS='-Clinker=clang -Clink-args=--ld-path=wild'"
INCR := "CARGO_PROFILE_RELEASE_INCREMENTAL=true"
CRANELIFT := "CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift"

[no-cd]
bench_linkers PROFILE="release":
    mkdir -p {{ RESULTS_DIR }}
    # Benchmarking linkers
    @hyperfine --prepare "cargo clean" --runs 3 --export-json {{ RESULTS_DIR }}/linkers_{{ PROFILE }}.json \
        --reference "{{ BASE }} cargo build --{{ PROFILE }}" \
        "{{ BASE }} {{ WILD }} cargo build --{{ PROFILE }}" \
        "{{ BASE }} mold -run cargo build --{{ PROFILE }}"

[no-cd]
bench_linkers_incr PROFILE="release" FILE=INCR_FILE JOBS="3":
    mkdir -p {{ RESULTS_DIR }}
    # Benchmarking linkers with incremental changes
    @hyperfine \
      --runs 3 --export-json {{ RESULTS_DIR }}/linkers_incr_{{ PROFILE }}.json \
      --prepare "{{ BASE }} {{ INCR }} cargo build --profile {{ PROFILE }} && touch {{ FILE }}" \
        --reference "{{ BASE }} {{ INCR }} cargo build --profile {{ PROFILE }} -j{{ JOBS }}" \
      --prepare "{{ BASE }} {{ INCR }} {{ WILD }} cargo build --profile {{ PROFILE }} && touch {{ FILE }}" \
        "{{ BASE }} {{ INCR }} {{ WILD }} cargo build --profile {{ PROFILE }} -j{{ JOBS }}" \
      --prepare "{{ BASE }} {{ INCR }} mold -run cargo build --profile {{ PROFILE }} && touch {{ FILE }}" \
        "{{ BASE }} {{ INCR }} mold -run cargo build --profile {{ PROFILE }} -j{{ JOBS }}"

[no-cd]
bench_opt PROFILE="release":
    mkdir -p {{ RESULTS_DIR }}
    # Benchmarking opt levels
    hyperfine --prepare "cargo clean" --warmup 1 \
      --runs 3 --export-json {{ RESULTS_DIR }}/opt_{{ PROFILE }}.json \
        --reference "cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=0 CARGO_PROFILE_RELEASE_OPT_LEVEL=0 \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=1 CARGO_PROFILE_RELEASE_OPT_LEVEL=1 \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=2 CARGO_PROFILE_RELEASE_OPT_LEVEL=2 \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_OPT_LEVEL=3 \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=s CARGO_PROFILE_RELEASE_OPT_LEVEL=s \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=z CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
          cargo build --profile {{ PROFILE }}"

[no-cd]
bench_opt_incr PROFILE="release" FILE=INCR_FILE:
    mkdir -p {{ RESULTS_DIR }}
    # Benchmarking opt levels with incremental changes
    @hyperfine --setup "cargo build --profile {{ PROFILE }}" --prepare "touch {{ FILE }}" \
      --warmup 1 --runs 3 --export-json {{ RESULTS_DIR }}/opt_incr_{{ PROFILE }}.json \
        --reference "cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=0 CARGO_PROFILE_RELEASE_OPT_LEVEL=0 \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=1 CARGO_PROFILE_RELEASE_OPT_LEVEL=1 \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=2 CARGO_PROFILE_RELEASE_OPT_LEVEL=2 \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_OPT_LEVEL=3 \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=s CARGO_PROFILE_RELEASE_OPT_LEVEL=s \
          cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ OPT_BASE }} CARGO_PROFILE_DEV_OPT_LEVEL=z CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
          cargo build --profile {{ PROFILE }}"

[no-cd]
bench_caching PROFILE="release":
    mkdir -p {{ RESULTS_DIR }}
    # Benchmarking with vs without sccaching
    @hyperfine --setup "{{ BASE }} cargo build --profile {{ PROFILE }}" --prepare "cargo clean" --warmup 1 \
      --runs 3 --export-json {{ RESULTS_DIR }}/caching_{{ PROFILE }}.json \
        "{{ BASE }} {{ WILD }} cargo build --profile {{ PROFILE }}" \
        "{{ BASE }} {{ WILD }} RUSTC_WRAPPER=sccache cargo build --profile {{ PROFILE }}"

[no-cd]
bench_caching_incr PROFILE="release" FILE=INCR_FILE:
    mkdir -p {{ RESULTS_DIR }}
    # Benchmarking with vs without sccaching after incremental changes
    @hyperfine --setup "{{ BASE }} {{ WILD }} cargo build --profile {{ PROFILE }}" --prepare "touch {{ FILE }}" --warmup 1 \
      --runs 3 --export-json {{ RESULTS_DIR }}/caching_incr_{{ PROFILE }}.json \
        "{{ BASE }} {{ WILD }} cargo build --profile {{ PROFILE }}" \ "{{ BASE }} {{ WILD }} RUSTC_WRAPPER=sccache cargo build --profile {{ PROFILE }}"

[no-cd]
bench_commands:
    mkdir -p {{ RESULTS_DIR }}
    hyperfine --prepare 'cargo clean' --runs 3 --export-json {{ RESULTS_DIR }}/commands_dev.json \
        "cargo check" \
        "cargo build" \
        "cargo test --no-run"
