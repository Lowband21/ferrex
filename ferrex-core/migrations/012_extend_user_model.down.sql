-- Revert user schema updates

-- Drop triggers
DROP TRIGGER IF EXISTS update_user_credentials_updated_at ON user_credentials;
DROP TRIGGER IF EXISTS update_users_updated_at ON users;

-- Drop the timestamp trigger function
DROP FUNCTION IF EXISTS update_updated_at_timestamp();

-- Re-add password_hash column to users table
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_hash VARCHAR(255);

-- Copy password hashes back from user_credentials
UPDATE users u
SET password_hash = uc.password_hash
FROM user_credentials uc
WHERE u.id = uc.user_id;

-- Drop user_credentials table
DROP TABLE IF EXISTS user_credentials;

-- Drop indexes
DROP INDEX IF EXISTS idx_users_is_active;
DROP INDEX IF EXISTS idx_users_last_login;
DROP INDEX IF EXISTS idx_users_username_lower;
DROP INDEX IF EXISTS idx_users_email_unique;

-- Remove added columns
ALTER TABLE users 
    DROP COLUMN IF EXISTS preferences,
    DROP COLUMN IF EXISTS email,
    DROP COLUMN IF EXISTS is_active,
    DROP COLUMN IF EXISTS last_login,
    DROP COLUMN IF EXISTS avatar_url;

-- Convert timestamp columns back to BIGINT
ALTER TABLE users 
    ALTER COLUMN created_at TYPE BIGINT 
        USING EXTRACT(EPOCH FROM created_at)::BIGINT * 1000,
    ALTER COLUMN updated_at TYPE BIGINT 
        USING EXTRACT(EPOCH FROM updated_at)::BIGINT * 1000;

-- Set defaults back to BIGINT epoch
ALTER TABLE users
    ALTER COLUMN created_at SET DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT * 1000,
    ALTER COLUMN updated_at SET DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT * 1000;

-- Recreate the original BIGINT trigger function
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = EXTRACT(EPOCH FROM NOW())::BIGINT * 1000;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Recreate trigger for users table
CREATE TRIGGER update_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();