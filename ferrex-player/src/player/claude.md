# Player Module - Video Playback System

## Overview
GStreamer-based video player with HLS support, custom controls, and HDR handling. This is a core module that's actively used and maintained.

## Architecture

### Core Components
- **video.rs** - GStreamer pipeline management
  - HDR detection and tonemapping
  - HLS variant selection
  - Pipeline state management
- **controls.rs** (1142 lines) - Video player UI controls
  - Custom control overlay
  - Quality selection menu
  - Seek bar implementation
- **state.rs** - Player-specific state management
  - Playback position tracking
  - Quality variant tracking
  - Buffering state

### Message System
- **messages.rs** - Player-specific messages
  - Separate from main app messages
  - Handles playback events
- **update.rs** (546 lines) - Player message handlers
  - Playback control (play/pause/seek)
  - Quality switching
  - Error handling

### Supporting Features
- **track_selection.rs** - Audio/subtitle track management
  - PARTIALLY IMPLEMENTED: Basic structure exists
  - Needs completion for full multi-track support
- **subtitle_parsers.rs** - WebVTT parsing
  - UNFINISHED: Basic parser structure
  - Would enable subtitle rendering
- **theme.rs** (498 lines) - Player-specific theming
  - Control styling
  - Overlay appearance

## Implementation Status

### Fully Implemented
- Basic playback controls (play/pause/seek)
- HLS streaming with quality selection
- HDR detection and tonemapping
- Fullscreen support
- Volume control

### Partially Implemented
- Track selection UI exists but not fully wired
- Subtitle parsing started but not rendering
- Bandwidth adaptation could be improved

### Known Issues
- Seeking can be sluggish on some streams
- Quality switching sometimes causes brief stutters
- Subtitle support incomplete

## GStreamer Pipeline
Uses playbin3 with custom configuration:
- Hardware acceleration when available
- Automatic format detection
- Buffer management for smooth playback

## Performance Considerations
- Debounced seek operations
- Efficient overlay rendering
- Minimal UI updates during playback