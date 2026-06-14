# ferrex-player-repository

Media repository overlays, yoke caches, and server-scoped disk cache support for
Ferrex player clients.

This crate owns player-side `MediaRepo` access and cache primitives without
depending on Iced, UI view code, playback backends, or the desktop player facade.
