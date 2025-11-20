-- Fix user timestamp columns to use proper TIMESTAMP WITH TIME ZONE
-- First, drop the existing trigger that expects BIGINT
DROP TRIGGER IF EXISTS update_users_updated_at ON users;

-- Drop existing defaults before type conversion
ALTER TABLE users
    ALTER COLUMN created_at DROP DEFAULT,
    ALTER COLUMN updated_at DROP DEFAULT;

-- Convert existing BIGINT columns to TIMESTAMP WITH TIME ZONE
-- For created_at and updated_at that already exist
ALTER TABLE users 
    ALTER COLUMN created_at TYPE TIMESTAMP WITH TIME ZONE 
        USING to_timestamp(created_at / 1000.0) AT TIME ZONE 'UTC',
    ALTER COLUMN updated_at TYPE TIMESTAMP WITH TIME ZONE 
        USING to_timestamp(updated_at / 1000.0) AT TIME ZONE 'UTC';

-- Set new defaults for the timestamp columns
ALTER TABLE users
    ALTER COLUMN created_at SET DEFAULT NOW(),
    ALTER COLUMN updated_at SET DEFAULT NOW();

-- Extend user model with additional fields
ALTER TABLE users 
    ADD COLUMN IF NOT EXISTS avatar_url VARCHAR(255),
    ADD COLUMN IF NOT EXISTS last_login TIMESTAMP WITH TIME ZONE,
    ADD COLUMN IF NOT EXISTS is_active BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS email VARCHAR(255),
    ADD COLUMN IF NOT EXISTS preferences JSONB NOT NULL DEFAULT '{}';

-- Add unique constraint on email if provided
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email_unique ON users(email) WHERE email IS NOT NULL;

-- Create separate table for password credentials (security best practice)
CREATE TABLE IF NOT EXISTS user_credentials (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    password_hash VARCHAR(255) NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Create index on user_credentials for performance
CREATE INDEX IF NOT EXISTS idx_user_credentials_user_id ON user_credentials(user_id);

-- Migrate existing password hashes to the new table
INSERT INTO user_credentials (user_id, password_hash)
SELECT id, password_hash FROM users
WHERE password_hash IS NOT NULL
ON CONFLICT (user_id) DO NOTHING;

-- Drop password_hash from users table after migration
ALTER TABLE users DROP COLUMN IF EXISTS password_hash;

-- Create proper updated_at trigger function for timestamps
CREATE OR REPLACE FUNCTION update_updated_at_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger for users table with the new function
CREATE TRIGGER update_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_timestamp();

-- Create trigger for user_credentials table
CREATE TRIGGER update_user_credentials_updated_at
    BEFORE UPDATE ON user_credentials
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_timestamp();

-- Add performance indexes
CREATE INDEX IF NOT EXISTS idx_users_username_lower ON users(LOWER(username));
CREATE INDEX IF NOT EXISTS idx_users_last_login ON users(last_login) WHERE is_active = true AND last_login IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_users_is_active ON users(is_active) WHERE is_active = true;