//! Performance configuration constants for the Ferrex Player
//! 
//! This module centralizes all performance-related settings to make tuning easier.
//! Adjust these values based on your target hardware and performance requirements.

use std::time::Duration;

/// Scrolling performance configuration
pub mod scrolling {
    
    /// Velocity threshold (pixels/second) to switch to fast scrolling mode
    /// Lower values = more aggressive fast mode activation
    pub const FAST_SCROLL_THRESHOLD: f32 = 5000.0;
    
    /// Minimum velocity to skip poster loading entirely
    pub const MIN_VELOCITY_FOR_POSTER_SKIP: f32 = 1000.0;
    
    /// Time to wait before considering scroll stopped (milliseconds)
    /// Lower values = quicker poster loading after scroll
    pub const SCROLL_STOP_DEBOUNCE_MS: u64 = 10;
    
    /// Number of rows to preload ahead of visible area
    pub const PRELOAD_AHEAD_ROWS: usize = 2;
    
    /// Number of rows to preload below visible area
    pub const PRELOAD_BELOW_ROWS: usize = 5;
}

/// Poster loading performance configuration
pub mod posters {
    use super::*;
    
    /// Maximum concurrent poster network loads
    pub const MAX_CONCURRENT_LOADS: usize = 2;
    
    /// Maximum posters to mark for loading per tick
    pub const MAX_TO_MARK_PER_TICK: usize = 4;
    
    /// Maximum concurrent poster processing tasks in background
    pub const MAX_BACKGROUND_PROCESSING: usize = 4;
    
    /// Poster monitor tick interval
    pub const MONITOR_TICK_INTERVAL: Duration = Duration::from_millis(100);
    
    /// Image processing settings
    pub mod processing {
        /// Use Triangle filter for faster thumbnail generation (vs Lanczos3)
        pub const USE_FAST_FILTER: bool = true;
        
        /// JPEG quality for thumbnails (0-100)
        pub const THUMBNAIL_JPEG_QUALITY: u8 = 90;
        
        /// JPEG quality for full-size images (0-100)
        pub const FULLSIZE_JPEG_QUALITY: u8 = 85;
        
        /// Maximum size before re-encoding full-size images
        pub const MAX_FULLSIZE_BYTES: usize = 2_000_000; // 2MB
    }
}

/// Animation performance configuration
pub mod animations {
    use super::*;
    
    /// Maximum concurrent animations
    pub const MAX_CONCURRENT_ANIMATIONS: usize = 5;
    
    /// Animation frame rate (milliseconds between frames)
    /// 25ms = 40fps, 16ms = 60fps
    pub const FRAME_INTERVAL_MS: u64 = 25;
    
    /// Flip animation duration
    pub const FLIP_DURATION: Duration = Duration::from_millis(800);
    
    /// Fade animation duration
    pub const FADE_DURATION: Duration = Duration::from_millis(600);
}

/// Virtual grid performance configuration
pub mod grid {
    /// Overscan rows during normal scrolling
    pub const NORMAL_OVERSCAN_ROWS: usize = 3;
    
    /// Overscan rows during fast scrolling (0 for best performance)
    pub const FAST_OVERSCAN_ROWS: usize = 0;
}

/// Debug and profiling configuration
pub mod debug {
    /// Enable verbose performance logging
    pub const VERBOSE_PERF_LOGGING: bool = false;
    
    /// Frame time warning threshold (milliseconds)
    pub const FRAME_TIME_WARNING_MS: u64 = 16;
}