//! Authentication policy DTOs and official-client PIN helpers.
//!
//! The server is authoritative for authentication state, but official clients
//! need stable policy contracts before they can safely render setup flows or
//! derive device/PIN proof material. These DTOs intentionally avoid server
//! domain types so desktop, mobile, and future player crates can share them.

/// Minimum raw PIN length official clients should require by default.
pub const PIN_MIN_LENGTH: usize = 4;
/// Maximum raw PIN length official clients should permit by default.
pub const PIN_MAX_LENGTH: usize = 8;
/// Maximum run of identical PIN digits allowed by default.
pub const PIN_MAX_CONSECUTIVE_IDENTICAL: usize = 2;

/// Password policy as exposed by setup/security-settings endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PasswordPolicyResponse {
    /// Whether the server rejects passwords that do not meet these rules.
    #[cfg_attr(feature = "serde", serde(default))]
    pub enforce: bool,
    /// Minimum password length when enforcement is active.
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_password_min_length")
    )]
    pub min_length: u16,
    /// Require at least one uppercase letter when enforced.
    #[cfg_attr(feature = "serde", serde(default))]
    pub require_uppercase: bool,
    /// Require at least one lowercase letter when enforced.
    #[cfg_attr(feature = "serde", serde(default))]
    pub require_lowercase: bool,
    /// Require at least one ASCII digit when enforced.
    #[cfg_attr(feature = "serde", serde(default))]
    pub require_number: bool,
    /// Require at least one non-alphanumeric character when enforced.
    #[cfg_attr(feature = "serde", serde(default))]
    pub require_special: bool,
}

impl PasswordPolicyResponse {
    /// Relaxed default used for first administrator setup flows.
    pub fn relaxed_admin_default() -> Self {
        Self {
            enforce: false,
            min_length: default_password_min_length(),
            require_uppercase: true,
            require_lowercase: true,
            require_number: true,
            require_special: false,
        }
    }

    /// Relaxed default used for regular user password flows.
    pub fn relaxed_user_default() -> Self {
        Self {
            enforce: false,
            min_length: default_password_min_length(),
            require_uppercase: false,
            require_lowercase: false,
            require_number: false,
            require_special: false,
        }
    }
}

impl Default for PasswordPolicyResponse {
    fn default() -> Self {
        Self::relaxed_user_default()
    }
}

/// PIN policy as exposed by setup/security/device-auth endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PinPolicyResponse {
    /// Minimum raw PIN length official clients must require.
    #[cfg_attr(feature = "serde", serde(default = "default_pin_min_length"))]
    pub min_length: u16,
    /// Maximum raw PIN length official clients must permit.
    #[cfg_attr(feature = "serde", serde(default = "default_pin_max_length"))]
    pub max_length: u16,
    /// Whether official clients must reject non-ASCII-digit characters.
    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub require_numeric: bool,
    /// Whether official clients must reject repeated digit runs.
    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub reject_repeated_digits: bool,
    /// Maximum allowed run of identical digits when repeated-digit checks are on.
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_max_consecutive_identical")
    )]
    pub max_consecutive_identical: u16,
    /// Whether official clients must reject ascending/descending simple sequences.
    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub reject_sequential_digits: bool,
}

impl Default for PinPolicyResponse {
    fn default() -> Self {
        Self {
            min_length: default_pin_min_length(),
            max_length: default_pin_max_length(),
            require_numeric: true,
            reject_repeated_digits: true,
            max_consecutive_identical: default_max_consecutive_identical(),
            reject_sequential_digits: true,
        }
    }
}

/// Device trust, remember-device, lockout, and admin PIN-unlock policy.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeviceTrustPolicyResponse {
    /// Default value official clients should use for remember-device controls.
    #[cfg_attr(feature = "serde", serde(default))]
    pub remember_device_default: bool,
    /// Number of days to trust remembered devices.
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_trust_duration_days")
    )]
    pub trust_duration_days: u16,
    /// Failed PIN attempts allowed before lockout.
    #[cfg_attr(feature = "serde", serde(default = "default_pin_max_attempts"))]
    pub pin_max_attempts: u8,
    /// Lockout duration after too many failed PIN attempts.
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_pin_lockout_minutes")
    )]
    pub pin_lockout_minutes: u16,
    /// Whether PIN-authenticated sessions can unlock admin operations.
    #[cfg_attr(feature = "serde", serde(default))]
    pub admin_pin_unlock_enabled: bool,
}

impl Default for DeviceTrustPolicyResponse {
    fn default() -> Self {
        Self {
            remember_device_default: false,
            trust_duration_days: default_trust_duration_days(),
            pin_max_attempts: default_pin_max_attempts(),
            pin_lockout_minutes: default_pin_lockout_minutes(),
            admin_pin_unlock_enabled: false,
        }
    }
}

