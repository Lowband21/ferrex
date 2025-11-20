//! API Routes Constants for Ferrex Player
//!
//! Comprehensive collection of all server API endpoints
//! All versioned API routes are prefixed with /api/v1

/// Base API path for versioned endpoints
pub const API_BASE: &str = "/api/v1";

/// System health and status endpoints
pub mod system {
    /// Health check endpoint
    pub const HEALTH: &str = "/health";
    /// Ping endpoint for connectivity test
    pub const PING: &str = "/ping";
}

/// Authentication endpoints
pub mod auth {
    /// User registration
    pub const REGISTER: &str = "/auth/register";
    /// User login
    pub const LOGIN: &str = "/auth/login";
    /// Token refresh
    pub const REFRESH: &str = "/auth/refresh";
    /// User logout
    pub const LOGOUT: &str = "/auth/logout";

    /// Device authentication endpoints
    pub mod device {
        /// Device login
        pub const LOGIN: &str = "/auth/device/login";
        /// PIN-based login
        pub const PIN_LOGIN: &str = "/auth/device/pin";
        /// Check device status
        pub const STATUS: &str = "/auth/device/status";
        /// Set device PIN
        pub const SET_PIN: &str = "/auth/device/pin/set";
        /// List user devices
        pub const LIST: &str = "/auth/device/list";
        /// Revoke device access
        pub const REVOKE: &str = "/auth/device/revoke";

        /// Device trust validation
        pub const VALIDATE_TRUST: &str = "/auth/device/validate";
        pub const REVOKE_TRUST: &str = "/auth/device/revoke-trust";
        pub const LIST_TRUSTED: &str = "/auth/device/trusted";
        pub const EXTEND_TRUST: &str = "/auth/device/extend-trust";
    }

    /// PIN authentication endpoints
    pub mod pin {
        /// Authenticate with PIN
        pub const AUTHENTICATE: &str = "/auth/pin/authenticate";
        /// Set PIN
        pub const SET: &str = "/auth/pin/set";
        /// Remove PIN for device (requires device_id parameter)
        pub const REMOVE: &str = "/auth/pin/remove";
        /// Check PIN availability for device (requires device_id parameter)
        pub const CHECK_AVAILABLE: &str = "/auth/pin/available";
    }
}

/// User management endpoints
pub mod users {
    /// List all users (public endpoint for user selection)
    pub const LIST_PUBLIC: &str = "/users/public";
    /// List users (authenticated)
    pub const LIST: &str = "/users";
    /// List users (authenticated, alternate endpoint)
    pub const LIST_AUTH: &str = "/users/list";
    /// Get current user info
    pub const ME: &str = "/users/me";
    /// Get user by ID (requires user_id parameter)
    pub const GET_BY_ID: &str = "/users";
    /// Update user (requires user_id parameter)
    pub const UPDATE: &str = "/users";
    /// Delete user (requires user_id parameter)
    pub const DELETE: &str = "/users";

    /// User preferences
    pub mod preferences {
        /// Get current user preferences
        pub const GET: &str = "/users/me/preferences";
        /// Update current user preferences
        pub const UPDATE: &str = "/users/me/preferences";
    }

    /// User sessions
    pub mod sessions {
        /// Get user sessions
        pub const LIST: &str = "/users/sessions";
        /// Delete specific session (requires session_id parameter)
        pub const DELETE: &str = "/users/sessions";
        /// Delete all sessions
        pub const DELETE_ALL: &str = "/users/sessions";
    }

    /// User permissions and roles
    pub mod permissions {
        /// Get my permissions
        pub const MY_PERMISSIONS: &str = "/users/me/permissions";
        /// Get user permissions (requires user_id parameter)
        pub const GET: &str = "/users/:id/permissions";
        /// Assign user roles (requires user_id parameter)
        pub const ASSIGN_ROLES: &str = "/users/:id/roles";
        /// Override user permission (requires user_id parameter)
        pub const OVERRIDE: &str = "/users/:id/permissions/override";
    }
}

/// Media endpoints
pub mod media {
    /// Get media by ID (requires media_id parameter)
    pub const GET: &str = "/media";
    /// Query media with filters
    pub const QUERY: &str = "/media/query";
    /// Batch fetch media
    pub const BATCH: &str = "/media/batch";

    /// Media progress tracking
    pub mod progress {
        /// Get media progress (requires media_id parameter)
        pub const GET: &str = "/media/:id/progress";
        /// Mark media as complete (requires media_id parameter)
        pub const COMPLETE: &str = "/media/:id/complete";
        /// Check if media is completed (requires media_id parameter)
        pub const IS_COMPLETED: &str = "/media/:id/is-completed";
    }

