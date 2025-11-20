macro_rules! v1_path {
    ($path:literal) => {
        concat!("/api/v1", $path)
    };
}

/// Versioned API route definitions shared across Ferrex services
pub mod v1 {
    pub const ROOT: &str = "/api/v1";
    pub const VERSION: &str = "v1";

    pub mod auth {
        pub const REGISTER: &str = v1_path!("/auth/register");
        pub const LOGIN: &str = v1_path!("/auth/login");
        pub const REFRESH: &str = v1_path!("/auth/refresh");
        pub const LOGOUT: &str = v1_path!("/auth/logout");

        pub mod device {
            pub const LOGIN: &str = v1_path!("/auth/device/login");
            pub const PIN_LOGIN: &str = v1_path!("/auth/device/pin");
            pub const PIN_CHALLENGE: &str =
                v1_path!("/auth/device/pin/challenge");
            pub const STATUS: &str = v1_path!("/auth/device/status");
            pub const SET_PIN: &str = v1_path!("/auth/device/pin/set");
            pub const CHANGE_PIN: &str = v1_path!("/auth/device/pin/change");
            pub const LIST: &str = v1_path!("/auth/device/list");
            pub const REVOKE: &str = v1_path!("/auth/device/revoke");
            pub const VALIDATE_TRUST: &str = v1_path!("/auth/device/validate");
            pub const REVOKE_TRUST: &str =
                v1_path!("/auth/device/revoke-trust");
            pub const LIST_TRUSTED: &str = v1_path!("/auth/device/trusted");
            pub const EXTEND_TRUST: &str =
                v1_path!("/auth/device/extend-trust");
        }
    }

    pub mod users {
        pub const COLLECTION: &str = v1_path!("/users");
        pub const LIST_AUTH: &str = v1_path!("/users/list");
        pub const CURRENT: &str = v1_path!("/users/me");
        pub const CURRENT_PREFERENCES: &str = v1_path!("/users/me/preferences");
        pub const CHANGE_PASSWORD: &str = v1_path!("/users/me/password");
        pub const ITEM: &str = v1_path!("/users/{id}");

