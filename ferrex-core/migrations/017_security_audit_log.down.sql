-- Rollback migration for security audit log and enhanced security features

-- Drop triggers
DROP TRIGGER IF EXISTS update_rate_limit_state_updated_at ON rate_limit_state;
DROP TRIGGER IF EXISTS update_refresh_tokens_updated_at ON refresh_tokens;

-- Drop the update function
DROP FUNCTION IF EXISTS update_updated_at_column();

-- Drop tables (only the new ones we created)
DROP TABLE IF EXISTS rate_limit_state;
DROP TABLE IF EXISTS security_audit_log;

-- Remove columns added to refresh_tokens (keep the original table)
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS id;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS device_session_id;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS token_hash;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS family_id;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS generation;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS used_at;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS used_count;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS revoked;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS revoked_at;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS revoked_reason;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS metadata;
ALTER TABLE refresh_tokens DROP COLUMN IF EXISTS updated_at;

-- Drop indexes we created
DROP INDEX IF EXISTS idx_refresh_tokens_device_session;
DROP INDEX IF EXISTS idx_refresh_tokens_token_hash;
DROP INDEX IF EXISTS idx_refresh_tokens_family_id;
DROP INDEX IF EXISTS idx_refresh_tokens_active;

-- Remove added columns from auth_device_sessions
ALTER TABLE auth_device_sessions DROP COLUMN IF EXISTS trusted_until;
ALTER TABLE auth_device_sessions DROP COLUMN IF EXISTS trust_extended_count;
ALTER TABLE auth_device_sessions DROP COLUMN IF EXISTS revoked_at;
ALTER TABLE auth_device_sessions DROP COLUMN IF EXISTS revoked_reason;

-- Drop indexes that were created
DROP INDEX IF EXISTS idx_auth_device_sessions_trusted_until;