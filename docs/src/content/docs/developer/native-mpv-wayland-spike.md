---
title: "Native mpv Wayland spike"
description: "Pinned protocol-trace fixture, observed mpv protocol inventory, and W0 bridge-boundary findings."
sidebar:
  order: 12
---

This page records W0 research for the [native mpv integration specification](https://github.com/Lowband21/ferrex/blob/dev/docs/specs/native-mpv-playback.md) and the resulting D-022 **HYBRID** decision. GStreamer remains the integrated Wayland backend and an mpv selection uses ordinary native-window presentation until a safer connection path exists. W1–W5 are retained as re-entry criteria rather than active implementation work. This Wayland-only decision does not change the plan to deliver fully integrated native-VO mpv presentation on Windows and macOS.

## Pinned trace fixture

The spike is pinned to **mpv 0.41.0**. The versioned capture tool refuses a different release so protocol drift is explicit:

```bash
./scripts/qa/native_playback_fixtures.py verify
./scripts/qa/native_playback_wayland_trace.py \
  --environment-id wl-wlroots-amd \
  --fixture h264-sdr-8bit.mkv

./scripts/qa/native_playback_wayland_trace.py \
  --environment-id wl-wlroots-amd \
  --fixture hdr10-pq.mkv

./scripts/qa/native_playback_wayland_trace.py \
  --environment-id wl-wlroots-amd \
  --fixture hlg.mkv
```

The tool starts mpv with `--no-config`, `gpu-next`, Vulkan, `waylandvk`, and a private JSON IPC socket. `WAYLAND_DEBUG=client` captures ordinary native-VO traffic while IPC applies these operation markers in order:

1. initial map and first configured VO;
2. pause, exact seek, and resume;
3. resize;
4. fullscreen enter and exit;
5. stop and replacement-file VO reload; and
6. quit and bounded teardown.

This is a native mpv baseline, not bridge traffic. It does not transfer a decoded frame into Iced or wgpu.

Each run is written under `target/native-playback-results/<environment-id>/<UTC-run-id>/` with mode `0700`. `summary.json` contains the fixture hash, safe mpv diagnostics, operation timing, and protocol inventory. `wayland-client.log` contains the operation-correlated trace. Before retention, the tool replaces workspace/home/runtime paths, window titles, output connector/name/description/make/model strings, and seat names. Both artifacts are mode `0600` and remain ignored. Review traces before sharing them even after automatic redaction.

The parser distinguishes:

- **advertised globals**, which describe the compositor rather than mpv usage;
- **bound globals**, including the negotiated version mpv actually requested; and
- **used object interfaces and methods**, including non-global child objects.

Its display-free unit tests cover parsing, path-segment validation, mpv version parsing, and trace redaction.

## Initial wlroots/AMD observation

Three local `wl-wlroots-amd` runs completed against the generated SDR, HDR10/PQ, and HLG fixtures. All selected mpv 0.41.0 `gpu-next`, Vulkan hardware decoding, and the expected input color parameters. Each trace had ten `wl_display.get_registry` requests across mpv/libplacebo/driver activity but exactly one `xdg_surface.get_toplevel` candidate. SDR, PQ, and HLG used the same protocol-interface set; their color-description values differed in the retained trace and mpv diagnostics.

The exact bound-global set on this environment was:

```text
ext_data_control_manager_v1
wl_compositor
wl_data_device_manager
wl_output
wl_seat
wl_shm
wl_subcompositor
wp_color_manager_v1
wp_commit_timing_manager_v1
wp_content_type_manager_v1
wp_cursor_shape_manager_v1
wp_fifo_manager_v1
wp_fractional_scale_manager_v1
wp_linux_drm_syncobj_manager_v1
wp_presentation
wp_single_pixel_buffer_manager_v1
wp_tearing_control_manager_v1
wp_viewporter
xdg_activation_v1
xdg_wm_base
zwp_idle_inhibit_manager_v1
zwp_linux_dmabuf_v1
zwp_tablet_manager_v2
zwp_text_input_manager_v3
zxdg_decoration_manager_v1
```

The complete observed object-interface set was:

```text
ext_data_control_device_v1
ext_data_control_manager_v1
ext_data_control_offer_v1
wl_buffer
wl_callback
wl_compositor
wl_data_device
wl_data_device_manager
wl_data_offer
wl_display
wl_keyboard
wl_output
wl_pointer
wl_region
wl_registry
wl_seat
wl_subcompositor
wl_subsurface
wl_surface
wp_color_management_surface_feedback_v1
wp_color_management_surface_v1
wp_color_manager_v1
wp_commit_timer_v1
wp_commit_timing_manager_v1
wp_content_type_manager_v1
wp_content_type_v1
wp_cursor_shape_device_v1
wp_cursor_shape_manager_v1
wp_fifo_manager_v1
wp_fifo_v1
wp_fractional_scale_manager_v1
wp_fractional_scale_v1
wp_image_description_creator_params_v1
wp_image_description_info_v1
wp_image_description_v1
wp_linux_drm_syncobj_manager_v1
wp_linux_drm_syncobj_surface_v1
wp_linux_drm_syncobj_timeline_v1
wp_presentation
wp_presentation_feedback
wp_single_pixel_buffer_manager_v1
wp_tearing_control_manager_v1
wp_viewport
wp_viewporter
xdg_activation_v1
xdg_surface
xdg_toplevel
xdg_wm_base
zwp_idle_inhibit_manager_v1
zwp_idle_inhibitor_v1
zwp_linux_buffer_params_v1
zwp_linux_dmabuf_feedback_v1
zwp_linux_dmabuf_v1
zwp_tablet_manager_v2
zwp_tablet_seat_v2
zwp_text_input_manager_v3
zwp_text_input_v3
zxdg_decoration_manager_v1
zxdg_toplevel_decoration_v1
```

`summary.json` is the source of truth for per-method requests/events and negotiated versions. This inventory is initial evidence only: KDE, GNOME, NVIDIA, EGL fallback, missing optional globals, and later mpv versions can change it.

## VO surface identification

Connection order is not a safe identity. The trace contains registry activity from the ordinary Wayland client, Vulkan/libplacebo queries, and driver helper queues. Any future bridge must forward auxiliary connections normally and identify a VO candidate by protocol behavior:

1. a private downstream client creates a `wl_surface`;
2. that client asks `xdg_wm_base.get_xdg_surface` for the surface; and
3. its `xdg_surface.get_toplevel` request establishes the candidate that would be virtualized.

The bridge must accept exactly one candidate for the active presenter generation. Zero candidates time out to a structured fallback; a second candidate before explicit VO replacement is an ambiguity failure. Cursor/subsurface creation, registry order, process ID, title, and app ID are not sufficient identities. VO restart first tears down the prior virtual role and advances the generation.

## `wl-proxy` evaluation

Research was anchored to:

- [`mahkoh/wl-proxy` 0.1.3 at `5874a0d`](https://github.com/mahkoh/wl-proxy/commit/5874a0d3d55ad6abfdb53a7e5a635951a9909a86), checked 2026-07-12; and
- [Jellyfin Desktop at `8722abd`](https://github.com/jellyfin/jellyfin-desktop/commit/8722abd2ce0f54e163a75928b72ca79e9b36b550), whose `jfn-wlproxy` wrapper demonstrates mpv shell-role interception.

The upstream `wl-proxy` **crate** is MIT OR Apache-2.0 despite the repository-level GPL license used by its example applications. It has broad generated protocol coverage, file-descriptor forwarding, object handlers, a private acceptor, and current color-management/color-representation, dmabuf, syncobj, presentation, fractional-scale, viewport, content-type, tearing, and FIFO definitions. These are strong reasons to prefer a pinned upstream spike over writing a raw wire parser.

It does not attach its upstream endpoint to an already established `wl_display` object namespace. `StateBuilder` opens/owns an upstream socket or file descriptor. Jellyfin directs mpv to the proxy, interposes `wl_display_connect` to capture mpv's foreign display, and creates its browser-overlay Wayland surfaces on that same mpv-owned downstream connection before handing their object IDs back to the proxy. Its existing browser connection is not the parent Ferrex needs. The application wrapper also changes `WAYLAND_DISPLAY` before mpv creation and leaves listener cleanup to process exit. Ferrex cannot copy those lifecycle and process-global assumptions: Iced already owns its renderer connection, input, and window lifecycle; playback sessions must tear down repeatedly; and changing process environment while another thread may connect is forbidden.

A future re-entry spike may reuse the permissively licensed upstream crate only behind a Linux-only, non-default feature. Ferrex will not copy or fork Jellyfin's GPL application wrapper. A fork is justified only if a small, reviewable upstream-endpoint or teardown change is proven necessary; protocol virtualization remains Ferrex-owned. Pinning and license review are required before adding the dependency.

## Connection-redirection blocker

Stable libmpv has no per-context option that supplies a `wl_display` or a private Wayland socket to the native VO. `WAYLAND_DISPLAY` and `WAYLAND_SOCKET` are process-global, while the VO and Vulkan helper connections may be opened after initialization on internal threads. A temporary environment override around `mpv_initialize` is therefore not safe.

The only candidate currently found that avoids that race is a **startup proxy topology**:

1. start a private, mode-restricted proxy before Iced opens its first Wayland connection;
2. keep the process Wayland endpoint fixed for the application lifetime;
3. unlike Jellyfin's wrapper, accept Iced and all libmpv/driver helper clients into one shared proxy state and upstream namespace;
4. identify and virtualize only the mpv shell-role candidate by the sequence above; and
5. leave every other Iced/mpv protocol object transparently forwarded.

This is broader than directing only mpv to a private socket and therefore conflicts with the current boundary; it is not selected. D-008 is deferred and D-022 records HYBRID rather than introducing thread-local environment tricks, symbol interposition, or an unrelated upstream connection.

## Recorded outcome

For Wayland releases under D-022:

- Auto/integrated playback remains on GStreamer/Subwave;
- explicit mpv playback uses the ordinary native window;
- integrated mpv reports the connection-bootstrap limitation rather than silently selecting a CPU/wgpu frame path; and
- W1–W5 remain deferred until a compliant mechanism or explicit specification amendment establishes a maintainable topology.

The decision is platform-specific. Windows P5 and macOS P6 continue toward fully integrated native-VO mpv presentation with Iced controls and their independent acceptance gates.