    /// Legacy media endpoints (non-versioned)
    pub mod legacy {
        /// Get poster image (requires media_id parameter)
        pub const POSTER: &str = "/poster";
        /// Get thumbnail image (requires media_id parameter)
        pub const THUMBNAIL: &str = "/thumbnail";
    }
}

/// Watch status tracking endpoints
pub mod watch {
    /// Update watch progress
    pub const UPDATE_PROGRESS: &str = "/watch/progress";
    /// Get watch state
    pub const GET_STATE: &str = "/watch/state";
    /// Get continue watching list
    pub const CONTINUE_WATCHING: &str = "/watch/continue";
    /// Clear watch progress (requires media_id parameter)
    pub const CLEAR_PROGRESS: &str = "/watch/progress";
}

/// Streaming endpoints
pub mod stream {
    /// Stream with progress tracking (requires media_id parameter)
    pub const STREAM: &str = "/stream";
    /// Report streaming progress (requires media_id parameter)
    pub const REPORT_PROGRESS: &str = "/stream/:id/progress";

    /// Legacy streaming endpoints (non-versioned)
    pub mod legacy {
        /// Direct media stream (requires media_id parameter)
        pub const DIRECT: &str = "/stream";
        /// HLS playlist (requires media_id parameter)
        pub const HLS_PLAYLIST: &str = "/stream/:id/hls/playlist.m3u8";
        /// HLS segment (requires media_id and segment parameters)
        pub const HLS_SEGMENT: &str = "/stream/:id/hls/:segment";
        /// Transcode stream (requires media_id parameter)
        pub const TRANSCODE: &str = "/stream/:id/transcode";
    }
}

/// Transcoding endpoints (non-versioned)
pub mod transcode {
    /// Start transcoding (requires media_id parameter)
    pub const START: &str = "/transcode";
    /// Get transcode status (requires job_id parameter)
    pub const STATUS: &str = "/transcode/status";
    /// Start adaptive transcoding (requires media_id parameter)
    pub const START_ADAPTIVE: &str = "/transcode/:id/adaptive";
    /// Get segment (requires media_id and segment_number parameters)
    pub const GET_SEGMENT: &str = "/transcode/:id/segment";
    /// Get master playlist (requires media_id parameter)
    pub const MASTER_PLAYLIST: &str = "/transcode/:id/master.m3u8";
    /// Get variant playlist (requires media_id and profile parameters)
    pub const VARIANT_PLAYLIST: &str = "/transcode/:id/variant/:profile/playlist.m3u8";
    /// Get variant segment (requires media_id, profile and segment parameters)
    pub const VARIANT_SEGMENT: &str = "/transcode/:id/variant/:profile/:segment";
    /// Cancel transcode job (requires job_id parameter)
    pub const CANCEL: &str = "/transcode/cancel";
    /// List transcode profiles
    pub const PROFILES: &str = "/transcode/profiles";
    /// Get cache statistics
    pub const CACHE_STATS: &str = "/transcode/cache/stats";
    /// Clear transcode cache (requires media_id parameter)
    pub const CLEAR_CACHE: &str = "/transcode/:id/clear-cache";
}

/// Synchronized playback endpoints
pub mod sync {
    /// Create sync session
    pub const CREATE_SESSION: &str = "/sync/sessions";
    /// Join sync session (requires code parameter)
    pub const JOIN_SESSION: &str = "/sync/sessions/join";
    /// Leave sync session (requires session_id parameter)
    pub const LEAVE_SESSION: &str = "/sync/sessions";
    /// Get sync session state (requires session_id parameter)
    pub const SESSION_STATE: &str = "/sync/sessions/:id/state";
    /// WebSocket endpoint for sync
    pub const WEBSOCKET: &str = "/sync/ws";
}

/// Library management endpoints
pub mod libraries {
    /// List all libraries
    pub const LIST: &str = "/libraries";
    /// Get library (requires library_id parameter)
    pub const GET: &str = "/libraries";
    /// Create library
    pub const CREATE: &str = "/libraries";
    /// Update library (requires library_id parameter)
    pub const UPDATE: &str = "/libraries";
    /// Delete library (requires library_id parameter)
    pub const DELETE: &str = "/libraries";
    /// Scan library (requires library_id parameter)
    pub const SCAN: &str = "/libraries/:id/scan";
    /// Get library media (requires library_id parameter)
    pub const GET_MEDIA: &str = "/libraries/:id/media";

