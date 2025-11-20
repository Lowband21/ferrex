# Widgets Module - Custom Iced Widgets

## Overview
Custom widgets extending Iced's capabilities, primarily for advanced rendering effects using GPU shaders.

## Components

### Shader-Based Widgets

#### rounded_image_shader.rs (1370 lines)
- **Status**: ACTIVE & CRITICAL
- GPU-accelerated rounded corner rendering for images
- Solves Iced limitation where border_radius doesn't clip images
- Uses WGSL shader with SDF (Signed Distance Field) technique
- Handles image loading from various sources (Path, Bytes, RGBA)
- Includes texture caching to prevent redundant GPU uploads

#### background_shader.rs (1261 lines)
- **Status**: ACTIVE
- Animated gradient background effects
- GPU-rendered dynamic backgrounds
- Used for visual enhancement in detail views
- Performance optimized with minimal GPU overhead

### Helper Widgets

#### image_for.rs
- **Status**: UNCLEAR - May be legacy
- Appears to be an image loading helper
- Associated with unified image loader

## Shader Implementation Details

### Rounded Image Shader
- Located in `shaders/rounded_image.wgsl`
- Uses smoothstep for anti-aliased edges
- Calculates distance to rounded rectangle border
- Discards pixels outside the rounded area

### Background Shader
- Located in `shaders/background.wgsl`
- Animated gradient effects
- Time-based animations
- Efficient GPU rendering

## Performance Characteristics
- GPU shaders run in parallel
- Minimal CPU overhead
- Texture caching prevents redundant uploads
- Frame-rate independent animations

## Known Issues & Solutions
- Iced's native border_radius doesn't clip images
- Solution: Custom shader implementation
- See main CLAUDE.md for detailed explanation

## Future Enhancements
- Additional shader effects (blur, shadows)
- More complex animations
- Shader hot-reloading for development