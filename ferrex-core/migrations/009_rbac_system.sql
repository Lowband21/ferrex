-- Role-Based Access Control (RBAC) System
-- This migration creates a flexible permission system to replace the simple is_admin flag

-- Create roles table
CREATE TABLE roles (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(50) UNIQUE NOT NULL,
    description TEXT,
    is_system BOOLEAN DEFAULT FALSE, -- System roles cannot be deleted
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Create permissions table
CREATE TABLE permissions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(100) UNIQUE NOT NULL,
    category VARCHAR(50) NOT NULL,
    description TEXT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Role permissions mapping
CREATE TABLE role_permissions (
    role_id UUID REFERENCES roles(id) ON DELETE CASCADE,
    permission_id UUID REFERENCES permissions(id) ON DELETE CASCADE,
    granted_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    PRIMARY KEY (role_id, permission_id)
);

-- User roles mapping
CREATE TABLE user_roles (
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID REFERENCES roles(id) ON DELETE CASCADE,
    granted_by UUID REFERENCES users(id),
    granted_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    PRIMARY KEY (user_id, role_id)
);

-- Optional: Per-user permission overrides
CREATE TABLE user_permissions (
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    permission_id UUID REFERENCES permissions(id) ON DELETE CASCADE,
    granted BOOLEAN NOT NULL DEFAULT TRUE, -- Can be used to explicitly deny permissions
    granted_by UUID REFERENCES users(id),
    granted_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    reason TEXT, -- Optional reason for override
    PRIMARY KEY (user_id, permission_id)
);

-- Create indexes for performance
CREATE INDEX idx_role_permissions_role ON role_permissions(role_id);
CREATE INDEX idx_role_permissions_permission ON role_permissions(permission_id);
CREATE INDEX idx_user_roles_user ON user_roles(user_id);
CREATE INDEX idx_user_roles_role ON user_roles(role_id);
CREATE INDEX idx_user_permissions_user ON user_permissions(user_id);
CREATE INDEX idx_permissions_category ON permissions(category);
CREATE INDEX idx_permissions_name ON permissions(name);

-- Add comments for documentation
COMMENT ON TABLE roles IS 'System and custom roles for access control';
COMMENT ON TABLE permissions IS 'Granular permissions that can be assigned to roles';
COMMENT ON TABLE role_permissions IS 'Maps permissions to roles';
COMMENT ON TABLE user_roles IS 'Assigns roles to users';
COMMENT ON TABLE user_permissions IS 'Per-user permission overrides (optional)';

-- Insert default roles
INSERT INTO roles (id, name, description, is_system) VALUES
    ('00000000-0000-0000-0000-000000000001', 'admin', 'Full system administrator with all permissions', true),
    ('00000000-0000-0000-0000-000000000002', 'user', 'Standard user with media access', true),
    ('00000000-0000-0000-0000-000000000003', 'guest', 'Limited guest access (no persistent data)', true);

-- Insert initial permissions
-- User Management
INSERT INTO permissions (name, category, description) VALUES
    ('users:read', 'users', 'View user profiles and list users'),
    ('users:create', 'users', 'Create new user accounts'),
    ('users:update', 'users', 'Modify user profiles and settings'),
    ('users:delete', 'users', 'Delete user accounts'),
    ('users:manage_roles', 'users', 'Assign and remove user roles');

-- Library Management
INSERT INTO permissions (name, category, description) VALUES
    ('libraries:read', 'libraries', 'View library information'),
    ('libraries:create', 'libraries', 'Create new libraries'),
    ('libraries:update', 'libraries', 'Modify library settings'),
    ('libraries:delete', 'libraries', 'Delete libraries'),
    ('libraries:scan', 'libraries', 'Trigger library scans');

-- Media Access
INSERT INTO permissions (name, category, description) VALUES
    ('media:read', 'media', 'View media information and browse'),
    ('media:stream', 'media', 'Stream and playback media'),
    ('media:download', 'media', 'Download media files'),
    ('media:update', 'media', 'Edit media metadata'),
    ('media:delete', 'media', 'Delete media files');

-- Server Management
INSERT INTO permissions (name, category, description) VALUES
    ('server:read_settings', 'server', 'View server configuration'),
    ('server:update_settings', 'server', 'Modify server configuration'),
    ('server:read_logs', 'server', 'View server logs'),
    ('server:manage_tasks', 'server', 'Run maintenance tasks');

-- Sync Sessions
INSERT INTO permissions (name, category, description) VALUES
    ('sync:create', 'sync', 'Create synchronized playback sessions'),
    ('sync:join', 'sync', 'Join synchronized playback sessions'),
    ('sync:manage', 'sync', 'Force-end any sync session');

-- Assign all permissions to admin role
INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000001', id FROM permissions;

-- Assign basic permissions to user role
INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000002', id FROM permissions
WHERE name IN (
    'users:read',       -- Can view own profile
    'libraries:read',   -- Can browse libraries
    'media:read',       -- Can view media
    'media:stream',     -- Can play media
    'sync:create',      -- Can create sync sessions
    'sync:join'         -- Can join sync sessions
);

-- Assign minimal permissions to guest role
INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000003', id FROM permissions
WHERE name IN (
    'libraries:read',   -- Can browse libraries
    'media:read',       -- Can view media
    'media:stream'      -- Can play media
);

-- Create admin_actions audit table for tracking admin actions
CREATE TABLE admin_actions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    admin_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    action_type VARCHAR(100) NOT NULL,
    target_type VARCHAR(50), -- 'user', 'library', 'media', etc.
    target_id UUID,
    description TEXT,
    metadata JSONB,
    ip_address INET,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Create indexes for admin actions
CREATE INDEX idx_admin_actions_admin ON admin_actions(admin_id);
CREATE INDEX idx_admin_actions_type ON admin_actions(action_type);
CREATE INDEX idx_admin_actions_target ON admin_actions(target_type, target_id);
CREATE INDEX idx_admin_actions_created ON admin_actions(created_at DESC);

-- Add comment to document the table
COMMENT ON TABLE admin_actions IS 'Audit log for administrative actions';

-- Note: Users will need to be assigned roles after setup
-- The first user should be assigned the admin role during initial setup