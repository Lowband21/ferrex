#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
ANDROID_DIR="$REPO_ROOT/mobile/android"
REPORT_DIR="${FERREX_ANDROID_QA_REPORT_DIR:-$REPO_ROOT/target/android-qa}"
EMULATOR_STATE_HOME="${FERREX_ANDROID_STATE_HOME:-$REPO_ROOT/target/android-qa/emulator-state}"
BOOT_TIMEOUT_SECONDS="${FERREX_ANDROID_BOOT_TIMEOUT_SECONDS:-360}"

EMULATOR_TOOL="${FERREX_ANDROID_EMULATORS_BIN:-ferrex-android-emulators}"
SDK_ROOT=""
AAPT2=""
ADB=""

PHONE_SERIAL="${FERREX_ANDROID_PHONE_SERIAL:-emulator-5554}"
PHONE_API="${FERREX_ANDROID_PHONE_API:-35}"
PHONE_MODEL="${FERREX_ANDROID_PHONE_MODEL:-sdk_gphone64_x86_64}"
PHONE_DEVICE="${FERREX_ANDROID_PHONE_DEVICE:-emu64xa}"
PHONE_PACKAGE="com.ferrex.android.debug"
PHONE_CATEGORY="android.intent.category.LAUNCHER"
PHONE_APK="$ANDROID_DIR/app/build/outputs/apk/mobile/debug/app-mobile-debug.apk"
PHONE_FLAVOR="mobile"

TV_SERIAL="${FERREX_ANDROID_TV_SERIAL:-emulator-5556}"
TV_API="${FERREX_ANDROID_TV_API:-34}"
TV_MODEL="${FERREX_ANDROID_TV_MODEL:-AOSP TV on x86}"
TV_DEVICE="${FERREX_ANDROID_TV_DEVICE:-emulator_x86_arm}"
TV_PACKAGE="com.ferrex.android.tv.debug"
TV_CATEGORY="android.intent.category.LEANBACK_LAUNCHER"
TV_APK="$ANDROID_DIR/app/build/outputs/apk/tv/debug/app-tv-debug.apk"
TV_FLAVOR="tv"

usage() {
  cat <<'EOF'
Usage: scripts/qa/android-emulator-qa.sh <command> [target]

Commands:
  doctor          Verify host tooling, KVM, SDK/aapt2, AVD creation, and both required serials
  build           Build mobile and TV debug APKs with the resolved Nix Android SDK/aapt2
  start           Ensure and start both Lowband emulators, then wait for emulator-5554 and emulator-5556
  install [target] Install debug APKs to their matching emulator serials and verify installed packages
  launch [target] Launch installed debug apps with the correct phone/TV launcher category
  check [target]  Record and validate target/device/package metadata without installing or launching

Targets for install/launch/check: all (default), phone/mobile, tv
EOF
}

info() {
  printf 'android-qa: %s\n' "$*" >&2
}

die() {
  printf 'android-qa: ERROR: %s\n' "$*" >&2
  exit 1
}

require_file_executable() {
  local path="$1"
  local label="$2"
  [ -x "$path" ] || die "$label is not executable at $path"
}

