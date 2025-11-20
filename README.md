# Rusty Media Server

A high-performance, self-hosted media server written in Rust with native cross-platform clients. Think Plex, but faster and with native desktop applications.

## 🎬 Overview

Rusty Media Server provides a complete media server solution with:
- High-performance streaming server built with Axum
- Native desktop client using Iced (no Electron!)
- Smart media organization and metadata extraction
- Hardware-accelerated video playback
- Automatic media library scanning
- Real-time streaming with range request support

## ✨ Features

- 🚀 **High Performance**: Built entirely in Rust for maximum speed and efficiency
- 🎬 **Smart Organization**: Automatic media detection, categorization, and metadata extraction
- 🖥️ **Native Desktop Client**: GPU-accelerated playback without web browser overhead
- 📂 **Server-Managed Library**: Plex-like media management - just point to your media folders
- 🎯 **Automatic Metadata**: Extracts video codec, resolution, duration, and more using FFmpeg
- 🔄 **Smart Streaming**: HTTP range request support for instant seeking
- 🗄️ **Modern Database**: SurrealDB for flexible media queries

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
- Linux/macOS/Windows

### Quick Start

```bash
# Clone the repository
git clone https://github.com/yourusername/rusty_media_server
cd rusty_media_server

# Set up environment
cp .env.example .env
# Edit .env and set MEDIA_ROOT to your media directory

# Run the server
cd server
MEDIA_ROOT=/path/to/your/media cargo run

# Run the desktop client (in another terminal)
cd desktop_iced
cargo run
```

Click "Scan Media Library" in the client to index your media!

## 🎮 Usage

### Server Configuration

The server uses environment variables for configuration:

- `MEDIA_ROOT`: Path to your media files directory
- `SERVER_PORT`: Port to run the server on (default: 3000)
- `DATABASE_URL`: Optional PostgreSQL connection string (uses in-memory DB by default)
- `RUST_LOG`: Log level (default: debug)

### Desktop Client Features

- **Library View**: Browse all your media files in a grid layout
- **Scan Button**: Trigger server-side media scanning
- **Video Player**: Built-in player with play/pause, seeking, and volume controls
- **Automatic Refresh**: Library updates automatically after scanning

## 🛠️ API Endpoints

- `GET /ping` - Health check
- `POST /library/scan-and-store` - Scan media directory and update database
- `GET /library` - Get all media files  
- `GET /stream/{id}` - Stream a specific media file with range request support

## 📦 Supported Media Formats

The server automatically detects and indexes these video formats:
- MP4, MKV, AVI, MOV, WebM, FLV, WMV
- M4V, MPG, MPEG, 3GP, OGV
- TS, MTS, M2TS

## 🏗️ Architecture

### Server
- **Web Framework**: Axum for high-performance async HTTP
- **Database**: SurrealDB (embedded mode for easy deployment)
- **Media Processing**: FFmpeg for metadata extraction
- **Streaming**: HTTP range requests for efficient video delivery

### Desktop Client
- **UI Framework**: Iced - native Rust GUI with GPU acceleration
- **Video Playback**: GStreamer integration via iced_video_player
- **HTTP Client**: reqwest for API communication

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📝 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🙏 Acknowledgments

- Built with [Axum](https://github.com/tokio-rs/axum) web framework
- Desktop UI powered by [Iced](https://github.com/iced-rs/iced)
- Media processing with [FFmpeg](https://ffmpeg.org/)
- Database powered by [SurrealDB](https://surrealdb.com/)