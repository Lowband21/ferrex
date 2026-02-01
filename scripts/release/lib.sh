#!/usr/bin/env bash
set -euo pipefail

die() {
  echo "error: $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

git_repo_slug() {
  local url
  url="$(git remote get-url origin 2>/dev/null || true)"
  [[ -n $url ]] || die "no git remote named 'origin' found"

  # https://github.com/OWNER/REPO(.git)
  if [[ $url =~ ^https://github.com/([^/]+)/([^/]+)(\.git)?$ ]]; then
    echo "${BASH_REMATCH[1]}/${BASH_REMATCH[2]%.git}"
    return 0
  fi
  # git@github.com:OWNER/REPO(.git)
  if [[ $url =~ ^git@github.com:([^/]+)/([^/]+)(\.git)?$ ]]; then
    echo "${BASH_REMATCH[1]}/${BASH_REMATCH[2]%.git}"
    return 0
  fi

  die "unsupported origin url (expected github): $url"
}

workspace_version() {
  awk '
    $0 ~ /^\[workspace\.package\]/ {in_ws=1; next}
    in_ws && $0 ~ /^\[/ {in_ws=0}
    in_ws && $0 ~ /^version[[:space:]]*=/ {
      sub(/^[^"]*"/,""); sub(/".*$/,""); print; exit
    }
  ' Cargo.toml
}

ferrexctl_version() {
  awk '
    $0 ~ /^\[package\]/ {in_pkg=1; next}
    in_pkg && $0 ~ /^\[/ {in_pkg=0}
    in_pkg && $0 ~ /^version[[:space:]]*=/ {
      sub(/^[^"]*"/,""); sub(/".*$/,""); print; exit
    }
  ' ferrexctl/Cargo.toml
}

ensure_clean_tree() {
  if ! git diff --quiet || ! git diff --cached --quiet; then
    die "working tree not clean; commit or stash changes before releasing"
  fi
}

ensure_gh() {
  require_cmd gh
  gh auth status >/dev/null 2>&1 || die "gh not authenticated; run: gh auth login"
}

ensure_docker() {
  require_cmd docker
  docker info >/dev/null 2>&1 || die "docker daemon not available"
}

ghcr_login() {
  local user token
  user="$(gh api user -q .login)"
  token="$(gh auth token)"
  [[ -n $user && -n $token ]] || die "unable to get gh auth token/user"
  echo "$token" | docker login ghcr.io -u "$user" --password-stdin >/dev/null
}

make_dist_dir() {
  local tag="$1"
  local dir="dist-release/${tag}"
  mkdir -p "$dir"
  echo "$dir"
}

json_escape() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  s="${s//$'\n'/\\n}"
  printf '%s' "$s"
}

sha256_file() {
  sha256sum "$1" | awk '{print $1}'
}

file_size() {
  stat -c %s "$1"
}

write_manifest() {
  local out="$1"
  local tag="$2"
  local version="$3"
  local scope="$4" # workspace|init
  shift 4

  local commit created
  commit="$(git rev-parse HEAD)"
  created="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  # Remaining args are artifact file paths (absolute or relative).
  local artifacts_json=""
  local first=1
  for path in "$@"; do
    [[ -f $path ]] || die "manifest artifact missing: $path"
    local name sha size
    name="$(basename "$path")"
    sha="$(sha256_file "$path")"
    size="$(file_size "$path")"
    if [[ $first -eq 1 ]]; then first=0; else artifacts_json+=", "; fi
    artifacts_json+="{\"name\":\"$(json_escape "$name")\",\"sha256\":\"$sha\",\"size\":$size}"
  done

  mkdir -p "$(dirname "$out")"
  cat >"$out" <<EOF
{
  "schema": "ferrex.release-manifest.v1",
  "scope": "$(json_escape "$scope")",
  "tag": "$(json_escape "$tag")",
  "version": "$(json_escape "$version")",
  "commit": "$(json_escape "$commit")",
  "created_utc": "$(json_escape "$created")",
  "artifacts": [ $artifacts_json ]
}
EOF
}

write_sha256sums() {
  local dir="$1"
  (cd "$dir" && find . -maxdepth 1 -type f ! -name 'SHA256SUMS' -printf '%f\0' | sort -z | xargs -0 sha256sum >SHA256SUMS)
}
