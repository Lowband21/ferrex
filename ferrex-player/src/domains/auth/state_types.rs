//! Authentication state management with proper state machine
//!
//! This module provides a type-safe state machine for authentication
//! that avoids the Arc<RwLock<Option<T>>> anti-pattern.

use ferrex_core::rbac::UserPermissions;
use ferrex_core::user::{AuthToken, User};
use std::sync::Arc;
use tokio::sync::watch;

/// Authentication state machine
#[derive(Debug, Clone)]
pub enum AuthState {
    /// No authenticated user
    Unauthenticated,

    /// User is authenticated with valid credentials
    Authenticated {
        user: User,
        token: AuthToken,
        permissions: UserPermissions,
        /// Server URL this auth is valid for
        server_url: String,
    },

    /// Token is being refreshed
    Refreshing {
        /// Previous auth state to fall back to if refresh fails
        previous: Box<AuthState>,
    },
}

impl AuthState {
    /// Check if the state represents an authenticated user
    pub fn is_authenticated(&self) -> bool {
        matches!(
            self,
            AuthState::Authenticated { .. } | AuthState::Refreshing { .. }
        )
    }

    /// Get the current user if authenticated
    pub fn user(&self) -> Option<&User> {
        match self {
            AuthState::Authenticated { user, .. } => Some(user),
            AuthState::Refreshing { previous } => previous.user(),
            AuthState::Unauthenticated => None,
        }
    }

    /// Get the current auth token if authenticated
    pub fn token(&self) -> Option<&AuthToken> {
        match self {
            AuthState::Authenticated { token, .. } => Some(token),
            AuthState::Refreshing { previous } => previous.token(),
            AuthState::Unauthenticated => None,
        }
    }

    /// Get the current permissions if authenticated
    pub fn permissions(&self) -> Option<&UserPermissions> {
        match self {
            AuthState::Authenticated { permissions, .. } => Some(permissions),
            AuthState::Refreshing { previous } => previous.permissions(),
            AuthState::Unauthenticated => None,
        }
    }
}

/// Thread-safe authentication state store using watch channel
/// This provides efficient read access without locks
#[derive(Clone, Debug)]
pub struct AuthStateStore {
    sender: Arc<watch::Sender<AuthState>>,
    receiver: watch::Receiver<AuthState>,
}

impl AuthStateStore {
    /// Create a new auth state store
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(AuthState::Unauthenticated);
        Self {
            sender: Arc::new(sender),
            receiver,
        }
    }

    /// Get the current auth state
    pub fn current(&self) -> AuthState {
        self.receiver.borrow().clone()
    }

    /// Check if authenticated without cloning
    pub fn is_authenticated(&self) -> bool {
        self.receiver.borrow().is_authenticated()
    }

    /// Access state without cloning
    pub fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&AuthState) -> R,
    {
        f(&self.receiver.borrow())
    }

    /// Subscribe to auth state changes
    pub fn subscribe(&self) -> watch::Receiver<AuthState> {
        self.receiver.clone()
    }

    /// Update the auth state
    pub fn set(&self, state: AuthState) {
        // Ignore send errors (no receivers)
        let _ = self.sender.send(state);
    }

    /// Transition to authenticated state
    pub fn authenticate(
        &self,
        user: User,
        token: AuthToken,
        permissions: UserPermissions,
        server_url: String,
    ) {
        self.set(AuthState::Authenticated {
            user,
            token,
            permissions,
            server_url,
        });
    }

    /// Transition to refreshing state
    pub fn start_refresh(&self) {
        let current = self.current();
        if matches!(current, AuthState::Authenticated { .. }) {
            self.set(AuthState::Refreshing {
                previous: Box::new(current),
            });
        }
    }

    /// Complete token refresh
    pub fn complete_refresh(&self, token: AuthToken) {
        if let AuthState::Refreshing { previous } = self.current()
            && let AuthState::Authenticated {
                user,
                permissions,
                server_url,
                ..
            } = *previous
        {
            self.set(AuthState::Authenticated {
                user,
                token,
                permissions,
                server_url,
            });
        }
    }

    /// Cancel refresh and restore previous state
    pub fn cancel_refresh(&self) {
        if let AuthState::Refreshing { previous } = self.current() {
            self.set(*previous);
        }
    }

    /// Log out the current user
    pub fn logout(&self) {
        self.set(AuthState::Unauthenticated);
    }
}

impl Default for AuthStateStore {
    fn default() -> Self {
        Self::new()
    }
}
