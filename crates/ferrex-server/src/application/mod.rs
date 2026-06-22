//! Application-service layer for server request handlers.
//!
//! Facades in this namespace coordinate core-domain services and repositories so
//! HTTP handlers do not need to know the details of authentication, persistence,
//! or orchestration workflows.

/// Authentication application facade used by setup and login handlers.
pub mod auth;
