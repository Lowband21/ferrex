//! Users section messages (Admin)

use super::state::UserRole;
use uuid::Uuid;

/// Messages for the users settings section
#[derive(Debug, Clone)]
pub enum UsersMessage {
    // User List
    /// Load list of users
    LoadUsers,
    /// Users loaded result
    UsersLoaded(Result<Vec<super::state::UserSummary>, String>),
    /// Select user for editing
    SelectUser(Uuid),
    /// Delete user
    DeleteUser(Uuid),
    /// Delete result
    DeleteResult(Result<Uuid, String>),
    /// Toggle user active status
    ToggleUserActive(Uuid, bool),
    /// Toggle result
    ToggleActiveResult(Result<(Uuid, bool), String>),

    // User Form
    /// Show add user form
    ShowAddForm,
    /// Show edit user form
    ShowEditForm(Uuid),
    /// Update form username field
    UpdateFormUsername(String),
    /// Update form display name field
    UpdateFormDisplayName(String),
    /// Update form email field
    UpdateFormEmail(String),
    /// Update form password field
    UpdateFormPassword(String),
    /// Update form confirm password field
    UpdateFormConfirmPassword(String),
    /// Update form role
    UpdateFormRole(UserRole),
    /// Update form active status
    UpdateFormActive(bool),
    /// Submit form (create or update)
    SubmitForm,
    /// Form submission result
    FormResult(Result<Uuid, String>),
    /// Cancel form
    CancelForm,
}

impl UsersMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::LoadUsers => "Users::LoadUsers",
            Self::UsersLoaded(_) => "Users::UsersLoaded",
            Self::SelectUser(_) => "Users::SelectUser",
            Self::DeleteUser(_) => "Users::DeleteUser",
            Self::DeleteResult(_) => "Users::DeleteResult",
            Self::ToggleUserActive(_, _) => "Users::ToggleUserActive",
            Self::ToggleActiveResult(_) => "Users::ToggleActiveResult",
            Self::ShowAddForm => "Users::ShowAddForm",
            Self::ShowEditForm(_) => "Users::ShowEditForm",
            Self::UpdateFormUsername(_) => "Users::UpdateFormUsername",
            Self::UpdateFormDisplayName(_) => "Users::UpdateFormDisplayName",
            Self::UpdateFormEmail(_) => "Users::UpdateFormEmail",
            Self::UpdateFormPassword(_) => "Users::UpdateFormPassword",
            Self::UpdateFormConfirmPassword(_) => {
                "Users::UpdateFormConfirmPassword"
            }
            Self::UpdateFormRole(_) => "Users::UpdateFormRole",
            Self::UpdateFormActive(_) => "Users::UpdateFormActive",
            Self::SubmitForm => "Users::SubmitForm",
            Self::FormResult(_) => "Users::FormResult",
            Self::CancelForm => "Users::CancelForm",
        }
    }
}