/// Device authentication status returned by device auth endpoints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeviceAuthStatus {
    /// Whether the current device is registered with the server.
    pub device_registered: bool,
    /// Whether the selected user has configured a PIN.
    pub has_pin: bool,
    /// Remaining PIN attempts when the server exposes lockout state.
    pub remaining_attempts: Option<u8>,
    /// Current PIN policy official clients should enforce.
    #[cfg_attr(feature = "serde", serde(default))]
    pub pin_policy: PinPolicyResponse,
    /// Current remember-device/trust policy.
    #[cfg_attr(feature = "serde", serde(default))]
    pub device_trust_policy: DeviceTrustPolicyResponse,
}

/// Server setup status shared by setup endpoints and clients.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SetupStatus {
    /// Whether the server still needs first-run setup.
    #[cfg_attr(feature = "serde", serde(default))]
    pub needs_setup: bool,
    /// Whether at least one administrator exists.
    #[cfg_attr(feature = "serde", serde(default))]
    pub has_admin: bool,
    /// Whether clients must supply the configured setup token.
    #[cfg_attr(feature = "serde", serde(default))]
    pub requires_setup_token: bool,
    /// Total number of users known by the server.
    #[cfg_attr(feature = "serde", serde(default))]
    pub user_count: usize,
    /// Total number of libraries known by the server.
    #[cfg_attr(feature = "serde", serde(default))]
    pub library_count: usize,
    /// Current password policy for administrator setup flows.
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_admin_password_policy")
    )]
    pub admin_password_policy: PasswordPolicyResponse,
    /// Current password policy for regular user flows.
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_user_password_policy")
    )]
    pub user_password_policy: PasswordPolicyResponse,
    /// Current PIN policy official clients should enforce.
    #[cfg_attr(feature = "serde", serde(default))]
    pub pin_policy: PinPolicyResponse,
    /// Current device-trust policy official clients should honor.
    #[cfg_attr(feature = "serde", serde(default))]
    pub device_trust_policy: DeviceTrustPolicyResponse,
}

impl Default for SetupStatus {
    fn default() -> Self {
        Self {
            needs_setup: false,
            has_admin: false,
            requires_setup_token: false,
            user_count: 0,
            library_count: 0,
            admin_password_policy:
                PasswordPolicyResponse::relaxed_admin_default(),
            user_password_policy: PasswordPolicyResponse::relaxed_user_default(
            ),
            pin_policy: PinPolicyResponse::default(),
            device_trust_policy: DeviceTrustPolicyResponse::default(),
        }
    }
}

/// Official-client PIN validation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PinPolicyRules {
    /// Minimum raw PIN length.
    pub min_length: usize,
    /// Maximum raw PIN length.
    pub max_length: usize,
    /// Whether only ASCII digits are allowed.
    pub require_numeric: bool,
    /// Whether repeated runs should be rejected.
    pub reject_repeated_digits: bool,
    /// Maximum consecutive identical characters when repeated-run checks are on.
    pub max_consecutive_identical: usize,
    /// Whether simple ascending/descending sequences are rejected.
    pub reject_sequential_digits: bool,
}

impl Default for PinPolicyRules {
    fn default() -> Self {
        Self {
            min_length: PIN_MIN_LENGTH,
            max_length: PIN_MAX_LENGTH,
            require_numeric: true,
            reject_repeated_digits: true,
            max_consecutive_identical: PIN_MAX_CONSECUTIVE_IDENTICAL,
            reject_sequential_digits: true,
        }
    }
}

impl From<&PinPolicyResponse> for PinPolicyRules {
    fn from(value: &PinPolicyResponse) -> Self {
        Self {
            min_length: usize::from(value.min_length),
            max_length: usize::from(value.max_length),
            require_numeric: value.require_numeric,
            reject_repeated_digits: value.reject_repeated_digits,
            max_consecutive_identical: usize::from(
                value.max_consecutive_identical,
            ),
            reject_sequential_digits: value.reject_sequential_digits,
        }
    }
}

/// Human-readable label describing a PIN policy.
pub fn policy_label_for(policy: PinPolicyRules) -> String {
    let mut parts = Vec::new();
    let charset = if policy.require_numeric {
        "digit PIN"
    } else {
        "character PIN"
    };
    parts.push(format!(
        "Use a {}–{} {}",
        policy.min_length, policy.max_length, charset
    ));
    if policy.reject_sequential_digits {
        parts.push("avoid sequences".to_string());
    }
    if policy.reject_repeated_digits {
        parts.push(format!(
            "avoid more than {} repeated in a row",
            policy.max_consecutive_identical
        ));
    }
    format!("{}.", parts.join("; "))
}

/// Validate a raw PIN against the default official-client policy.
pub fn validate_pin(pin: &str) -> Result<(), String> {
    validate_pin_with_policy(pin, PinPolicyRules::default())
}

