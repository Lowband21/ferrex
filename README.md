# Ferrex Media Server

A high-performance, self-hosted media server written in Rust with native cross-platform clients. Think Plex, but faster and with native desktop applications.

## 🎬 Overview

Ferrex Media Server provides a complete media server solution with:
- High-performance streaming server built with Axum
- Native desktop client using Iced (no Electron!)
- Smart media organization with TMDB metadata integration
- Hardware-accelerated video playback with GStreamer
- Automatic media library scanning and poster fetching
- Real-time streaming with HTTP range request support
- Netflix-style UI with horizontal scrolling categories

## ✨ Features

- 🚀 **High Performance**: Built entirely in Rust for maximum speed and efficiency
- 🎬 **Smart Organization**: Automatic media detection, categorization, and TMDB metadata
- 🖥️ **Native Desktop Client**: GPU-accelerated playback without web browser overhead
- 📂 **Rich Media Library**: Beautiful poster display with detailed media information
- 🎯 **Advanced Playback**: Variable speed with pitch correction, seeking, volume control
- 🔄 **Smart Streaming**: HTTP range request support for instant seeking
- 🎨 **Modern UI**: Netflix-style interface with smooth scrolling and animations
- 🔍 **Metadata Integration**: Automatic poster and plot fetching from TMDB

## Project Structure

- `server/` - Axum-based media server
- `core/` - Shared Rust library (scanning, metadata, database)
- `desktop_iced/` - Iced-based native desktop client
- `test-media/` - Sample media files for testing
- `scripts/` - Development utilities

## Development Setup

### Prerequisites

- Rust 1.75+ with cargo
- FFmpeg libraries installed
- GStreamer 1.0+ (for desktop client)
- TMDB API key (optional, for metadata)
- Linux/macOS/Windows

### Quick Start

```bash
# Clone the repository
git clone https://github.com/yourusername/ferrex_media_server
cd ferrex_media_server

# Set up environment
cp .env.example .env
# Edit .env and set:
# - MEDIA_ROOT to your media directory
# - TMDB_API_KEY for poster fetching (optional)

# Run the server
cargo run --bin ferrex-server

# Run the desktop client (in another terminal)
cargo run --bin ferrex-player
```

Click "Scan Media Library" in the client to index your media!

### macOS Specific Instructions

On macOS, you'll need to install dependencies via Homebrew and use special scripts for the desktop client:

```bash
# Install dependencies
brew install ffmpeg gstreamer pkg-config

# Set up environment
source ./setup_env.sh

# Run the desktop client with proper library paths
./run_desktop.sh
```

**Note**: For best experience, run the desktop client in release mode: `./run_desktop.sh --release`

## 🎮 Usage

### Server Configuration

The server uses environment variables for configuration:

- `MEDIA_ROOT`: Path to your media files directory
- `SERVER_PORT`: Port to run the server on (default: 3000)
- `TMDB_API_KEY`: TMDB API key for fetching movie/TV metadata
- `CACHE_DIR`: Directory for poster cache (default: ./cache)
- `RUST_LOG`: Log level (default: debug)

### Desktop Client Features

- **Library View**: Netflix-style horizontal scrolling categories with poster art
- **Detail View**: See plot, cast, ratings, and technical information
- **Video Player**: Advanced controls including:
  - Seeking with preview
  - Variable playback speed (0.5x - 2x) with pitch correction
  - Volume control and mute
  - Fullscreen mode
  - Keyboard shortcuts
- **Metadata System**: Automatic poster and information fetching from TMDB
- **Smart Categories**: Automatic grouping into Movies, TV Shows, etc.

## 🛠️ API Endpoints

- `GET /ping` - Health check
- `POST /library/scan-and-store` - Scan media directory and update database
- `GET /library` - Get all media files with metadata
- `GET /stream/{id}` - Stream a specific media file with range request support
- `POST /metadata/fetch/{id}` - Fetch metadata for a media item
- `GET /poster/{id}` - Get cached poster image

## 📦 Supported Media Formats

The server automatically detects and indexes these video formats:
- MP4, MKV, AVI, MOV, WebM, FLV, WMV
- M4V, MPG, MPEG, 3GP, OGV
- TS, MTS, M2TS

## 🏗️ Architecture

### Server
- **Web Framework**: Axum for high-performance async HTTP
- **Media Processing**: FFmpeg for metadata extraction
- **Metadata Provider**: TMDB integration for movie/TV information
- **Streaming**: HTTP range requests for efficient video delivery
- **Caching**: Disk-based poster cache for performance

### Desktop Client
- **UI Framework**: Iced - native Rust GUI with GPU acceleration
- **Video Playback**: GStreamer integration with custom enhancements
- **HTTP Client**: reqwest for API communication
- **UI Design**: Netflix-inspired with smooth scrolling and animations

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📝 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🙏 Acknowledgments

- Built with [Axum](https://github.com/tokio-rs/axum) web framework
- Desktop UI powered by [Iced](https://github.com/iced-rs/iced)
- Media processing with [FFmpeg](https://ffmpeg.org/)
- Video playback with [GStreamer](https://gstreamer.freedesktop.org/)
- Metadata from [TMDB](https://www.themoviedb.org/)