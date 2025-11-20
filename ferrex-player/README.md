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

1. **Start the media server** (in another terminal):
   ```bash
   cd ../server
   cargo run
   ```

2. **Run the media player**:
   ```bash
   ./run.sh

   # Or with custom server URL
   MEDIA_SERVER_URL=http://localhost:8080 ./run.sh
   ```

3. **Scan for media files**:
   - The application will load any previously scanned media on startup
   - To scan new directories on the server, use the scan controls in the UI:
     - Enter the **server-side** path to scan (e.g., `/home/user/Videos`)
     - Click the "Scan" button
     - The server will scan its local directories and store media metadata
   - Alternatively, use the command-line script:
     ```bash
     ./scan_media.sh /path/on/server/to/media/files
     ```
   
   **Note**: The path must exist on the server machine, not your local machine!

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

## Authentication Tokens & "Remember This Device"

- **Password Login (JWT)**: Standard username/password authentication returns a short-lived JWT (15 minutes by default). The server requires the `JWT_SECRET` environment variable in development (e.g. `export JWT_SECRET=dev-secret` before running tests or the server).
- **Device Sessions (PIN / Remember Device)**: Device-aware login returns a 64-character session token tied to the current device. These tokens are not JWTs and instead rely on a 30-day trust window that refreshes whenever you log in with "Remember this device" enabled.
- **Auto-login toggle**: The login screen checkbox and the Settings → Preferences toggle are now synchronized. Enabling one enables the other, updates the server-side preference, and persists the device trust window so the player can sign in automatically on next launch.
- **Device management**: Settings → Device Management lists trusted devices, highlights the current device, and allows revocation. Revoking a device clears its remembered-session window immediately.

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
