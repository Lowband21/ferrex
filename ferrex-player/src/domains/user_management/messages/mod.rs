use ferrex_core::{rbac::UserPermissions, user::User};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum Message {
    // User CRUD operations
    LoadUsers,
    UsersLoaded(Result<Vec<User>, String>),

    // User selection
    SelectUser(Uuid),
    UserSelected(User),

    // User creation
    CreateUser,
    CreateUserFormUpdateUsername(String),
    CreateUserFormUpdateDisplayName(String),
    CreateUserFormUpdatePassword(String),
    CreateUserFormUpdateConfirmPassword(String),
    CreateUserFormTogglePasswordVisibility,
    CreateUserFormSubmit,
    CreateUserSuccess(User),
    CreateUserError(String),
    CreateUserCancel,

    // User updates
    UpdateUser(Uuid),
    UpdateUserFormUpdateUsername(String),
    UpdateUserFormUpdateDisplayName(String),
    UpdateUserFormUpdatePassword(String),
    UpdateUserFormUpdateConfirmPassword(String),
    UpdateUserFormTogglePasswordVisibility,
    UpdateUserFormSubmit,
    UpdateUserSuccess(User),
    UpdateUserError(String),
    UpdateUserCancel,

    // User deletion
    DeleteUser(Uuid),
    DeleteUserConfirm(Uuid),
    DeleteUserSuccess(Uuid),
    DeleteUserError(String),
    DeleteUserCancel,

    // First-run user creation (moved from auth)
    FirstRunCreateUser,
    FirstRunUpdateUsername(String),
    FirstRunUpdateDisplayName(String),
    FirstRunUpdatePassword(String),
    FirstRunUpdateConfirmPassword(String),
    FirstRunTogglePasswordVisibility,
    FirstRunSubmit,
    FirstRunSuccess(User),
    FirstRunError(String),

    // Navigation
    ShowUserList,
    BackToUserList,
}

impl Message {
    /// Returns a sanitized display string that hides sensitive credential data
    pub fn sanitized_display(&self) -> String {
        match self {
            // Sensitive credential messages - hide the actual values
            Self::CreateUserFormUpdatePassword(_) => {
                "CreateUserFormUpdatePassword(***)".to_string()
            }
            Self::CreateUserFormUpdateConfirmPassword(_) => {
                "CreateUserFormUpdateConfirmPassword(***)".to_string()
            }
            Self::UpdateUserFormUpdatePassword(_) => {
                "UpdateUserFormUpdatePassword(***)".to_string()
            }
            Self::UpdateUserFormUpdateConfirmPassword(_) => {
                "UpdateUserFormUpdateConfirmPassword(***)".to_string()
            }
            Self::FirstRunUpdatePassword(_) => "FirstRunUpdatePassword(***)".to_string(),
            Self::FirstRunUpdateConfirmPassword(_) => {
                "FirstRunUpdateConfirmPassword(***)".to_string()
            }

            // Non-sensitive messages - show full debug representation
            _ => format!("{:?}", self),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            // User CRUD operations
            Self::LoadUsers => "UserManagement::LoadUsers",
            Self::UsersLoaded(_) => "UserManagement::UsersLoaded",

            // User selection
            Self::SelectUser(_) => "UserManagement::SelectUser",
            Self::UserSelected(_) => "UserManagement::UserSelected",

            // User creation
            Self::CreateUser => "UserManagement::CreateUser",
            Self::CreateUserFormUpdateUsername(_) => "UserManagement::CreateUserFormUpdateUsername",
            Self::CreateUserFormUpdateDisplayName(_) => {
                "UserManagement::CreateUserFormUpdateDisplayName"
            }
            Self::CreateUserFormUpdatePassword(_) => "UserManagement::CreateUserFormUpdatePassword",
            Self::CreateUserFormUpdateConfirmPassword(_) => {
                "UserManagement::CreateUserFormUpdateConfirmPassword"
            }
            Self::CreateUserFormTogglePasswordVisibility => {
                "UserManagement::CreateUserFormTogglePasswordVisibility"
            }
            Self::CreateUserFormSubmit => "UserManagement::CreateUserFormSubmit",
            Self::CreateUserSuccess(_) => "UserManagement::CreateUserSuccess",
            Self::CreateUserError(_) => "UserManagement::CreateUserError",
            Self::CreateUserCancel => "UserManagement::CreateUserCancel",

            // User updates
            Self::UpdateUser(_) => "UserManagement::UpdateUser",
            Self::UpdateUserFormUpdateUsername(_) => "UserManagement::UpdateUserFormUpdateUsername",
            Self::UpdateUserFormUpdateDisplayName(_) => {
                "UserManagement::UpdateUserFormUpdateDisplayName"
            }
            Self::UpdateUserFormUpdatePassword(_) => "UserManagement::UpdateUserFormUpdatePassword",
            Self::UpdateUserFormUpdateConfirmPassword(_) => {
                "UserManagement::UpdateUserFormUpdateConfirmPassword"
            }
            Self::UpdateUserFormTogglePasswordVisibility => {
                "UserManagement::UpdateUserFormTogglePasswordVisibility"
            }
            Self::UpdateUserFormSubmit => "UserManagement::UpdateUserFormSubmit",
            Self::UpdateUserSuccess(_) => "UserManagement::UpdateUserSuccess",
            Self::UpdateUserError(_) => "UserManagement::UpdateUserError",
            Self::UpdateUserCancel => "UserManagement::UpdateUserCancel",

            // User deletion
            Self::DeleteUser(_) => "UserManagement::DeleteUser",
            Self::DeleteUserConfirm(_) => "UserManagement::DeleteUserConfirm",
            Self::DeleteUserSuccess(_) => "UserManagement::DeleteUserSuccess",
            Self::DeleteUserError(_) => "UserManagement::DeleteUserError",
            Self::DeleteUserCancel => "UserManagement::DeleteUserCancel",

            // First-run user creation
            Self::FirstRunCreateUser => "UserManagement::FirstRunCreateUser",
            Self::FirstRunUpdateUsername(_) => "UserManagement::FirstRunUpdateUsername",
            Self::FirstRunUpdateDisplayName(_) => "UserManagement::FirstRunUpdateDisplayName",
            Self::FirstRunUpdatePassword(_) => "UserManagement::FirstRunUpdatePassword",
            Self::FirstRunUpdateConfirmPassword(_) => {
                "UserManagement::FirstRunUpdateConfirmPassword"
            }
            Self::FirstRunTogglePasswordVisibility => {
                "UserManagement::FirstRunTogglePasswordVisibility"
            }
            Self::FirstRunSubmit => "UserManagement::FirstRunSubmit",
            Self::FirstRunSuccess(_) => "UserManagement::FirstRunSuccess",
            Self::FirstRunError(_) => "UserManagement::FirstRunError",

            // Navigation
            Self::ShowUserList => "UserManagement::ShowUserList",
            Self::BackToUserList => "UserManagement::BackToUserList",
        }
    }
}

/// Cross-domain events that user management domain can emit
#[derive(Clone, Debug)]
pub enum UserManagementEvent {
    UserCreated(User),
    UserUpdated(User),
    UserDeleted(Uuid),
    UsersListChanged,
    UserSelected(User),
}
