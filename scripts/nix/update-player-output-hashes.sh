#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

die() {
  echo "error: $*" >&2
  exit 1
}

command -v nix >/dev/null 2>&1 || die "missing nix"

lock="Cargo.lock"
[[ -f $lock ]] || die "missing $lock"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

tsv="$tmp/git-deps.tsv"

# Extract git dependencies as: <crateKey>\t<source>
awk '
  $1=="name"{gsub(/"/,"",$3); name=$3}
  $1=="version"{gsub(/"/,"",$3); ver=$3}
  $1=="source"{
    src=$0
    if (src ~ /"git\+/) {
      sub(/^source = "/,"",src); sub(/"$/,"",src)
      printf "%s-%s\t%s\n", name, ver, src
    }
  }
' "$lock" | sort -u >"$tsv"

if [[ ! -s $tsv ]]; then
  die "no git dependencies found in Cargo.lock"
fi

declare -A crate_to_keysource=()
declare -A keysource_to_hash=()

while IFS=$'\t' read -r crate_key src; do
  src="${src#git+}"
  url="${src%%#*}"
  rev="${src##*#}"
  url="${url%%\?*}"
  keysource="${url}#${rev}"
  crate_to_keysource["$crate_key"]="$keysource"
done <"$tsv"

# Prefetch each unique source once.
for keysource in "${crate_to_keysource[@]}"; do
  if [[ -n ${keysource_to_hash[$keysource]:-} ]]; then
    continue
  fi
  url="${keysource%%#*}"
  rev="${keysource##*#}"
  echo "prefetch: $url @ $rev" >&2
  json="$(nix run nixpkgs#nix-prefetch-git -- --url "$url" --rev "$rev" --quiet)"
  hash="$(echo "$json" | sed -n 's/.*"hash"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
  [[ -n $hash ]] || die "failed to parse hash for $url @ $rev"
  keysource_to_hash["$keysource"]="$hash"
done

# Emit nix attrset lines, sorted by crate key.
for k in "${!crate_to_keysource[@]}"; do
  echo "$k"
done | sort | while read -r crate_key; do
  keysource="${crate_to_keysource[$crate_key]}"
  hash="${keysource_to_hash[$keysource]}"
  printf '                "%s" = "%s";\n' "$crate_key" "$hash"
done
