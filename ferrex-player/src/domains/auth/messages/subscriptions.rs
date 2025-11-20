use super::Message;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::domains::auth::state_types::{AuthState, AuthStateStore};
use crate::state_refactored::State;
use futures::stream;
use iced::Subscription;

/// Creates all auth-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // TODO: RUS-136 - Add trait-based auth state monitoring
    // Currently auth state subscriptions are disabled as they depend on internal AuthManager state
    // We'll need to add state change notifications to the AuthService trait

    // Subscribe to token refresh check if authenticated
    if state.domains.auth.state.is_authenticated {
        subscriptions.push(token_refresh_subscription());
    }

    Subscription::batch(subscriptions)
}

/// Subscription data for auth state monitoring
#[derive(Debug, Clone)]
struct AuthStateSubscription {
    auth_state: AuthStateStore,
}

impl std::hash::Hash for AuthStateSubscription {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Use a unique ID for this subscription type
        std::any::TypeId::of::<AuthStateSubscription>().hash(state);
    }
}

/// Creates a subscription for auth state changes
fn auth_state_subscription(auth_state: &AuthStateStore) -> Subscription<DomainMessage> {
    Subscription::run_with(
        AuthStateSubscription {
            auth_state: auth_state.clone(),
        },
        auth_state_stream,
    )
}

/// Stream function for auth state subscription
fn auth_state_stream(
    subscription: &AuthStateSubscription,
) -> impl futures::Stream<Item = DomainMessage> {
    stream::unfold(
        AuthStateTracker::new(subscription.auth_state.clone()),
        |mut tracker| async move {
            // Wait for the next state change
            if let Some(new_state) = tracker.wait_for_change().await {
                // Map state changes to appropriate messages
                let message = match &new_state {
                    AuthState::Authenticated {
                        user, permissions, ..
                    } => {
                        // Trigger login success message which will emit cross-domain events
                        Message::LoginSuccess(user.clone(), permissions.clone())
                    }
                    AuthState::Unauthenticated => {
                        // Check if this is a logout (previous state was authenticated)
                        if tracker.was_authenticated {
                            // Trigger logout complete which will emit cross-domain events
                            Message::LogoutComplete
                        } else {
                            // Initial unauthenticated state, check if setup is needed
                            Message::CheckSetupStatus
                        }
                    }
                    AuthState::Refreshing { .. } => {
                        // Token is being refreshed, no UI action needed
                        // The auth manager handles this internally
                        return None;
                    }
                };

                Some((DomainMessage::Auth(message), tracker))
            } else {
                // Channel closed, stop subscription
                None
            }
        },
    )
}

/// Subscription to periodically check if token needs refresh
fn token_refresh_subscription() -> Subscription<DomainMessage> {
    struct TokenRefreshSubscription;

    // TODO: Implement proper token refresh logic
    // This should NOT use CheckAuthStatus which is meant for startup only
    // For now, disable this subscription to prevent unnecessary operations
    // iced::time::every(Duration::from_secs(30))
    //     .map(|_| DomainMessage::Auth(Message::RefreshTokenIfNeeded))
    Subscription::none()
}

/// Tracks auth state changes for the subscription
struct AuthStateTracker {
    auth_state: AuthStateStore,
    receiver: tokio::sync::watch::Receiver<AuthState>,
    was_authenticated: bool,
}

impl AuthStateTracker {
    fn new(auth_state: AuthStateStore) -> Self {
        let receiver = auth_state.subscribe();
        let was_authenticated = auth_state.is_authenticated();
        Self {
            auth_state,
            receiver,
            was_authenticated,
        }
    }

    /// Wait for the next state change
    async fn wait_for_change(&mut self) -> Option<AuthState> {
        // Wait for a change in the watch channel
        if self.receiver.changed().await.is_ok() {
            let new_state = self.receiver.borrow().clone();

            // Update authentication tracking
            let is_authenticated = new_state.is_authenticated();
            if !is_authenticated && self.was_authenticated {
                // User logged out
                self.was_authenticated = false;
            } else if is_authenticated && !self.was_authenticated {
                // User logged in
                self.was_authenticated = true;
            }

            Some(new_state)
        } else {
            // Channel closed
            None
        }
    }
}
