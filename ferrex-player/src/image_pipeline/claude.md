# Image Pipeline - Media Artwork Management

## Overview
Handles loading, caching, and processing of media artwork (posters, backdrops, thumbnails). Critical for performance with large media libraries.

## Components

### cache.rs
- **Status**: ACTIVE but may be DEPRECATED
- In-memory image cache with size limits
- LRU eviction policy
- May be replaced by UnifiedImageService

### loader.rs
- **Status**: UNCLEAR - Check integration with UnifiedImageService
- Async image loading from URLs
- HTTP client integration
- Error handling and retries

### processor.rs
- **Status**: ACTIVE
- Image resizing and optimization
- Format conversion
- Thumbnail generation

## Current State Analysis

### Potential Redundancy
The codebase appears to have multiple image handling systems:
1. This image_pipeline module
2. UnifiedImageService (newer approach?)
3. Direct Iced image handle usage

### Integration Points
- Used by view components for poster display
- Integrated with metadata fetching
- Cache shared across views

## Performance Optimizations
- Memory-limited cache
- Lazy loading with placeholders
- Progressive image loading
- Debounced fetch operations

## Refactoring Considerations
Need to determine if this module should be:
1. Kept and enhanced
2. Merged with UnifiedImageService
3. Deprecated in favor of unified approach

## Cache Strategy
- In-memory cache with configurable size limit
- Disk cache for persistent storage (if implemented)
- Network cache headers respected
- Automatic cleanup of old entries