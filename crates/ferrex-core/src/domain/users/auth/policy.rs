use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Password policy describing optional enforcement rules.
///
/// When `enforce` is false policies operate in advisory mode (UI surfaces
/// strength information but the backend does not reject weak passwords).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PasswordPolicy {
    /// Whether the server should reject passwords that do not meet the rules.
    pub enforce: bool,
    /// Minimum length required when `enforce` is true.
    pub min_length: u16,
    /// Require at least one uppercase letter when enforced.
    pub require_uppercase: bool,
    /// Require at least one lowercase letter when enforced.
    pub require_lowercase: bool,
    /// Require at least one number when enforced.
    pub require_number: bool,
    /// Require at least one non-alphanumeric character when enforced.
    pub require_special: bool,
}

impl PasswordPolicy {
    pub fn relaxed_admin_default() -> Self {
        Self {
            enforce: false,
            min_length: 8,
            require_uppercase: true,
            require_lowercase: true,
            require_number: true,
            require_special: false,
        }
    }

    pub fn relaxed_user_default() -> Self {
        Self {
            enforce: false,
            min_length: 8,
            require_uppercase: false,
            require_lowercase: false,
            require_number: false,
            require_special: false,
        }
    }

    /// Evaluate a password against the policy returning failed rule labels.
    pub fn check(&self, password: &str) -> PasswordPolicyCheck {
        let mut failures = Vec::new();

        if self.enforce {
            if password.len() < self.min_length as usize {
                failures.push(PasswordPolicyRule::MinLength(self.min_length));
            }
            if self.require_uppercase
                && !password.chars().any(|c| c.is_uppercase())
            {
                failures.push(PasswordPolicyRule::Uppercase);
            }
            if self.require_lowercase
                && !password.chars().any(|c| c.is_lowercase())
            {
                failures.push(PasswordPolicyRule::Lowercase);
            }
            if self.require_number
                && !password.chars().any(|c| c.is_ascii_digit())
            {
                failures.push(PasswordPolicyRule::Number);
            }
            if self.require_special
                && !password.chars().any(|c| !c.is_alphanumeric())
            {
                failures.push(PasswordPolicyRule::Special);
            }
        }

        PasswordPolicyCheck { failures }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordPolicyRule {
    MinLength(u16),
    Uppercase,
    Lowercase,
    Number,
    Special,
}

impl fmt::Display for PasswordPolicyRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MinLength(len) => {
                write!(f, "minimum length of {} characters", len)
            }
            Self::Uppercase => write!(f, "at least one uppercase letter"),
            Self::Lowercase => write!(f, "at least one lowercase letter"),
            Self::Number => write!(f, "at least one number"),
            Self::Special => write!(f, "at least one special character"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordPolicyCheck {
    pub failures: Vec<PasswordPolicyRule>,
}

impl PasswordPolicyCheck {
    pub fn is_satisfied(&self) -> bool {
        self.failures.is_empty()
    }
}

/// PIN policy that official clients enforce before deriving PIN proofs.
///
/// The server never receives the raw PIN, so it cannot inspect length or
/// patterns after proof derivation. Server-side enforcement is limited to
/// requiring non-empty proof material, device possession, trust expiry, and
/// lockout/rate-limit rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinSecurityPolicy {
    /// Minimum raw PIN length official clients must require.
    pub min_length: u16,
    /// Maximum raw PIN length official clients must permit.
    pub max_length: u16,
    /// Whether official clients must reject non-ASCII-digit characters.
    pub require_numeric: bool,
    /// Whether official clients must reject runs longer than
    /// `max_consecutive_identical`.
    pub reject_repeated_digits: bool,
    /// Maximum allowed run of identical digits when repeated-digit checks are on.
    pub max_consecutive_identical: u16,
    /// Whether official clients must reject ascending/descending sequential PINs.
    pub reject_sequential_digits: bool,
}

impl PinSecurityPolicy {
    pub fn official_client_default() -> Self {
        Self {
            min_length: 4,
            max_length: 8,
            require_numeric: true,
            reject_repeated_digits: true,
            max_consecutive_identical: 2,
            reject_sequential_digits: true,
        }
    }
}

impl Default for PinSecurityPolicy {
    fn default() -> Self {
        Self::official_client_default()
    }
}

/// Device trust, remember-device, lockout, and admin PIN-unlock policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceTrustPolicy {
    /// Default value official clients should use for remember-device controls.
    pub remember_device_default: bool,
    /// Number of days to extend `trusted_until` for remembered/trusted devices.
    pub trust_duration_days: u16,
    /// Failed PIN proof attempts allowed before device lockout.
    pub pin_max_attempts: u8,
    /// Lockout duration after `pin_max_attempts` failures.
    pub pin_lockout_minutes: u16,
    /// Whether PIN-created playback sessions may perform admin operations.
    ///
    /// The default is false. When false, admin routes require full password-
    /// authenticated sessions.
    pub admin_pin_unlock_enabled: bool,
}

impl DeviceTrustPolicy {
    pub fn secure_default() -> Self {
        Self {
            remember_device_default: false,
            trust_duration_days: 30,
            pin_max_attempts: 3,
            pin_lockout_minutes: 5,
            admin_pin_unlock_enabled: false,
        }
    }
}

impl Default for DeviceTrustPolicy {
    fn default() -> Self {
        Self::secure_default()
    }
}

/// Full set of security settings stored for authentication policies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthSecuritySettings {
    pub admin_password_policy: PasswordPolicy,
    pub user_password_policy: PasswordPolicy,
    pub pin_policy: PinSecurityPolicy,
    pub device_trust_policy: DeviceTrustPolicy,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
}

impl Default for AuthSecuritySettings {
    fn default() -> Self {
        Self {
            admin_password_policy: PasswordPolicy::relaxed_admin_default(),
            user_password_policy: PasswordPolicy::relaxed_user_default(),
            pin_policy: PinSecurityPolicy::default(),
            device_trust_policy: DeviceTrustPolicy::default(),
            updated_at: Utc::now(),
            updated_by: None,
        }
    }
}
