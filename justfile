# This file can be detected and used directly from subfolders
# Useful for keeping in sync with multiple projects/worktrees
# Uses the .env from it's containing folder

set dotenv-path := "."
set dotenv-override := true

########################################
# Docker Compose shortcuts (stack + cfg)
########################################

[no-cd]
up profile="release":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    docker rm -f ferrex_media_db_simple ferrex_media_cache_simple ferrex_media_server >/dev/null 2>&1 || true
    FERREX_BUILD_PROFILE={{ profile }} COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env up -d --build

[no-cd]
up-tailscale profile="release":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    docker rm -f ferrex_media_db_simple ferrex_media_cache_simple ferrex_media_server ferrex_tailscale >/dev/null 2>&1 || true
    FERREX_BUILD_PROFILE={{ profile }} COMPOSE_PROJECT_NAME=ferrex docker compose -f docker-compose.yml -f docker-compose.tailscale.yml --env-file config/.env up -d --build

[no-cd]
down:
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env down
    docker rm -f ferrex_media_db_simple ferrex_media_cache_simple ferrex_media_server >/dev/null 2>&1 || true

[no-cd]
down-tailscale:
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    COMPOSE_PROJECT_NAME=ferrex docker compose -f docker-compose.yml -f docker-compose.tailscale.yml --env-file config/.env down
    docker rm -f ferrex_media_db_simple ferrex_media_cache_simple ferrex_media_server ferrex_tailscale >/dev/null 2>&1 || true

[no-cd]
logs service="ferrex":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env logs -f {{ service }}

# Override the default Serve mapping in docker/tailscale/serve-config.json.
[no-cd]
tailscale-serve target="http://127.0.0.1:3000":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    COMPOSE_PROJECT_NAME=ferrex docker compose -f docker-compose.yml -f docker-compose.tailscale.yml --env-file config/.env exec tailscale tailscale serve https / {{ target }}

# Interactive config init inside the ferrex container.

# Writes config/ferrex.toml and config/.env on the host via volume mounts.
[no-cd]
init-config args="" FERREX_INIT_SKIP_BUILD="0":
    utils/init-config.sh {{ args }}

[no-cd]
config args="" FERREX_INIT_SKIP_BUILD="1":
    utils/init-config.sh {{ args }}

[no-cd]
rebuild-server profile="release":
    docker build -f docker/Dockerfile.prod --build-arg BUILD_PROFILE={{ profile }} -t ferrex/server:local .

# Validate configuration (will also sanity-check DB/Redis connectivity if present)
[no-cd]
check-config profile="release" args="":
    if [ ! -f config/.env ]; then echo "Config missing: config/.env. Run: just init-config"; exit 1; fi
    docker rm -f ferrex_media_db_simple ferrex_media_cache_simple ferrex_media_server >/dev/null 2>&1 || true
    FERREX_BUILD_PROFILE={{ profile }} COMPOSE_PROJECT_NAME=ferrex docker compose --env-file config/.env up -d db cache
    if [ "{{ profile }}" = "release" ]; then \
        docker image inspect ferrex/server:local >/dev/null 2>&1 || docker build -f docker/Dockerfile.prod --build-arg BUILD_PROFILE={{ profile }} -t ferrex/server:local .; \
    else \
        docker build -f docker/Dockerfile.prod --build-arg BUILD_PROFILE={{ profile }} -t ferrex/server:local .; \
    fi
    docker run --rm \
      --network ferrex_default \
      -v "$PWD/config":/app/config \
      ferrex/server:local \
      config check --config-path /app/config/ferrex.toml --env-file /app/config/.env {{ args }}

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
    cargo clippy --workspace --all-targets --all-features --workspace

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
run-player:
    cargo run -p ferrex-player

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