is_home_android_sdk() {
  local candidate="$1"
  [ -n "${HOME:-}" ] || return 1
  case "$candidate" in
    "$HOME/Android/Sdk"|"$HOME/Android/Sdk"/*|"$HOME/android-sdk"|"$HOME/android-sdk"/*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

resolve_aapt2() {
  local sdk_root="$1"
  local explicit="${FERREX_ANDROID_AAPT2:-${ANDROID_AAPT2:-}}"
  if [ -n "$explicit" ]; then
    [ -x "$explicit" ] || die "explicit aapt2 is not executable at $explicit"
    AAPT2="$explicit"
    return 0
  fi

  [ -d "$sdk_root/build-tools" ] || die "Android SDK build-tools directory is missing under $sdk_root"
  AAPT2="$(find -L "$sdk_root/build-tools" -mindepth 2 -maxdepth 2 -type f -name aapt2 -executable 2>/dev/null | sort -V | tail -n 1)"
  [ -n "$AAPT2" ] || die "could not resolve aapt2 under $sdk_root/build-tools"
}

validate_sdk_root() {
  local sdk_root="$1"
  [ -n "$sdk_root" ] || return 1
  is_home_android_sdk "$sdk_root" && die "refusing to use home Android SDK at $sdk_root; set FERREX_ANDROID_SDK_ROOT to the Nix SDK"
  [ -d "$sdk_root" ] || die "resolved Android SDK root does not exist: $sdk_root"
  [ -x "$sdk_root/platform-tools/adb" ] || die "resolved Android SDK is missing platform-tools/adb: $sdk_root"
  [ -x "$sdk_root/emulator/emulator" ] || die "resolved Android SDK is missing emulator/emulator: $sdk_root"
  resolve_aapt2 "$sdk_root"
  SDK_ROOT="$sdk_root"
}

resolve_sdk_from_env() {
  local candidate
  for candidate in "${FERREX_ANDROID_SDK_ROOT:-}" "${ANDROID_SDK_ROOT:-}" "${ANDROID_HOME:-}"; do
    if [ -n "$candidate" ]; then
      validate_sdk_root "$candidate"
      return 0
    fi
  done
  return 1
}

resolve_sdk_from_emulator_tool() {
  command -v "$EMULATOR_TOOL" >/dev/null 2>&1 || return 1
  local status sdk_root
  status="$(FERREX_ANDROID_STATE_HOME="$EMULATOR_STATE_HOME" "$EMULATOR_TOOL" status 2>/dev/null || true)"
  sdk_root="$(printf '%s\n' "$status" | sed -n 's/^ANDROID_HOME=//p' | head -n 1)"
  [ -n "$sdk_root" ] || return 1
  validate_sdk_root "$sdk_root"
}

resolve_sdk_from_adb_link() {
  command -v adb >/dev/null 2>&1 || return 1
  local adb_path link_target candidate
  adb_path="$(command -v adb)"

  link_target="$(readlink "$adb_path" 2>/dev/null || true)"
  if [ -n "$link_target" ]; then
    case "$link_target" in
      */bin/adb)
        candidate="${link_target%/bin/adb}/libexec/android-sdk"
        if [ -d "$candidate" ]; then
          validate_sdk_root "$candidate"
          return 0
        fi
        ;;
    esac
  fi

  link_target="$(readlink -f "$adb_path" 2>/dev/null || true)"
  case "$link_target" in
    */platform-tools/adb)
      candidate="${link_target%/platform-tools/adb}"
      if [ -d "$candidate/build-tools" ]; then
        validate_sdk_root "$candidate"
        return 0
      fi
      ;;
  esac

  return 1
}

resolve_android_env() {
  if [ -n "$SDK_ROOT" ]; then
    return 0
  fi

  mkdir -p "$REPORT_DIR" "$EMULATOR_STATE_HOME"
  if resolve_sdk_from_env; then
    :
  elif resolve_sdk_from_emulator_tool; then
    :
  elif resolve_sdk_from_adb_link; then
    :
  else
    die "could not resolve the Nix Android SDK; set FERREX_ANDROID_SDK_ROOT or install ferrex-android-emulators"
  fi

  ADB="$SDK_ROOT/platform-tools/adb"
  require_file_executable "$ADB" "adb"
  require_file_executable "$AAPT2" "aapt2"

  export FERREX_ANDROID_STATE_HOME="$EMULATOR_STATE_HOME"
  export FERREX_ANDROID_SDK_ROOT="$SDK_ROOT"
  export ANDROID_HOME="$SDK_ROOT"
  export ANDROID_SDK_ROOT="$SDK_ROOT"

  info "ANDROID_HOME=$ANDROID_HOME"
  info "android.aapt2FromMavenOverride=$AAPT2"
}

require_emulator_tool() {
  command -v "$EMULATOR_TOOL" >/dev/null 2>&1 || die "ferrex-android-emulators is missing; install/enable the Lowband Android emulator host tooling"
}

require_kvm() {
  [ -e /dev/kvm ] || die "/dev/kvm is missing; Android emulator acceleration is unavailable"
  [ -r /dev/kvm ] && [ -w /dev/kvm ] || die "/dev/kvm is not readable and writable for user $(id -un)"
}

normalize_target() {
  case "${1:-all}" in
    all|both|"") printf '%s\n' all ;;
    phone|mobile) printf '%s\n' phone ;;
    tv|television) printf '%s\n' tv ;;
    *) die "unknown target '$1'" ;;
  esac
}