        #[deprecated(
            note = "User session routes are being migrated to the auth domain"
        )]
        pub mod sessions {
            pub const COLLECTION: &str = v1_path!("/users/sessions");
            pub const ITEM: &str = v1_path!("/users/sessions/{id}");
        }
    }

    pub mod setup {
        pub const STATUS: &str = v1_path!("/setup/status");
        pub const CREATE_ADMIN: &str = v1_path!("/setup/admin");
        pub const CLAIM_START: &str = v1_path!("/setup/claim/start");
        pub const CLAIM_CONFIRM: &str = v1_path!("/setup/claim/confirm");
    }

    pub mod media {
        pub const QUERY: &str = v1_path!("/media/query");

        pub mod item {
            pub const PROGRESS: &str = v1_path!("/media/{id}/progress");
            pub const COMPLETE: &str = v1_path!("/media/{id}/complete");
            pub const IS_COMPLETED: &str = v1_path!("/media/{id}/is-completed");
        }
    }

    pub mod watch {
        pub const UPDATE_PROGRESS: &str = v1_path!("/watch/progress");
        pub const STATE: &str = v1_path!("/watch/state");
        pub const CONTINUE: &str = v1_path!("/watch/continue");
        pub const CLEAR_PROGRESS: &str = v1_path!("/watch/progress/{media_id}");
        // Identity-based TV helpers
        pub const SERIES_STATE: &str =
            v1_path!("/watch/series/{tmdb_series_id}");
        pub const SEASON_STATE: &str = v1_path!(
            "/watch/series/{tmdb_series_id}/season/{season_number}"
        );
        pub const SERIES_NEXT: &str =
            v1_path!("/watch/series/{tmdb_series_id}/next");
    }

    pub mod folders {
        pub const INVENTORY: &str = v1_path!("/folders/inventory/{library_id}");
        pub const PROGRESS: &str = v1_path!("/folders/progress/{library_id}");
    }

    pub mod libraries {
        pub const COLLECTION: &str = v1_path!("/libraries");
        pub const ITEM: &str = v1_path!("/libraries/{id}");
        pub const MEDIA: &str = v1_path!("/libraries/{id}/media");
        pub const SORTED_IDS: &str = v1_path!("/libraries/{id}/sorted-ids");
        pub const SORTED_INDICES: &str =
            v1_path!("/libraries/{id}/indices/sorted");
        pub const FILTERED_INDICES: &str =
            v1_path!("/libraries/{id}/indices/filter");

        pub mod scans {
            pub const START: &str = v1_path!("/libraries/{id}/scans:start");
            pub const PAUSE: &str = v1_path!("/libraries/{id}/scans:pause");
            pub const RESUME: &str = v1_path!("/libraries/{id}/scans:resume");
            pub const CANCEL: &str = v1_path!("/libraries/{id}/scans:cancel");
        }
    }

    pub mod scan {
        pub const ACTIVE: &str = v1_path!("/scan/active");
        pub const HISTORY: &str = v1_path!("/scan/history");
        pub const PROGRESS: &str = v1_path!("/scan/progress");
        pub const EVENTS: &str = v1_path!("/scan/{id}/events");
        pub const PROGRESS_STREAM: &str = v1_path!("/scan/{id}/progress");
        pub const METRICS: &str = v1_path!("/scan/metrics");
        pub const CONFIG: &str = v1_path!("/scan/config");
    }

    pub mod events {
        pub const MEDIA: &str = v1_path!("/events/media");
    }

    pub mod images {
        pub const SERVE: &str =
            v1_path!("/images/{type}/{id}/{category}/{index}");
    }

    pub mod stream {
        pub const PLAY: &str = v1_path!("/stream/{id}");
        pub const PLAYBACK_TICKET: &str = v1_path!("/stream/{id}/ticket");
        pub const REPORT_PROGRESS: &str =
            v1_path!("/stream/{media_type}/{id}/progress");
    }

    pub mod sync {
        pub const WEBSOCKET: &str = v1_path!("/sync/ws");
    }

    pub mod admin {
        pub const USERS: &str = v1_path!("/admin/users");
        pub const USER_ITEM: &str = v1_path!("/admin/users/{id}");
        pub const USER_ROLES: &str = v1_path!("/admin/users/{id}/roles");
        pub const USER_SESSIONS: &str = v1_path!("/admin/users/{id}/sessions");
        pub const REVOKE_SESSION: &str =
            v1_path!("/admin/users/{user_id}/sessions/{session_id}");
        pub const STATS: &str = v1_path!("/admin/stats");

        pub const MEDIA_ROOT_BROWSER: &str =
            v1_path!("/admin/media/root-browser");

        pub mod dev {
            pub const RESET_CHECK: &str = v1_path!("/admin/dev/reset/check");
            pub const RESET_DATABASE: &str =
                v1_path!("/admin/dev/reset/database");
            pub const SEED: &str = v1_path!("/admin/dev/seed");
        }

        pub mod demo {
            pub const STATUS: &str = v1_path!("/admin/demo/status");
            pub const RESET: &str = v1_path!("/admin/demo/reset");
        }

        pub mod sessions {
            pub const REGISTER: &str = v1_path!("/admin/sessions/register");
            pub const REMOVE: &str = v1_path!("/admin/sessions/{device_id}");
        }

        pub mod security {
            pub const SETTINGS: &str =
                v1_path!("/admin/security/password-policy");
        }
    }

    pub mod roles {
        pub const LIST: &str = v1_path!("/roles");
        pub const PERMISSIONS: &str = v1_path!("/permissions");
        pub const USER_PERMISSIONS: &str = v1_path!("/users/{id}/permissions");
        pub const USER_ROLES: &str = v1_path!("/users/{id}/roles");
        pub const OVERRIDE_PERMISSION: &str =
            v1_path!("/users/{id}/permissions/override");
        pub const MY_PERMISSIONS: &str = v1_path!("/users/me/permissions");
    }
}

/// Helper utilities for working with route templates
pub mod utils {
    /// Replace a single path parameter (e.g. `"{id}"`) with the provided value.
    pub fn replace_param(
        route: &str,
        param: &str,
        value: impl AsRef<str>,
    ) -> String {
        route.replace(param, value.as_ref())
    }

    /// Replace multiple path parameters in order.
    pub fn replace_params(
        route: &str,
        params: &[(impl AsRef<str>, impl AsRef<str>)],
    ) -> String {
        let mut path = route.to_string();
        for (param, value) in params {
            path = path.replace(param.as_ref(), value.as_ref());
        }
        path
    }

    /// Append query parameters to the provided route.
    pub fn with_query(route: &str, params: &[(&str, &str)]) -> String {
        if params.is_empty() {
            return route.to_string();
        }

        let mut path =
            String::with_capacity(route.len() + 1 + params.len() * 8);
        path.push_str(route);
        path.push('?');

        for (i, (key, value)) in params.iter().enumerate() {
            if i > 0 {
                path.push('&');
            }
            path.push_str(key);
            path.push('=');
            path.push_str(value);
        }

        path
    }
}
