-- Additional performance indexes for user management tables
-- Note: Some indexes are already created in migration 012

-- Index for tracking password update times
CREATE INDEX IF NOT EXISTS idx_user_credentials_updated ON user_credentials(updated_at DESC);

-- Index for email lookups when not null (case-insensitive)
CREATE INDEX IF NOT EXISTS idx_users_email_lower ON users(LOWER(email)) WHERE email IS NOT NULL;

-- Partial index for preferences queries (for future use)
CREATE INDEX IF NOT EXISTS idx_users_preferences_auto_login 
ON users((preferences->>'auto_login_enabled')) 
WHERE (preferences->>'auto_login_enabled')::boolean = true;