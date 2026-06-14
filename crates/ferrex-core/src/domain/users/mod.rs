//! User-domain boundary.
//!
//! Groups authentication, authorization, identity models, and management
//! flows under a single namespace so other layers can depend on cohesive
//! submodules instead of scattered top-level exports.

pub mod auth;
pub mod rbac;
pub mod user;
pub mod user_management;
