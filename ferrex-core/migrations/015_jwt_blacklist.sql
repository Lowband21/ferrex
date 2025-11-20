-- Create table for JWT token revocation
CREATE TABLE jwt_blacklist (
    jti VARCHAR(255) PRIMARY KEY,
    user_id UUID NOT NULL,
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_reason VARCHAR(255)
);

-- Index for cleanup queries
CREATE INDEX idx_jwt_blacklist_expires_at ON jwt_blacklist(expires_at);

-- Index for user lookups
CREATE INDEX idx_jwt_blacklist_user_id ON jwt_blacklist(user_id);