use std::time::Duration;

/// Maximum number of retry attempts for failed image loads.
pub const IMAGE_MAX_RETRY_ATTEMPTS: u8 = 15;

/// Default delay before retrying a pending (202 Accepted) image request.
pub const IMAGE_PENDING_RETRY_DELAY: Duration = Duration::from_millis(750);

/// Minimum delay between retry attempts for transient failures.
pub const IMAGE_RETRY_THROTTLE: Duration = Duration::from_millis(750);
