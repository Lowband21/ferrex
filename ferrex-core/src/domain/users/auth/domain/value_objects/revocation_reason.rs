use std::fmt;

/// Enumerates the canonical reasons for revoking authentication material.
///
/// Having a strongly typed list keeps logging, metrics, and policy decisions
/// consistent across repository_ports and services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationReason {
    /// Session was rotated as part of the normal refresh flow.
    Rotation,
    /// Refresh reuse was detected after issuing a new token.
    ReuseDetected,
    /// Device trust was explicitly revoked.
    DeviceRevoked,
    /// A superseding session replaced the previous token.
    SessionReplaced,
    /// User-initiated password change.
    PasswordChange,
    /// Administrator forced a password reset.
    AdminPasswordReset,
    /// User explicitly logged out of a session.
    UserLogout,
}

impl RevocationReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rotation => "rotation",
            Self::ReuseDetected => "reuse_detected",
            Self::DeviceRevoked => "device_revoked",
            Self::SessionReplaced => "replaced_by_new_token",
            Self::PasswordChange => "password_change",
            Self::AdminPasswordReset => "admin_password_reset",
            Self::UserLogout => "user_logout",
        }
    }
}

impl fmt::Display for RevocationReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
