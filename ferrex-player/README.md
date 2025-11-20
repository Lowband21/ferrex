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

## Architecture

This client uses:
- **Iced**: Native GUI framework
- **iced_video_player**: GStreamer-based video widget for Iced
- **GStreamer**: Media playback backend
- **Tokio**: Async runtime for network requests

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