targets_for() {
  case "$(normalize_target "${1:-all}")" in
    all) printf '%s\n' phone tv ;;
    phone) printf '%s\n' phone ;;
    tv) printf '%s\n' tv ;;
  esac
}

serial_for_target() {
  case "$1" in
    phone) printf '%s\n' "$PHONE_SERIAL" ;;
    tv) printf '%s\n' "$TV_SERIAL" ;;
    *) die "unknown target '$1'" ;;
  esac
}

api_for_target() {
  case "$1" in
    phone) printf '%s\n' "$PHONE_API" ;;
    tv) printf '%s\n' "$TV_API" ;;
    *) die "unknown target '$1'" ;;
  esac
}

model_for_target() {
  case "$1" in
    phone) printf '%s\n' "$PHONE_MODEL" ;;
    tv) printf '%s\n' "$TV_MODEL" ;;
    *) die "unknown target '$1'" ;;
  esac
}

device_for_target() {
  case "$1" in
    phone) printf '%s\n' "$PHONE_DEVICE" ;;
    tv) printf '%s\n' "$TV_DEVICE" ;;
    *) die "unknown target '$1'" ;;
  esac
}

package_for_target() {
  case "$1" in
    phone) printf '%s\n' "$PHONE_PACKAGE" ;;
    tv) printf '%s\n' "$TV_PACKAGE" ;;
    *) die "unknown target '$1'" ;;
  esac
}

category_for_target() {
  case "$1" in
    phone) printf '%s\n' "$PHONE_CATEGORY" ;;
    tv) printf '%s\n' "$TV_CATEGORY" ;;
    *) die "unknown target '$1'" ;;
  esac
}

apk_for_target() {
  case "$1" in
    phone) printf '%s\n' "$PHONE_APK" ;;
    tv) printf '%s\n' "$TV_APK" ;;
    *) die "unknown target '$1'" ;;
  esac
}

flavor_for_target() {
  case "$1" in
    phone) printf '%s\n' "$PHONE_FLAVOR" ;;
    tv) printf '%s\n' "$TV_FLAVOR" ;;
    *) die "unknown target '$1'" ;;
  esac
}

strip_cr() {
  tr -d '\r'
}

adb_shell() {
  local serial="$1"
  shift
  "$ADB" -s "$serial" shell "$@" | strip_cr
}

require_serial_present() {
  local serial="$1"
  local state
  state="$($ADB devices | awk -v serial="$serial" '$1 == serial { print $2; found = 1 } END { if (!found) print "" }')"
  [ "$state" = "device" ] || die "required Android serial $serial is absent or not ready (adb state: ${state:-missing})"
}

wait_for_serial() {
  local serial="$1"
  local boot=""
  "$ADB" -s "$serial" wait-for-device
  for _attempt in $(seq 1 "$BOOT_TIMEOUT_SECONDS"); do
    boot="$(adb_shell "$serial" getprop sys.boot_completed 2>/dev/null || true)"
    if [ "$boot" = "1" ]; then
      require_serial_present "$serial"
      return 0
    fi
    sleep 2
  done
  die "serial $serial did not finish booting within $BOOT_TIMEOUT_SECONDS polling attempts"
}

wm_size_for_serial() {
  local serial="$1"
  adb_shell "$serial" wm size | sed -n 's/^Physical size:[[:space:]]*//p' | head -n 1
}

leanback_for_serial() {
  local serial="$1"
  if adb_shell "$serial" pm list features | grep -qx 'feature:android.software.leanback'; then
    printf '%s\n' true
  else
    printf '%s\n' false
  fi
}

record_target_check() {
  local target="$1"
  local serial="$2"
  local api_level="$3"
  local model="$4"
  local device="$5"
  local wm_size="$6"
  local leanback="$7"
  local package_name="${8:-}"
  local package_path="${9:-}"
  local version_code="${10:-}"
  local version_name="${11:-}"
  local flavor="${12:-}"
  local output="$REPORT_DIR/$target-target-check.env"

  mkdir -p "$REPORT_DIR"
  {
    printf 'target=%s\n' "$target"
    printf 'serial=%s\n' "$serial"
    printf 'api_level=%s\n' "$api_level"
    printf 'model=%s\n' "$model"
    printf 'device=%s\n' "$device"
    printf 'wm_size=%s\n' "$wm_size"
    printf 'leanback=%s\n' "$leanback"
    printf 'package_name=%s\n' "$package_name"
    printf 'package_path=%s\n' "$package_path"
    printf 'version_code=%s\n' "$version_code"
    printf 'version_name=%s\n' "$version_name"
    printf 'flavor=%s\n' "$flavor"
  } > "$output"

  info "recorded $target target check at $output"
  sed 's/^/android-qa:   /' "$output" >&2
}

