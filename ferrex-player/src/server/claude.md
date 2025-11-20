# Server Module - HTTP Server for HLS Streaming

## Overview
Local HTTP server providing HLS streaming and library scanning functionality. Essential component for video playback.

## Components

### hls.rs (559 lines)
- **Status**: ACTIVE & CRITICAL
- HLS (HTTP Live Streaming) server implementation
- Segment generation and serving
- Master playlist generation
- Bandwidth adaptation support

### scan.rs
- **Status**: ACTIVE
- Library scanning orchestration
- File system traversal
- Media file detection
- Progress reporting via SSE

## Functionality

### HLS Streaming
- On-demand transcoding with FFmpeg
- Multiple quality variants
- Segment caching
- Bandwidth-aware streaming

### Library Scanning
- Recursive directory scanning
- Media file type detection
- Metadata extraction coordination
- Progress updates to UI

## Integration Points
- Called by player for video URLs
- Triggered by admin UI for scans
- Communicates via SSE for real-time updates

## Performance Considerations
- Segment pre-generation for smooth playback
- Cache management for transcoded segments
- Efficient file system traversal

## Known Issues
- Transcoding can be CPU intensive
- Segment cleanup could be improved
- No persistent segment cache across restarts

## Future Improvements
- Hardware-accelerated transcoding
- Persistent segment cache
- More intelligent quality ladder generation
- Better error recovery