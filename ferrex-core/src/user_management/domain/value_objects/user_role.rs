use serde::{Deserialize, Serialize};

/// User role enumeration for role-based access control
///
/// Defines the different roles a user can have in the system,
/// each with different permissions and capabilities.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default,
)]
pub enum UserRole {
    /// Regular user with basic permissions
    /// - Can view media
    /// - Can manage own watch status
    /// - Can update own profile
    #[default]
    User,

    /// Moderator with enhanced permissions
    /// - All User permissions
    /// - Can manage media library
    /// - Can view user activity logs
    Moderator,

    /// Administrator with full system access
    /// - All Moderator permissions
    /// - Can manage users and roles
    /// - Can access system settings
    /// - Can view system analytics
    Admin,
}

impl UserRole {
    /// Check if this role has at least the specified permission level
    pub fn has_permission_level(&self, required_role: UserRole) -> bool {
        match (self, required_role) {
            (UserRole::Admin, _) => true,
            (UserRole::Moderator, UserRole::User | UserRole::Moderator) => true,
            (UserRole::User, UserRole::User) => true,
            _ => false,
        }
    }

    /// Check if this role can manage other users
    pub fn can_manage_users(&self) -> bool {
        matches!(self, UserRole::Admin | UserRole::Moderator)
    }

    /// Check if this role can access admin features
    pub fn can_access_admin(&self) -> bool {
        matches!(self, UserRole::Admin)
    }

    /// Check if this role can manage the media library
    pub fn can_manage_library(&self) -> bool {
        matches!(self, UserRole::Admin | UserRole::Moderator)
    }

    /// Get all available roles
    pub fn all() -> &'static [UserRole] {
        &[UserRole::User, UserRole::Moderator, UserRole::Admin]
    }

    /// Get the role name as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::User => "user",
            UserRole::Moderator => "moderator",
            UserRole::Admin => "admin",
        }
    }
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserRole::User => write!(f, "User"),
            UserRole::Moderator => write!(f, "Moderator"),
            UserRole::Admin => write!(f, "Administrator"),
        }
    }
}

impl std::str::FromStr for UserRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "user" => Ok(UserRole::User),
            "moderator" => Ok(UserRole::Moderator),
            "admin" | "administrator" => Ok(UserRole::Admin),
            _ => Err(format!("Invalid user role: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_levels() {
        assert!(UserRole::Admin.has_permission_level(UserRole::User));
        assert!(UserRole::Admin.has_permission_level(UserRole::Moderator));
        assert!(UserRole::Admin.has_permission_level(UserRole::Admin));

        assert!(UserRole::Moderator.has_permission_level(UserRole::User));
        assert!(UserRole::Moderator.has_permission_level(UserRole::Moderator));
        assert!(!UserRole::Moderator.has_permission_level(UserRole::Admin));

        assert!(UserRole::User.has_permission_level(UserRole::User));
        assert!(!UserRole::User.has_permission_level(UserRole::Moderator));
        assert!(!UserRole::User.has_permission_level(UserRole::Admin));
    }

    #[test]
    fn role_capabilities() {
        assert!(UserRole::Admin.can_manage_users());
        assert!(UserRole::Admin.can_access_admin());
        assert!(UserRole::Admin.can_manage_library());

        assert!(UserRole::Moderator.can_manage_users());
        assert!(!UserRole::Moderator.can_access_admin());
        assert!(UserRole::Moderator.can_manage_library());

        assert!(!UserRole::User.can_manage_users());
        assert!(!UserRole::User.can_access_admin());
        assert!(!UserRole::User.can_manage_library());
    }

    #[test]
    fn string_conversion() {
        assert_eq!(UserRole::User.as_str(), "user");
        assert_eq!(UserRole::Moderator.as_str(), "moderator");
        assert_eq!(UserRole::Admin.as_str(), "admin");

        assert_eq!("user".parse::<UserRole>().unwrap(), UserRole::User);
        assert_eq!(
            "moderator".parse::<UserRole>().unwrap(),
            UserRole::Moderator
        );
        assert_eq!("admin".parse::<UserRole>().unwrap(), UserRole::Admin);
        assert_eq!(
            "administrator".parse::<UserRole>().unwrap(),
            UserRole::Admin
        );
    }
}
