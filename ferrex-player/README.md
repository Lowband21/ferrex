# Ferrex Player (Iced Desktop Client)

A high-performance media player built with Iced and GStreamer, featuring video rendering through iced_video_player.

## Prerequisites

1. **GStreamer**: Install GStreamer and its plugins:
   ```bash
   # Ubuntu/Debian
   sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
       gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
       gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
       gstreamer1.0-libav

   # Fedora
   sudo dnf install gstreamer1-devel gstreamer1-plugins-base-devel \
       gstreamer1-plugins-base gstreamer1-plugins-good \
       gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free

   # macOS
   brew install gstreamer gst-plugins-base gst-plugins-good \
       gst-plugins-bad gst-plugins-ugly gst-libav
   ```

2. **Rust**: Latest stable Rust toolchain

## Running the Application

1. **Start the media server** (in another terminal, from the workspace root):
   ```bash
   cargo run -p ferrex-server
   ```

2. **Run the media player**:
   ```bash
   # Default (connects to http://localhost:3000)
   cargo run -p ferrex-player

   # Or with a custom server URL
   FERREX_SERVER_URL=http://localhost:3000 cargo run -p ferrex-player
   ```

3. **Media scanning**:
   - The player displays media indexed by the server.
   - Make sure the server has at least one library configured (with `MEDIA_ROOT` set) and trigger a scan via the API if needed:
     ```bash
     # Start a scan for a library (replace {id} with the library UUID)
     curl -X POST http://localhost:3000/api/v1/libraries/{id}/scans:start
     ```
   - The media path must exist on the server machine.

## Features

- Hardware-accelerated video playback via GStreamer
- Non-blocking video updates using iced_video_player
- Media library grid view with thumbnails (coming soon)
- Playback controls with auto-hide
- Volume control and seeking
- Support for various video formats
- **Performance Profiling**: Comprehensive profiling infrastructure with Puffin/Tracy support

## Architecture

This client uses:
- **Iced**: Native GUI framework
- **iced_video_player**: GStreamer-based video widget for Iced
- **GStreamer**: Media playback backend
- **Tokio**: Async runtime for network requests
- **Profiling**: Puffin/Tracy/Tracing for performance analysis

## Authentication & "Remember This Device"

- **Opaque session tokens**: Password login returns a short‑lived opaque access token with a refresh token (no JWT). The server keys for auth are `AUTH_PASSWORD_PEPPER` and `AUTH_TOKEN_KEY`.
- **Device Sessions (PIN / Remember Device)**: Device‑aware login can bind a trusted device and PIN. Trusted devices maintain a 30‑day window that refreshes when "Remember this device" is enabled.
- **Auto‑login toggle**: The login screen checkbox and the Settings → Preferences toggle are synchronized.
- **Device management**: Settings → Device Management lists trusted devices, highlights the current device, and allows revocation.

## Performance Profiling

The application includes comprehensive performance profiling infrastructure:

### Running with Profiling

```bash
# Run with Puffin web UI (recommended for development)
cargo run --features puffin-server
# Open http://127.0.0.1:8585 in browser to view profiling data

# Run with Tracy (requires Tracy profiler)
cargo run --features profile-with-tracy

# Run with tracing output
cargo run --features profile-with-tracing
```

### Profiled Areas

- **UI Operations**: Grid rendering, virtual lists, view updates
- **Domain Updates**: All domain message handlers (auth, library, media, etc.)
- **Poster Loading**: Cache hits, network loads, GPU uploads
- **Animations**: Shader transitions, hover effects, flip animations
- **Metadata**: Fetching and processing media metadata

### Performance Targets

- View operations: <8ms
- Frame time: <8.33ms (120fps)
- Scroll frame: <4ms during scrolling
- Poster load: <50ms
- Cache hit rate: >80%

## Troubleshooting

1. **"No media files available"**: Make sure to run `./scan_media.sh` first
2. **Missing codec errors**: Install additional GStreamer plugins
3. **Server connection errors**: Ensure the server is running on the correct port

## Controls

- **Click**: Play/Pause
- **Double-click**: Fullscreen (coming soon)
- **Slider**: Seek through video
- **Volume slider**: Adjust volume
- **Back button**: Return to library
