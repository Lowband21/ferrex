use std::{collections::HashMap, fmt, sync::Arc};

use uuid::Uuid;

use crate::{
    database::repository_ports::rbac::RbacRepository,
    domain::users::rbac::{self, PermissionCategory},
    error::Result,
};

pub struct RbacBootstrapService {
    repo: Arc<dyn RbacRepository>,
}

impl fmt::Debug for RbacBootstrapService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RbacBootstrapService")
            .field("repo", &"Arc<dyn RbacRepository>")
            .finish()
    }
}

impl RbacBootstrapService {
    pub fn new(repo: Arc<dyn RbacRepository>) -> Self {
        Self { repo }
    }

    pub async fn ensure_defaults(&self) -> Result<()> {
        let admin_role_id = Uuid::from_u128(1);
        let user_role_id = Uuid::from_u128(2);
        let guest_role_id = Uuid::from_u128(3);

        let roles = [
            (
                admin_role_id,
                rbac::roles::ADMIN,
                "Full system administrator with all permissions",
                true,
            ),
            (
                user_role_id,
                rbac::roles::USER,
                "Standard user with media access",
                true,
            ),
            (
                guest_role_id,
                rbac::roles::GUEST,
                "Limited guest access (no persistent data)",
                true,
            ),
        ];

        for (id, name, description, is_system) in roles {
            self.repo
                .upsert_role(id, name, description, is_system)
                .await?;
        }

        struct PermissionSeed {
            name: &'static str,
            category: PermissionCategory,
            description: &'static str,
        }

        let default_permissions = [
            PermissionSeed {
                name: rbac::permissions::USERS_READ,
                category: PermissionCategory::Users,
                description: "View user profiles and list users",
            },
            PermissionSeed {
                name: rbac::permissions::USERS_CREATE,
                category: PermissionCategory::Users,
                description: "Create new user accounts",
            },
            PermissionSeed {
                name: rbac::permissions::USERS_UPDATE,
                category: PermissionCategory::Users,
                description: "Modify user profiles and settings",
            },
            PermissionSeed {
                name: rbac::permissions::USERS_DELETE,
                category: PermissionCategory::Users,
                description: "Delete user accounts",
            },
            PermissionSeed {
                name: rbac::permissions::USERS_MANAGE_ROLES,
                category: PermissionCategory::Users,
                description: "Assign and remove user roles",
            },
            PermissionSeed {
                name: rbac::permissions::LIBRARIES_READ,
                category: PermissionCategory::Libraries,
                description: "View library information",
            },
            PermissionSeed {
                name: rbac::permissions::LIBRARIES_CREATE,
                category: PermissionCategory::Libraries,
                description: "Create new libraries",
            },
            PermissionSeed {
                name: rbac::permissions::LIBRARIES_UPDATE,
                category: PermissionCategory::Libraries,
                description: "Modify library settings",
            },
            PermissionSeed {
                name: rbac::permissions::LIBRARIES_DELETE,
                category: PermissionCategory::Libraries,
                description: "Delete libraries",
            },
            PermissionSeed {
                name: rbac::permissions::LIBRARIES_SCAN,
                category: PermissionCategory::Libraries,
                description: "Trigger library scans",
            },
            PermissionSeed {
                name: rbac::permissions::MEDIA_READ,
                category: PermissionCategory::Media,
                description: "View media information and browse",
            },
            PermissionSeed {
                name: rbac::permissions::MEDIA_STREAM,
                category: PermissionCategory::Media,
                description: "Stream and playback media",
            },
            PermissionSeed {
                name: rbac::permissions::MEDIA_DOWNLOAD,
                category: PermissionCategory::Media,
                description: "Download media files",
            },
            PermissionSeed {
                name: rbac::permissions::MEDIA_UPDATE,
                category: PermissionCategory::Media,
                description: "Edit media metadata",
            },
            PermissionSeed {
                name: rbac::permissions::MEDIA_DELETE,
                category: PermissionCategory::Media,
                description: "Delete media files",
            },
            PermissionSeed {
                name: rbac::permissions::SERVER_READ_SETTINGS,
                category: PermissionCategory::Server,
                description: "View server configuration",
            },
            PermissionSeed {
                name: rbac::permissions::SERVER_UPDATE_SETTINGS,
                category: PermissionCategory::Server,
                description: "Modify server configuration",
            },
            PermissionSeed {
                name: rbac::permissions::SERVER_READ_LOGS,
                category: PermissionCategory::Server,
                description: "View server logs",
            },
            PermissionSeed {
                name: rbac::permissions::SERVER_MANAGE_TASKS,
                category: PermissionCategory::Server,
                description: "Run maintenance tasks",
            },
            PermissionSeed {
                name: rbac::permissions::SYNC_CREATE,
                category: PermissionCategory::Sync,
                description: "Create synchronized playback sessions",
            },
            PermissionSeed {
                name: rbac::permissions::SYNC_JOIN,
                category: PermissionCategory::Sync,
                description: "Join synchronized playback sessions",
            },
            PermissionSeed {
                name: rbac::permissions::SYNC_MANAGE,
                category: PermissionCategory::Sync,
                description: "Force-end any sync session",
            },
            PermissionSeed {
                name: "server:reset_database",
                category: PermissionCategory::Server,
                description: "Reset the database during development",
            },
            PermissionSeed {
                name: "server:seed_database",
                category: PermissionCategory::Server,
                description: "Seed the database with development fixtures",
            },
        ];

        let mut permission_ids = HashMap::new();
        for perm in default_permissions.iter() {
            let id = self
                .repo
                .upsert_permission(perm.name, perm.category, perm.description)
                .await?;
            permission_ids.insert(perm.name, id);
        }

        for &permission_id in permission_ids.values() {
            self.repo
                .assign_permission_to_role(admin_role_id, permission_id)
                .await?;
        }

        let user_defaults = [
            rbac::permissions::USERS_READ,
            rbac::permissions::LIBRARIES_READ,
            rbac::permissions::MEDIA_READ,
            rbac::permissions::MEDIA_STREAM,
            rbac::permissions::SYNC_CREATE,
            rbac::permissions::SYNC_JOIN,
        ];

        for perm_name in user_defaults.iter() {
            if let Some(&permission_id) = permission_ids.get(perm_name) {
                self.repo
                    .assign_permission_to_role(user_role_id, permission_id)
                    .await?;
            }
        }

        let guest_defaults = [
            rbac::permissions::LIBRARIES_READ,
            rbac::permissions::MEDIA_READ,
            rbac::permissions::MEDIA_STREAM,
        ];

        for perm_name in guest_defaults.iter() {
            if let Some(&permission_id) = permission_ids.get(perm_name) {
                self.repo
                    .assign_permission_to_role(guest_role_id, permission_id)
                    .await?;
            }
        }

        Ok(())
    }
}
