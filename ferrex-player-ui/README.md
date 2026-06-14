# ferrex-player-ui

`ferrex-player-ui` owns the desktop player's presentation layer during the Ferrex player crate decomposition. It contains the Iced UI modules, design tokens, themes, shader widgets and WGSL assets, view models, windows/focus helpers, UI-bound auth handlers, the Iced image service/cache handles, and 10-foot surfaces that were previously compiled directly from `ferrex-player` or temporary lower-crate shims.

The `ferrex-player` package remains the binary/facade and re-exports this crate so existing `ferrex_player::*` imports keep working while the integration stack lands.
