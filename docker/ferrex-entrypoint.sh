#!/usr/bin/env bash
set -euo pipefail

SERVER_BINARY=${SERVER_BINARY:-/usr/local/bin/ferrex-server}
SERVER_USER=${SERVER_USER:-ferrex}
SERVER_GROUP=${SERVER_GROUP:-ferrex}
CACHE_ROOT=${CACHE_ROOT:-/app/cache}

# Ensure the cache root and expected subdirectories exist so we can fix ownership.
for dir in \
    "${CACHE_ROOT}" \
    "${CACHE_ROOT}/images" \
    "${CACHE_ROOT}/transcode" \
    "${CACHE_ROOT}/thumbnails"
do
    if [ ! -d "${dir}" ]; then
        mkdir -p "${dir}"
    fi
done

# Reset ownership each start in case the cache volume is newly mounted or owned by root.
if chown -R "${SERVER_USER}:${SERVER_GROUP}" "${CACHE_ROOT}"; then
    exec gosu "${SERVER_USER}:${SERVER_GROUP}" "${SERVER_BINARY}" "$@"
fi

>&2 echo "warning: failed to set cache ownership; starting ${SERVER_BINARY} as root"
exec "${SERVER_BINARY}" "$@"