/// Return whether a raw PIN satisfies a policy.
pub fn pin_satisfies_policy(pin: &str, policy: PinPolicyRules) -> bool {
    validate_pin_with_policy(pin, policy).is_ok()
}

/// Return whether a PIN and confirmation match and satisfy a policy.
pub fn pin_pair_satisfies_policy(
    pin: &str,
    confirm_pin: &str,
    policy: PinPolicyRules,
) -> bool {
    pin == confirm_pin && pin_satisfies_policy(pin, policy)
}

/// Validate a raw PIN against an explicit official-client policy.
pub fn validate_pin_with_policy(
    pin: &str,
    policy: PinPolicyRules,
) -> Result<(), String> {
    if pin.len() < policy.min_length {
        return Err(format!(
            "PIN must be at least {} digits",
            policy.min_length
        ));
    }
    if pin.len() > policy.max_length {
        return Err(format!(
            "PIN must be no more than {} digits",
            policy.max_length
        ));
    }
    if policy.require_numeric && !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("PIN must contain only digits".to_string());
    }
    if policy.reject_repeated_digits
        && has_too_many_repeated_digits(pin, policy.max_consecutive_identical)
    {
        return Err(format!(
            "PIN cannot repeat the same digit more than {} times in a row",
            policy.max_consecutive_identical
        ));
    }
    if policy.reject_sequential_digits && is_sequential(pin, policy.min_length)
    {
        return Err(
            "PIN cannot be a simple sequence like 1234 or 4321".to_string()
        );
    }
    Ok(())
}

fn has_too_many_repeated_digits(
    pin: &str,
    max_consecutive_identical: usize,
) -> bool {
    let mut previous = None;
    let mut run = 0;
    for digit in pin.chars() {
        if Some(digit) == previous {
            run += 1;
        } else {
            previous = Some(digit);
            run = 1;
        }
        if run > max_consecutive_identical {
            return true;
        }
    }
    false
}

fn is_sequential(pin: &str, min_length: usize) -> bool {
    let digits = pin.as_bytes();
    if digits.len() < min_length || !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }

    let ascending = digits.windows(2).all(|pair| pair[1] == pair[0] + 1);
    let descending = digits.windows(2).all(|pair| pair[0] == pair[1] + 1);
    ascending || descending
}

fn default_password_min_length() -> u16 {
    8
}

#[cfg(feature = "serde")]
fn default_admin_password_policy() -> PasswordPolicyResponse {
    PasswordPolicyResponse::relaxed_admin_default()
}

#[cfg(feature = "serde")]
fn default_user_password_policy() -> PasswordPolicyResponse {
    PasswordPolicyResponse::relaxed_user_default()
}

fn default_pin_min_length() -> u16 {
    PIN_MIN_LENGTH as u16
}

fn default_pin_max_length() -> u16 {
    PIN_MAX_LENGTH as u16
}

fn default_max_consecutive_identical() -> u16 {
    PIN_MAX_CONSECUTIVE_IDENTICAL as u16
}

fn default_trust_duration_days() -> u16 {
    30
}

fn default_pin_max_attempts() -> u8 {
    3
}

fn default_pin_lockout_minutes() -> u16 {
    5
}

#[cfg(feature = "serde")]
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_default_pin_policy() {
        assert!(validate_pin("2580").is_ok());
        assert!(validate_pin("123").is_err());
        assert!(validate_pin("123456789").is_err());
        assert!(validate_pin("12a4").is_err());
        assert!(validate_pin("1114").is_err());
        assert!(validate_pin("1234").is_err());
        assert!(validate_pin("4321").is_err());
    }

    #[test]
    fn validates_configured_pin_policy() {
        let policy = PinPolicyRules {
            min_length: 5,
            max_length: 6,
            ..PinPolicyRules::default()
        };
        assert!(validate_pin_with_policy("25809", policy).is_ok());
        assert!(validate_pin_with_policy("2580", policy).is_err());
        assert!(validate_pin_with_policy("2580987", policy).is_err());
    }

    #[test]
    fn validates_matching_pin_pair_with_policy() {
        let policy = PinPolicyRules {
            min_length: 5,
            max_length: 8,
            ..PinPolicyRules::default()
        };

        assert!(pin_pair_satisfies_policy("25809", "25809", policy));
        assert!(!pin_pair_satisfies_policy("25809", "25808", policy));
        assert!(!pin_pair_satisfies_policy("2580", "2580", policy));
    }

    #[test]
    fn setup_status_defaults_include_policy_contracts() {
        let status = SetupStatus::default();
        assert_eq!(status.admin_password_policy.min_length, 8);
        assert!(status.admin_password_policy.require_uppercase);
        assert!(!status.user_password_policy.require_uppercase);
        assert_eq!(status.pin_policy.min_length, PIN_MIN_LENGTH as u16);
        assert_eq!(status.device_trust_policy.pin_max_attempts, 3);
    }
}