    /// Library events SSE endpoint
    pub const EVENTS_SSE: &str = "/library/events/sse";
}

/// Scanning endpoints (non-versioned)
pub mod scan {
    /// Start scan
    pub const START: &str = "/scan/start";
    /// Scan all libraries
    pub const ALL: &str = "/scan/all";
    /// Get scan progress (requires scan_id parameter)
    pub const PROGRESS: &str = "/scan/progress";
    /// Get scan progress SSE stream (requires scan_id parameter)
    pub const PROGRESS_SSE: &str = "/scan/progress/:id/sse";
    /// Get active scans
    pub const ACTIVE: &str = "/scan/active";
    /// Get scan history
    pub const HISTORY: &str = "/scan/history";
    /// Cancel scan (requires scan_id parameter)
    pub const CANCEL: &str = "/scan/cancel";
}

/// Image serving endpoints
pub mod images {
    /// Serve image (requires type, id, category, index parameters)
    /// Types: movie, series, season, episode, person
    /// Categories: poster, backdrop, logo, still, profile
    pub const GET_IMAGE: &str = "images/:type/:id/:category/:index";
}

/// Setup endpoints (first-run configuration)
pub mod setup {
    /// Check setup status
    pub const STATUS: &str = "/setup/status";
    /// Create initial admin user
    pub const CREATE_ADMIN: &str = "/setup/admin";
}

/// Admin endpoints
pub mod admin {
    /// List all users
    pub const LIST_USERS: &str = "/admin/users";
    /// Assign user roles (requires user_id parameter)
    pub const ASSIGN_ROLES: &str = "/admin/users/:id/roles";
    /// Delete user (requires user_id parameter)
    pub const DELETE_USER: &str = "/admin/users";
    /// Get user sessions (requires user_id parameter)
    pub const GET_USER_SESSIONS: &str = "/admin/users/:id/sessions";
    /// Revoke user session (requires user_id and session_id parameters)
    pub const REVOKE_SESSION: &str = "/admin/users/:user_id/sessions/:session_id";
    /// Get admin statistics
    pub const STATS: &str = "/admin/stats";

    /// Development/debugging endpoints
    pub mod dev {
        /// Check reset status
        pub const RESET_CHECK: &str = "/admin/dev/reset/check";
        /// Reset database
        pub const RESET_DATABASE: &str = "/admin/dev/reset/database";
        /// Seed database with test data
        pub const SEED: &str = "/admin/dev/seed";
    }

    /// Admin session management
    pub mod sessions {
        /// Register admin session
        pub const REGISTER: &str = "/admin/sessions/register";
        /// Remove admin session (requires device_id parameter)
        pub const REMOVE: &str = "/admin/sessions";
    }
}

/// Role and permission management endpoints
pub mod roles {
    /// List all roles
    pub const LIST: &str = "/roles";
    /// List all permissions
    pub const LIST_PERMISSIONS: &str = "/permissions";
}

/// Helper functions for building URLs with parameters
pub mod utils {
    /// Replace a parameter in a route template
    ///
    /// # Example
    /// ```
    /// let url = replace_param("/users/:id", ":id", "123");
    /// assert_eq!(url, "/users/123");
    /// ```
    pub fn replace_param(route: &str, param: &str, value: impl AsRef<str>) -> String {
        route.replace(param, value.as_ref())
    }

    /// Build a URL with multiple parameters
    ///
    /// # Example
    /// ```
    /// let url = build_url("/users/:id/sessions/:session_id", &[
    ///     (":id", "user123"),
    ///     (":session_id", "sess456")
    /// ]);
    /// assert_eq!(url, "/users/user123/sessions/sess456");
    /// ```
    pub fn replace_params(route: &str, params: &[(impl AsRef<str>, impl AsRef<str>)]) -> String {
        let mut url = route.to_string();
        for (param, value) in params {
            url = url.replace(param.as_ref(), value.as_ref());
        }
        url
    }
    /// Add query parameters to a URL
    ///
    /// # Example
    /// ```
    /// let url = with_query("/users", &[("limit", "10"), ("offset", "20")]);
    /// assert_eq!(url, "/users?limit=10&offset=20");
    /// ```
    pub fn with_query(route: &str, params: &[(&str, &str)]) -> String {
        if params.is_empty() {
            return route.to_string();
        }

        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        format!("{}?{}", route, query)
    }
}
