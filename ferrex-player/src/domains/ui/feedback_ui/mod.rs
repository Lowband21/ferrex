pub mod update;

use std::time::Instant;

use crate::domains::ui::messages::UiMessage;

pub use update::update_feedback_ui;

/// Unique identifier for a toast
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToastId(u64);

impl ToastId {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// Toast notification levels
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// A toast notification to display
#[derive(Clone, Debug)]
pub struct ToastNotification {
    pub message: String,
    pub level: ToastLevel,
}

impl ToastNotification {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: ToastLevel::Info,
        }
    }

    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: ToastLevel::Success,
        }
    }

    #[allow(dead_code)]
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: ToastLevel::Warning,
        }
    }

    #[allow(dead_code)]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: ToastLevel::Error,
        }
    }
}

/// Active toast with expiry tracking
#[derive(Clone, Debug)]
pub struct ActiveToast {
    pub id: ToastId,
    pub message: String,
    pub level: ToastLevel,
    pub expires_at: Instant,
}

/// Container for managing toast notifications
#[derive(Debug, Default)]
pub struct ToastManager {
    pub toasts: Vec<ActiveToast>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self { toasts: Vec::new() }
    }

    /// Push a new toast with the specified duration
    pub fn push(
        &mut self,
        notification: ToastNotification,
        duration: std::time::Duration,
    ) -> ToastId {
        let id = ToastId::new();
        let toast = ActiveToast {
            id,
            message: notification.message,
            level: notification.level,
            expires_at: Instant::now() + duration,
        };
        self.toasts.push(toast);
        id
    }

    /// Dismiss a specific toast
    pub fn dismiss(&mut self, id: ToastId) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Remove expired toasts, returns true if any were removed
    pub fn cleanup_expired(&mut self) -> bool {
        let now = Instant::now();
        let before = self.toasts.len();
        self.toasts.retain(|t| t.expires_at > now);
        self.toasts.len() != before
    }

    /// Check if there are any active toasts
    pub fn has_toasts(&self) -> bool {
        !self.toasts.is_empty()
    }
}

#[derive(Clone)]
pub enum FeedbackMessage {
    ClearError,
    /// Show a toast notification
    ShowToast(ToastNotification),
    /// Dismiss a specific toast by ID
    DismissToast(ToastId),
    /// Tick to check for expired toasts
    ToastTick,
}

impl From<FeedbackMessage> for UiMessage {
    fn from(msg: FeedbackMessage) -> Self {
        UiMessage::Feedback(msg)
    }
}

impl FeedbackMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ClearError => "UI::ClearError",
            Self::ShowToast(_) => "UI::ShowToast",
            Self::DismissToast(_) => "UI::DismissToast",
            Self::ToastTick => "UI::ToastTick",
        }
    }
}

impl std::fmt::Debug for FeedbackMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClearError => write!(f, "UI::ClearError"),
            Self::ShowToast(toast) => write!(f, "UI::ShowToast({:?})", toast),
            Self::DismissToast(id) => write!(f, "UI::DismissToast({:?})", id),
            Self::ToastTick => write!(f, "UI::ToastTick"),
        }
    }
}
