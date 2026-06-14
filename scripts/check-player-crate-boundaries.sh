#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

non_ui_crates=(
  ferrex-player-foundation
  ferrex-player-api
  ferrex-player-auth
  ferrex-player-repository
  ferrex-player-library
  ferrex-player-media
  ferrex-player-metadata
  ferrex-player-search
  ferrex-player-user-admin
)

runtime_dependency_pattern='^[[:space:]]*(iced|iced_[[:alnum:]_-]*|iced_aw|lucide-icons|subwave[[:alnum:]_-]*)[[:space:]]*='
runtime_source_pattern='\b(iced|iced_[[:alnum:]_]*|iced_aw|subwave[[:alnum:]_]*)::|\buse[[:space:]]+(iced|iced_[[:alnum:]_]*|iced_aw|subwave[[:alnum:]_]*)\b|\bextern[[:space:]]+crate[[:space:]]+(iced|iced_[[:alnum:]_]*|iced_aw|subwave[[:alnum:]_]*)\b'
app_layer_dependency_pattern='^[[:space:]]*(ferrex-player|ferrex-player-app|ferrex-player-ui)[[:space:]]*='
app_layer_source_pattern='\b(ferrex_player|ferrex_player_app|ferrex_player_ui)::|\buse[[:space:]]+(ferrex_player|ferrex_player_app|ferrex_player_ui)\b|\bextern[[:space:]]+crate[[:space:]]+(ferrex_player|ferrex_player_app|ferrex_player_ui)\b'
foundation_dependency_pattern='^[[:space:]]*(ferrex-player|ferrex-player-app|ferrex-player-ui|ferrex-core|ferrex-model|ferrex-contracts|iced|iced_[[:alnum:]_-]*|iced_aw|lucide-icons|subwave[[:alnum:]_-]*)[[:space:]]*='
foundation_source_pattern='\b(ferrex_player|ferrex_player_app|ferrex_player_ui|ferrex_core|ferrex_model|ferrex_contracts|iced|iced_[[:alnum:]_]*|iced_aw|subwave[[:alnum:]_]*)::|\buse[[:space:]]+(ferrex_player|ferrex_player_app|ferrex_player_ui|ferrex_core|ferrex_model|ferrex_contracts|iced|iced_[[:alnum:]_]*|iced_aw|subwave[[:alnum:]_]*)\b|\bextern[[:space:]]+crate[[:space:]]+(ferrex_player|ferrex_player_app|ferrex_player_ui|ferrex_core|ferrex_model|ferrex_contracts|iced|iced_[[:alnum:]_]*|iced_aw|subwave[[:alnum:]_]*)\b'
tree_runtime_pattern='^[[:space:]]*(iced|iced_[[:alnum:]_-]*|iced_aw|lucide-icons|subwave[[:alnum:]_-]*)[[:space:]]+v'

check_manifest() {
  local manifest="$1"
  local pattern="$2"
  local message="$3"

  if grep -nE "$pattern" "$manifest"; then
    echo "$message" >&2
    exit 1
  fi
}

check_sources() {
  local src_dir="$1"
  local pattern="$2"
  local message="$3"

  if grep -R -nE "$pattern" "$src_dir"; then
    echo "$message" >&2
    exit 1
  fi
}

check_manifest_with_allowlist() {
  local manifest="$1"
  local pattern="$2"
  local allow_pattern="$3"
  local message="$4"
  local matches

  matches="$(grep -nE "$pattern" "$manifest" | grep -vE "$allow_pattern" || true)"
  if [[ -n "$matches" ]]; then
    printf '%s\n' "$matches"
    echo "$message" >&2
    exit 1
  fi
}

check_sources_with_allowlist() {
  local src_dir="$1"
  local pattern="$2"
  local allow_pattern="$3"
  local message="$4"
  local matches

  matches="$(grep -R -nE "$pattern" "$src_dir" | grep -vE "$allow_pattern" || true)"
  if [[ -n "$matches" ]]; then
    printf '%s\n' "$matches"
    echo "$message" >&2
    exit 1
  fi
}

for crate in "${non_ui_crates[@]}"; do
  manifest="$repo_root/$crate/Cargo.toml"
  src_dir="$repo_root/$crate/src"

  check_manifest \
    "$manifest" \
    "$runtime_dependency_pattern" \
    "$crate has a UI/video runtime dependency; keep Iced/subwave in ferrex-player-ui or ferrex-player-app"
  check_manifest \
    "$manifest" \
    "$app_layer_dependency_pattern" \
    "$crate depends on the player facade/app/UI layer; lower crates must not depend upward"
  check_sources \
    "$src_dir" \
    "$runtime_source_pattern" \
    "$crate imports a UI/video runtime crate; keep Iced/subwave code in UI/app crates"
  check_sources \
    "$src_dir" \
    "$app_layer_source_pattern" \
    "$crate imports the player facade/app/UI layer; lower crates must not depend upward"
done

settings_manifest="$repo_root/ferrex-player-settings/Cargo.toml"
settings_src_dir="$repo_root/ferrex-player-settings/src"
settings_iced_core_manifest_allow='^[[:digit:]]+:[[:space:]]*iced_core[[:space:]]*='
settings_iced_core_source_allow='^[^:]+:[[:digit:]]+:[[:space:]]*((pub[[:space:]]+)?use[[:space:]]+iced_core\b|extern[[:space:]]+crate[[:space:]]+iced_core\b)|^[^:]+:[[:digit:]]+:.*\biced_core::'

check_manifest_with_allowlist \
  "$settings_manifest" \
  "$runtime_dependency_pattern" \
  "$settings_iced_core_manifest_allow" \
  "ferrex-player-settings has a UI/video runtime dependency beyond the allowed iced_core color DTO boundary"
check_manifest \
  "$settings_manifest" \
  "$app_layer_dependency_pattern" \
  "ferrex-player-settings depends on the player facade/app/UI layer; lower crates must not depend upward"
check_sources_with_allowlist \
  "$settings_src_dir" \
  "$runtime_source_pattern" \
  "$settings_iced_core_source_allow" \
  "ferrex-player-settings imports a UI/video runtime crate beyond the allowed iced_core color DTO boundary"
check_sources \
  "$settings_src_dir" \
  "$app_layer_source_pattern" \
  "ferrex-player-settings imports the player facade/app/UI layer; lower crates must not depend upward"

check_manifest \
  "$repo_root/ferrex-player-foundation/Cargo.toml" \
  "$foundation_dependency_pattern" \
  "ferrex-player-foundation has a banned direct dependency"
check_sources \
  "$repo_root/ferrex-player-foundation/src" \
  "$foundation_source_pattern" \
  "ferrex-player-foundation imports a banned crate/layer"

for crate in "${non_ui_crates[@]}"; do
  tree="$(cd "$repo_root" && cargo tree -p "$crate" --edges normal --prefix none)"
  if printf '%s\n' "$tree" | grep -E "$tree_runtime_pattern"; then
    echo "$crate pulls in Iced/subwave transitively through normal dependencies" >&2
    exit 1
  fi
done

settings_tree="$(cd "$repo_root" && cargo tree -p ferrex-player-settings --edges normal --prefix none)"
if printf '%s\n' "$settings_tree" \
  | grep -E "$tree_runtime_pattern" \
  | grep -vE '^[[:space:]]*iced_core[[:space:]]+v'; then
  echo "ferrex-player-settings pulls in Iced/subwave transitively beyond the allowed iced_core color DTO boundary" >&2
  exit 1
fi

echo "player crate dependency boundary check passed"
