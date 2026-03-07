# shellcheck shell=bash

# Detect a usable Python 3 interpreter across platforms. We persist the result
# in FERREX_PYTHON_BIN so subsequent callers can simply invoke ferrex_python.
# On failure, the caller should echo an actionable message and abort.

ferrex_python_reset() {
  unset FERREX_PYTHON_BIN
  FERREX_PYTHON_BIN_SET=0
}

ferrex_python_reset

_ferrex_python_try() {
  local -a cmd=("$@")
  command -v "${cmd[0]}" >/dev/null 2>&1 || return 1
  if "${cmd[@]}" - <<'PY' >/dev/null 2>&1; then
import sys
sys.exit(0 if sys.version_info >= (3, 8) else 1)
PY
    FERREX_PYTHON_BIN=("${cmd[@]}")
    FERREX_PYTHON_BIN_SET=1
    return 0
  fi
  return 1
}

ferrex_detect_python() {
  if [[ ${FERREX_PYTHON_BIN_SET:-0} -eq 1 ]]; then
    return 0
  fi

  if _ferrex_python_try python3; then return 0; fi
  if _ferrex_python_try python; then return 0; fi
  if _ferrex_python_try py -3; then return 0; fi
  if _ferrex_python_try py; then return 0; fi

  return 1
}

ferrex_python() {
  ferrex_require_python || return 1
  "${FERREX_PYTHON_BIN[@]}" "$@"
}

ferrex_require_python() {
  if ferrex_detect_python; then
    return 0
  fi

  cat >&2 <<'EOF'
Error: Python 3.8+ is required but was not found on PATH.

Install Python 3 and ensure one of the following commands is available:
  * python3
  * python
  * py -3 (Windows launcher)

On Windows, either enable WSL (recommended) or install the official Python
distribution and tick "Add python to PATH".
EOF
  return 1
}
