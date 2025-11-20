-- Rollback unified device session management
-- This removes the auth_device_sessions table and associated objects

-- Drop the trigger first
DROP TRIGGER IF EXISTS trg_auth_device_sessions_updated_at ON auth_device_sessions;

-- Drop the trigger function
DROP FUNCTION IF EXISTS update_auth_device_sessions_updated_at();

-- Drop all indexes (they will be dropped automatically with the table, but being explicit)
DROP INDEX IF EXISTS idx_auth_device_sessions_fingerprint_active;
DROP INDEX IF EXISTS idx_auth_device_sessions_user_active;
DROP INDEX IF EXISTS idx_auth_device_sessions_token_active;
DROP INDEX IF EXISTS idx_auth_device_sessions_token_expires;
DROP INDEX IF EXISTS idx_auth_device_sessions_last_activity;
DROP INDEX IF EXISTS idx_auth_device_sessions_user_device;
DROP INDEX IF EXISTS idx_auth_device_sessions_user_fingerprint_unique;

-- Drop the main table
DROP TABLE IF EXISTS auth_device_sessions;