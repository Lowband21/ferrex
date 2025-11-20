# Views Module - UI Components

## Overview
This module contains all UI views and components for the media player. Uses Iced's component system with a focus on performance through virtual scrolling.

## Directory Structure

### Core Views
- **all.rs** - Combined view showing all media types
- **header.rs** (419 lines) - Navigation header with library selector
- **library.rs** (414 lines) - Library view with media grid
- **loading.rs** - Loading states and spinners
- **error.rs** - Error display components
- **macros.rs** (533 lines) - UI generation macros (poster creation, etc.)

### Feature-Specific Views
- **movies/** - Movie list and detail views
  - `view_movie_detail.rs` - Movie details with backdrop
- **tv/** - TV show views
  - `view_tv.rs` (768 lines) - TV show details with seasons
- **admin/** - Administrative interface
  - Library management forms
  - System settings
- **scanning/** - Library scan progress overlay

### Reusable Components
- **cards/** - Media card components
  - UNFINISHED: Appears to be a new card system implementation
  - May replace legacy card functions in components.rs
- **carousel/** - Horizontal scrolling carousels
  - Used for featured content
  - Has windowed rendering for performance
- **grid/** - Virtual grid implementation
  - `virtual_list.rs` - Performance-critical virtual scrolling
  - `macros.rs` - Grid generation helpers

## Code Status

### Active/Needed
- Virtual scrolling system - Critical for performance
- Header navigation - Core UI element
- Movie/TV detail views - Primary user interface
- Admin views - Required for library management

### Legacy/Deprecated
- Some card creation functions in parent components.rs
- Old poster generation methods (replaced by macros)

### Unfinished but Valuable
- **cards/** module - New component-based card system
  - More modular than current approach
  - Should eventually replace legacy functions
- Carousel windowing optimizations

## Performance Patterns
- Virtual scrolling for large lists
- Lazy image loading with placeholders
- Debounced scroll handlers
- Cached view calculations