check_device_target() {
  local target="$1"
  local should_record="${2:-true}"
  local serial expected_serial expected_api expected_model expected_device api_level model device wm_size leanback
  serial="$(serial_for_target "$target")"
  expected_serial="$serial"
  expected_api="$(api_for_target "$target")"
  expected_model="$(model_for_target "$target")"
  expected_device="$(device_for_target "$target")"

  require_serial_present "$serial"
  api_level="$(adb_shell "$serial" getprop ro.build.version.sdk)"
  model="$(adb_shell "$serial" getprop ro.product.model)"
  device="$(adb_shell "$serial" getprop ro.product.device)"
  wm_size="$(wm_size_for_serial "$serial")"
  leanback="$(leanback_for_serial "$serial")"

  [ "$serial" = "$expected_serial" ] || die "$target serial mismatch: expected $expected_serial, got $serial"
  [ "$api_level" = "$expected_api" ] || die "$target API mismatch on $serial: expected $expected_api, got ${api_level:-empty}"
  [ -n "$model" ] || die "$target model property is empty on $serial"
  [ -n "$device" ] || die "$target device property is empty on $serial"
  [ "$model" = "$expected_model" ] || die "$target model mismatch on $serial: expected '$expected_model', got '$model'"
  [ "$device" = "$expected_device" ] || die "$target device mismatch on $serial: expected '$expected_device', got '$device'"
  [ -n "$wm_size" ] || die "$target wm size is empty on $serial"
  case "$target:$leanback" in
    phone:false|tv:true) ;;
    phone:true) die "phone target $serial reports android.software.leanback" ;;
    tv:false) die "tv target $serial is missing android.software.leanback" ;;
  esac

  if [ "$should_record" = "true" ]; then
    record_target_check "$target" "$serial" "$api_level" "$model" "$device" "$wm_size" "$leanback"
  fi
}

apk_badging_line() {
  local apk="$1"
  "$AAPT2" dump badging "$apk" | sed -n "s/^package: //p" | head -n 1
}

apk_package_name() {
  local apk="$1"
  apk_badging_line "$apk" | sed -n "s/^name='\([^']*\)'.*/\1/p"
}

require_apk_matches_target() {
  local target="$1"
  local apk package expected_package flavor
  apk="$(apk_for_target "$target")"
  expected_package="$(package_for_target "$target")"
  flavor="$(flavor_for_target "$target")"

  [ -f "$apk" ] || die "$target APK is missing at $apk; run the build primitive first"
  package="$(apk_package_name "$apk")"
  [ "$package" = "$expected_package" ] || die "$target APK package mismatch for $apk: expected $expected_package, got ${package:-empty}"
  case "$(basename "$apk")" in
    *"$flavor"*) ;;
    *) die "$target APK path does not include expected flavor '$flavor': $apk" ;;
  esac
}

check_installed_target() {
  local target="$1"
  local serial package expected_flavor api_level model device wm_size leanback package_path dump version_code version_name flavor
  serial="$(serial_for_target "$target")"
  package="$(package_for_target "$target")"
  expected_flavor="$(flavor_for_target "$target")"

  check_device_target "$target" false

  package_path="$(adb_shell "$serial" pm path "$package" | sed -n 's/^package://p' | head -n 1)"
  [ -n "$package_path" ] || die "$target package $package is not installed on $serial"

  dump="$(adb_shell "$serial" dumpsys package "$package")"
  version_code="$(printf '%s\n' "$dump" | sed -n 's/^[[:space:]]*versionCode=\([^[:space:]]*\).*/\1/p' | head -n 1)"
  version_name="$(printf '%s\n' "$dump" | sed -n 's/^[[:space:]]*versionName=//p' | head -n 1)"
  [ -n "$version_code" ] || die "$target package $package did not report versionCode on $serial"
  [ -n "$version_name" ] || die "$target package $package did not report versionName on $serial"

  flavor="$expected_flavor"
  case "$target:$version_name" in
    phone:*-tv) die "phone package $package has TV versionName '$version_name' on $serial" ;;
    tv:*-tv) ;;
    tv:*) die "tv package $package does not have TV versionName suffix on $serial: '$version_name'" ;;
  esac

  api_level="$(adb_shell "$serial" getprop ro.build.version.sdk)"
  model="$(adb_shell "$serial" getprop ro.product.model)"
  device="$(adb_shell "$serial" getprop ro.product.device)"
  wm_size="$(wm_size_for_serial "$serial")"
  leanback="$(leanback_for_serial "$serial")"
  record_target_check "$target" "$serial" "$api_level" "$model" "$device" "$wm_size" "$leanback" \
    "$package" "$package_path" "$version_code" "$version_name" "$flavor"
}

