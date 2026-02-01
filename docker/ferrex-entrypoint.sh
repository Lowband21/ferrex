#!/usr/bin/env bash
set -euo pipefail

SERVER_BINARY=${SERVER_BINARY:-/usr/local/bin/ferrex-server}
SERVER_USER=${SERVER_USER:-ferrex}
SERVER_GROUP=${SERVER_GROUP:-ferrex}
CACHE_ROOT=${CACHE_ROOT:-/app/cache}
PUID=${PUID:-}
PGID=${PGID:-}
UMASK=${UMASK:-}

warn() {
  >&2 echo "warning: $*"
}

# Ensure the cache root and expected subdirectories exist so we can fix ownership.
for dir in \
  "${CACHE_ROOT}" \
  "${CACHE_ROOT}/images" \
  "${CACHE_ROOT}/transcode" \
  "${CACHE_ROOT}/thumbnails"; do
  if [ ! -d "${dir}" ]; then
    mkdir -p "${dir}"
  fi
done

# Optional umask (common in Unraid templates)
if [ -n "${UMASK}" ]; then
  umask "${UMASK}" || warn "invalid UMASK '${UMASK}' (expected e.g. 0022)"
fi

gosu_target="${SERVER_USER}:${SERVER_GROUP}"
if [ -n "${PUID}" ] || [ -n "${PGID}" ]; then
  if [ -z "${PUID}" ] || [ -z "${PGID}" ]; then
    warn "PUID/PGID override requires both PUID and PGID; ignoring override"
  else
    gosu_target="${PUID}:${PGID}"
  fi
fi

# If running as root inside the container, chown the cache and drop privileges to
# SERVER_USER:SERVER_GROUP. On rootless runs (e.g., podman with --user), we skip
# gosu and execute directly to avoid EPERM when switching users.
if [ "$(id -u)" = "0" ]; then
  if chown -R "${gosu_target}" "${CACHE_ROOT}"; then
    exec gosu "${gosu_target}" "${SERVER_BINARY}" "$@"
  fi
  >&2 echo "warning: failed to set cache ownership; starting ${SERVER_BINARY} as root"
  exec "${SERVER_BINARY}" "$@"
else
  echo "info: running as UID $(id -u):$(id -g); skipping gosu"
  exec "${SERVER_BINARY}" "$@"
fi
