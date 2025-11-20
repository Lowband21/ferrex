use ferrex_core::api_types::users_admin::AdminUserInfo;
use ferrex_core::player_prelude::User;
use uuid::Uuid;

#[derive(Clone)]
pub enum Message {
    // User CRUD operations
    LoadUsers,
    UsersLoaded(Result<Vec<AdminUserInfo>, String>),

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
            Self::FirstRunUpdatePassword(_) => {
                "FirstRunUpdatePassword(***)".to_string()
            }
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
            Self::CreateUserFormUpdateUsername(_) => {
                "UserManagement::CreateUserFormUpdateUsername"
            }
            Self::CreateUserFormUpdateDisplayName(_) => {
                "UserManagement::CreateUserFormUpdateDisplayName"
            }
            Self::CreateUserFormUpdatePassword(_) => {
                "UserManagement::CreateUserFormUpdatePassword"
            }
            Self::CreateUserFormUpdateConfirmPassword(_) => {
                "UserManagement::CreateUserFormUpdateConfirmPassword"
            }
            Self::CreateUserFormTogglePasswordVisibility => {
                "UserManagement::CreateUserFormTogglePasswordVisibility"
            }
            Self::CreateUserFormSubmit => {
                "UserManagement::CreateUserFormSubmit"
            }
            Self::CreateUserSuccess(_) => "UserManagement::CreateUserSuccess",
            Self::CreateUserError(_) => "UserManagement::CreateUserError",
            Self::CreateUserCancel => "UserManagement::CreateUserCancel",

