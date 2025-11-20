-- Drop additional performance indexes created in this migration

DROP INDEX IF EXISTS idx_user_credentials_updated;
DROP INDEX IF EXISTS idx_users_email_lower;
DROP INDEX IF EXISTS idx_users_preferences_auto_login;