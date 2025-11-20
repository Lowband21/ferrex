pub mod device_handlers;
pub mod device_validation;
pub mod handlers;
pub mod jwt;
pub mod middleware;
pub mod permission_middleware;
pub mod pin_handlers;
pub mod tls;
pub mod user_preferences;

pub use jwt::{generate_access_token, generate_refresh_token, validate_token, validate_token_sync};
pub use middleware::{admin_middleware, auth_middleware, optional_auth_middleware};
pub use permission_middleware::{
    permission_layer, require_any_permission, require_permission, require_permission_async,
};
