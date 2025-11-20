//! Security utilities for the Ferrex Player
//!
//! This module provides secure handling of sensitive data like credentials,
//! tokens, and other authentication-related information.

pub mod secure_credential;

pub use secure_credential::SecureCredential;