            // User updates
            Self::UpdateUser(_) => "UserManagement::UpdateUser",
            Self::UpdateUserFormUpdateUsername(_) => {
                "UserManagement::UpdateUserFormUpdateUsername"
            }
            Self::UpdateUserFormUpdateDisplayName(_) => {
                "UserManagement::UpdateUserFormUpdateDisplayName"
            }
            Self::UpdateUserFormUpdatePassword(_) => {
                "UserManagement::UpdateUserFormUpdatePassword"
            }
            Self::UpdateUserFormUpdateConfirmPassword(_) => {
                "UserManagement::UpdateUserFormUpdateConfirmPassword"
            }
            Self::UpdateUserFormTogglePasswordVisibility => {
                "UserManagement::UpdateUserFormTogglePasswordVisibility"
            }
            Self::UpdateUserFormSubmit => {
                "UserManagement::UpdateUserFormSubmit"
            }
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
            Self::FirstRunUpdateUsername(_) => {
                "UserManagement::FirstRunUpdateUsername"
            }
            Self::FirstRunUpdateDisplayName(_) => {
                "UserManagement::FirstRunUpdateDisplayName"
            }
            Self::FirstRunUpdatePassword(_) => {
                "UserManagement::FirstRunUpdatePassword"
            }
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

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::LoadUsers => {
                write!(f, "UserManagement::LoadUsers")
            }
            Message::UsersLoaded(users) => {
                write!(f, "UserManagement::UsersLoaded({:#?})", users)
            }
            Message::SelectUser(uuid) => {
                write!(f, "UserManagement::SelectUser({})", uuid)
            }
            Message::UserSelected(user) => {
                write!(f, "UserManagement::UserSelected({})", user.id)
            }
            Message::CreateUser => {
                write!(f, "UserManagement::CreateUser")
            }
            Message::CreateUserFormUpdateUsername(_) => {
                write!(f, "UserManagement::CreateUserFormUpdateUsername")
            }
            Message::CreateUserFormUpdateDisplayName(_) => {
                write!(f, "UserManagement::CreateUserFormUpdateDisplayName")
            }
            Message::CreateUserFormUpdatePassword(_) => {
                write!(f, "UserManagement::CreateUserFormUpdatePassword")
            }
            Message::CreateUserFormUpdateConfirmPassword(_) => {
                write!(f, "UserManagement::CreateUserFormUpdateConfirmPassword")
            }
            Message::CreateUserFormTogglePasswordVisibility => {
                write!(
                    f,
                    "UserManagement::CreateUserFormTogglePasswordVisibility"
                )
            }
            Message::CreateUserFormSubmit => {
                write!(f, "UserManagement::CreateUserFormSubmit")
            }
            Message::CreateUserSuccess(user) => {
                write!(f, "UserManagement::CreateUserSuccess({})", user.id)
            }
            Message::CreateUserError(_) => {
                write!(f, "UserManagement::CreateUserError")
            }
            Message::CreateUserCancel => {
                write!(f, "UserManagement::CreateUserCancel")
            }
            Message::UpdateUser(uuid) => {
                write!(f, "UserManagement::UpdateUser({})", uuid)
            }
            Message::UpdateUserFormUpdateUsername(_) => {
                write!(f, "UserManagement::UpdateUserFormUpdateUsername")
            }
            Message::UpdateUserFormUpdateDisplayName(_) => {
                write!(f, "UserManagement::UpdateUserFormUpdateDisplayName")
            }
            Message::UpdateUserFormUpdatePassword(_) => {
                write!(f, "UserManagement::UpdateUserFormUpdatePassword")
            }
            Message::UpdateUserFormUpdateConfirmPassword(_) => {
                write!(f, "UserManagement::UpdateUserFormUpdateConfirmPassword")
            }
            Message::UpdateUserFormTogglePasswordVisibility => {
                write!(
                    f,
                    "UserManagement::UpdateUserFormTogglePasswordVisibility"
                )
            }
            Message::UpdateUserFormSubmit => {
                write!(f, "UserManagement::UpdateUserFormSubmit")
            }
            Message::UpdateUserSuccess(user) => {
                write!(f, "UserManagement::UpdateUserSuccess({})", user.id)
            }
            Message::UpdateUserError(_) => {
                write!(f, "UserManagement::UpdateUserError")
            }
            Message::UpdateUserCancel => {
                write!(f, "UserManagement::UpdateUserCancel")
            }
            Message::DeleteUser(uuid) => {
                write!(f, "UserManagement::DeleteUser({})", uuid)
            }
            Message::DeleteUserConfirm(uuid) => {
                write!(f, "UserManagement::DeleteUserConfirm({})", uuid)
            }
            Message::DeleteUserSuccess(uuid) => {
                write!(f, "UserManagement::DeleteUserSuccess({})", uuid)
            }
            Message::DeleteUserError(_) => {
                write!(f, "UserManagement::DeleteUserError")
            }
            Message::DeleteUserCancel => {
                write!(f, "UserManagement::DeleteUserCancel")
            }
            Message::FirstRunCreateUser => {
                write!(f, "UserManagement::FirstRunCreateUser")
            }
            Message::FirstRunUpdateUsername(_) => {
                write!(f, "UserManagement::FirstRunUpdateUsername")
            }
            Message::FirstRunUpdateDisplayName(_) => {
                write!(f, "UserManagement::FirstRunUpdateDisplayName")
            }
            Message::FirstRunUpdatePassword(_) => {
                write!(f, "UserManagement::FirstRunUpdatePassword")
            }
            Message::FirstRunUpdateConfirmPassword(_) => {
                write!(f, "UserManagement::FirstRunUpdateConfirmPassword")
            }
            Message::FirstRunTogglePasswordVisibility => {
                write!(f, "UserManagement::FirstRunTogglePasswordVisibility")
            }
            Message::FirstRunSubmit => {
                write!(f, "UserManagement::FirstRunSubmit")
            }
            Message::FirstRunSuccess(user) => {
                write!(f, "UserManagement::FirstRunSuccess({})", user.id)
            }
            Message::FirstRunError(_) => {
                write!(f, "UserManagement::FirstRunError")
            }
            Message::ShowUserList => {
                write!(f, "UserManagement::ShowUserList")
            }
            Message::BackToUserList => {
                write!(f, "UserManagement::BackToUserList")
            }
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
