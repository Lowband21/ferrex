//! User Management Module
//!
//! This module provides components for managing users including domain models,
//! validation helpers, and application services for administrative workflows.

pub mod application;
pub mod domain;

pub use application::{
    CreateUserCommand, DeleteUserCommand, ListUsersOptions, PaginatedUsers, UpdateUserCommand,
    UserAdminError, UserAdminRecord, UserAdministrationService,
};
pub use domain::*;
