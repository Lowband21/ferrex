//! Profile section state
//!
//! Contains all state related to user profile settings.

/// Profile settings state
#[derive(Debug, Clone, Default)]
pub struct ProfileState {
    // Account subsection
    /// User's display name
    pub display_name: String,
    /// User's email address
    pub email: String,
    /// Avatar URL or identifier (future)
    pub avatar: Option<String>,

    // UI state
    /// Whether a save operation is in progress
    pub loading: bool,
    /// Error message from last operation
    pub error: Option<String>,
    /// Success message from last operation
    pub success_message: Option<String>,

    // Form dirty tracking
    /// Whether the form has unsaved changes
    pub is_dirty: bool,
}

impl ProfileState {
    /// Reset form to clean state
    pub fn reset(&mut self) {
        self.loading = false;
        self.error = None;
        self.success_message = None;
        self.is_dirty = false;
    }

    /// Mark form as having unsaved changes
    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
        self.success_message = None;
    }
}
