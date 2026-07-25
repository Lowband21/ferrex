#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

fail_on_match() {
  local pattern="$1"
  local message="$2"
  shift 2

  local matches
  matches="$(rg -n --glob '*.rs' "$pattern" "$@" || true)"
  if [[ -n "$matches" ]]; then
    printf '%s\n' "$matches"
    printf '%s\n' "$message" >&2
    exit 1
  fi
}

# The native-VO milestone deliberately links only mpv's client API. Adding a
# render-context symbol would silently move frame/swapchain responsibility into
# Ferrex and requires a specification amendment, not an incidental import.
fail_on_match \
  '\bmpv_render_(context|param|update|report|frame|api)\b|\bMPV_RENDER_' \
  'native mpv code references the libmpv render API; native-VO must not create an mpv_render_context' \
  crates/ferrex-player-mpv/src \
  crates/ferrex-player-playback/src/mpv_adapter.rs

# libmpv control and native presenters must not gain a decoded-frame upload path
# through wgpu. Iced-facing layout/host acquisition lives at the separate slot
# boundary and therefore is intentionally not searched here.
fail_on_match \
  '\b(wgpu|iced_wgpu)::|\buse[[:space:]]+(wgpu|iced_wgpu)\b' \
  'native mpv control/presenter code imports wgpu; decoded frames must stay in the native VO' \
  crates/ferrex-player-mpv/src \
  crates/ferrex-player-playback/src/mpv_adapter.rs \
  crates/ferrex-player-playback/src/presenter.rs \
  crates/ferrex-player-playback/src/windows_presenter.rs \
  crates/ferrex-player-playback/src/macos_presenter.rs

# Subwave is concrete adapter state. Domain/view policy may use only the
# Ferrex-owned session, snapshot, commands, events, and capability models.
subwave_matches="$(
  rg -n --glob '*.rs' '\bSubwaveVideo\b|\bsubwave_unified::video::(AudioTrack|SubtitleTrack)\b' \
    crates/ferrex-player-playback/src \
    crates/ferrex-player-ui/src \
    crates/ferrex-player-app/src \
    | rg -v '^crates/ferrex-player-playback/src/subwave_adapter\.rs:' \
    || true
)"
if [[ -n "$subwave_matches" ]]; then
  printf '%s\n' "$subwave_matches"
  printf '%s\n' 'Subwave concrete types escaped the playback adapter boundary' >&2
  exit 1
fi

# Raw libmpv ownership stays in its isolated wrapper. The playback adapter may
# consume the wrapper, but UI/app/domain policy must not reach through it.
mpv_owner_matches="$(
  rg -n --glob '*.rs' '\b(libmpv2_sys|ferrex_player_mpv)::' \
    crates/ferrex-player-playback/src \
    crates/ferrex-player-ui/src \
    crates/ferrex-player-app/src \
    | rg -v '^crates/ferrex-player-playback/src/mpv_adapter\.rs:' \
    || true
)"
if [[ -n "$mpv_owner_matches" ]]; then
  printf '%s\n' "$mpv_owner_matches"
  printf '%s\n' 'libmpv wrapper/raw bindings escaped the mpv adapter boundary' >&2
  exit 1
fi

# Player state is event/timer driven. A decoded-frame callback must not return
# as a progress/event polling mechanism during legacy cleanup.
fail_on_match \
  '\bon_new_frame\b|\bNewFrame\b' \
  'player code contains a frame-driven state/progress callback' \
  crates/ferrex-player-playback/src \
  crates/ferrex-player-ui/src \
  crates/ferrex-player-app/src

printf '%s\n' 'native mpv architecture boundary check passed'