cmd_doctor() {
  require_emulator_tool
  require_kvm
  resolve_android_env
  "$EMULATOR_TOOL" ensure all || die "AVD creation/repair failed via ferrex-android-emulators ensure all"
  require_serial_present "$PHONE_SERIAL"
  require_serial_present "$TV_SERIAL"
  check_device_target phone
  check_device_target tv
}

cmd_build() {
  resolve_android_env
  ( cd "$ANDROID_DIR" && \
    ANDROID_HOME="$SDK_ROOT" \
    ANDROID_SDK_ROOT="$SDK_ROOT" \
    ./gradlew :app:assembleMobileDebug :app:assembleTvDebug --no-daemon --stacktrace \
      -Pandroid.aapt2FromMavenOverride="$AAPT2" )
}

cmd_start() {
  require_emulator_tool
  require_kvm
  resolve_android_env
  "$EMULATOR_TOOL" ensure all || die "AVD creation/repair failed via ferrex-android-emulators ensure all"
  "$EMULATOR_TOOL" start all || die "failed to start Lowband Android emulators via ferrex-android-emulators start all"
  wait_for_serial "$PHONE_SERIAL"
  wait_for_serial "$TV_SERIAL"
  check_device_target phone
  check_device_target tv
}

cmd_install() {
  local target
  resolve_android_env
  for target in $(targets_for "${1:-all}"); do
    require_apk_matches_target "$target"
    wait_for_serial "$(serial_for_target "$target")"
    check_device_target "$target" false
    "$ADB" -s "$(serial_for_target "$target")" install -r -d "$(apk_for_target "$target")"
    check_installed_target "$target"
  done
}

resolve_launch_component() {
  local serial="$1"
  local package="$2"
  local category="$3"
  local component
  component="$(adb_shell "$serial" cmd package resolve-activity --brief \
    -a android.intent.action.MAIN \
    -c "$category" \
    "$package" | tail -n 1)"
  case "$component" in
    "$package"/*) printf '%s\n' "$component" ;;
    *) die "could not resolve $package launcher activity for category $category on $serial (got '${component:-empty}')" ;;
  esac
}

cmd_launch() {
  local target serial package category component
  resolve_android_env
  for target in $(targets_for "${1:-all}"); do
    serial="$(serial_for_target "$target")"
    package="$(package_for_target "$target")"
    category="$(category_for_target "$target")"
    wait_for_serial "$serial"
    check_installed_target "$target"
    component="$(resolve_launch_component "$serial" "$package" "$category")"
    info "launching $package on $serial with category $category via $component"
    "$ADB" -s "$serial" shell am start -W \
      -a android.intent.action.MAIN \
      -c "$category" \
      -n "$component" >/dev/null
  done
}

cmd_check() {
  local target
  resolve_android_env
  for target in $(targets_for "${1:-all}"); do
    check_installed_target "$target"
  done
}

main() {
  local command_name="${1:-help}"
  if [ "$#" -gt 0 ]; then
    shift
  fi

  case "$command_name" in
    doctor) cmd_doctor "$@" ;;
    build) cmd_build "$@" ;;
    start) cmd_start "$@" ;;
    install) cmd_install "$@" ;;
    launch) cmd_launch "$@" ;;
    check|verify) cmd_check "$@" ;;
    help|-h|--help) usage ;;
    *)
      usage >&2
      die "unknown command '$command_name'"
      ;;
  esac
}

main "$@"
