//! Temporary shim for official-client PIN policy helpers.
//!
//! The implementations live in `ferrex-player-foundation` so future player
//! crates and mobile clients can share the same setup/auth policy behavior while
//! existing `ferrex-player` imports continue to compile.

pub use ferrex_player_foundation::auth::{
    PIN_MAX_CONSECUTIVE_IDENTICAL, PIN_MAX_LENGTH, PIN_MIN_LENGTH,
    PinPolicyRules, pin_pair_satisfies_policy, pin_satisfies_policy,
    policy_label_for, validate_pin, validate_pin_with_policy,
};
