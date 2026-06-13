//! Official-client PIN policy helpers.
//!
//! The server stores only client-derived PIN proofs, so desktop/mobile clients
//! enforce raw PIN length and pattern rules before deriving proof material.

pub const PIN_MIN_LENGTH: usize = 4;
pub const PIN_MAX_LENGTH: usize = 8;
pub const PIN_MAX_CONSECUTIVE_IDENTICAL: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinPolicyRules {
    pub min_length: usize,
    pub max_length: usize,
    pub require_numeric: bool,
    pub reject_repeated_digits: bool,
    pub max_consecutive_identical: usize,
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

pub fn validate_pin(pin: &str) -> Result<(), String> {
    validate_pin_with_policy(pin, PinPolicyRules::default())
}

pub fn pin_satisfies_policy(pin: &str, policy: PinPolicyRules) -> bool {
    validate_pin_with_policy(pin, policy).is_ok()
}

pub fn pin_pair_satisfies_policy(
    pin: &str,
    confirm_pin: &str,
    policy: PinPolicyRules,
) -> bool {
    pin == confirm_pin && pin_satisfies_policy(pin, policy)
}

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
}
