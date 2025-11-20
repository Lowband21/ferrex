-- Unified device session management for domain-driven authentication
-- This table consolidates device trust, user sessions, and PIN management
-- Based on the DeviceSession aggregate from ferrex-core/src/auth/domain/aggregates/device_session.rs

CREATE TABLE auth_device_sessions (
    -- Primary identifier for the device session
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    
    -- User this session belongs to
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- Device fingerprint hash (SHA256, 64 characters)
    device_fingerprint TEXT NOT NULL,
    
    -- Human-readable device name
    device_name TEXT NOT NULL,
    
    -- Device trust status: pending, trusted, revoked
    status TEXT NOT NULL CHECK (status IN ('pending', 'trusted', 'revoked')),
    
    -- Argon2id hash of the device PIN (optional)
    pin_hash TEXT,
    
    -- Current session token (base64 URL-safe encoded, optional)
    session_token TEXT,
    
    -- Session token creation timestamp
    session_token_created_at TIMESTAMPTZ,
    
    -- Session token expiration timestamp
    session_token_expires_at TIMESTAMPTZ,
    
    -- Failed PIN attempts counter
    failed_attempts SMALLINT NOT NULL DEFAULT 0,
    
    -- When the device was first registered
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Last activity timestamp
    last_activity TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Audit timestamps
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Performance indexes
-- Index for finding active sessions by device fingerprint
CREATE INDEX idx_auth_device_sessions_fingerprint_active 
ON auth_device_sessions(device_fingerprint) 
WHERE status IN ('pending', 'trusted');

-- Index for user's active device sessions
CREATE INDEX idx_auth_device_sessions_user_active 
ON auth_device_sessions(user_id, status) 
WHERE status IN ('pending', 'trusted');

-- Index for session token lookups (partial index for active tokens only)
-- Note: Cannot use NOW() in partial index as it's not IMMUTABLE
-- The expiration check will be done in queries instead
CREATE UNIQUE INDEX idx_auth_device_sessions_token_active 
ON auth_device_sessions(session_token) 
WHERE session_token IS NOT NULL 
  AND status = 'trusted';

-- Index for session token expiration cleanup
CREATE INDEX idx_auth_device_sessions_token_expires 
ON auth_device_sessions(session_token_expires_at) 
WHERE session_token IS NOT NULL;

-- Index for activity-based queries and cleanup
CREATE INDEX idx_auth_device_sessions_last_activity 
ON auth_device_sessions(last_activity DESC);

-- Composite index for user device lookups
CREATE INDEX idx_auth_device_sessions_user_device 
ON auth_device_sessions(user_id, device_fingerprint);

-- Foreign key constraints
-- User reference is already handled by the REFERENCES clause above

-- Unique constraint to prevent duplicate device sessions per user
-- A user can only have one session per device fingerprint
CREATE UNIQUE INDEX idx_auth_device_sessions_user_fingerprint_unique 
ON auth_device_sessions(user_id, device_fingerprint) 
WHERE status IN ('pending', 'trusted');

-- Trigger to automatically update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_auth_device_sessions_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_auth_device_sessions_updated_at
    BEFORE UPDATE ON auth_device_sessions
    FOR EACH ROW
    EXECUTE FUNCTION update_auth_device_sessions_updated_at();

-- Comments for documentation
COMMENT ON TABLE auth_device_sessions IS 
'Unified device session management combining device trust, user sessions, and PIN authentication';

COMMENT ON COLUMN auth_device_sessions.device_fingerprint IS 
'SHA256 hash of device hardware characteristics (64 hex characters)';

COMMENT ON COLUMN auth_device_sessions.status IS 
'Device trust status: pending (needs PIN setup), trusted (can authenticate), revoked (blocked)';

COMMENT ON COLUMN auth_device_sessions.pin_hash IS 
'Argon2id hash of the device-specific PIN for quick user authentication';

COMMENT ON COLUMN auth_device_sessions.session_token IS 
'Current active session token (base64 URL-safe encoded, 43 characters)';

COMMENT ON COLUMN auth_device_sessions.failed_attempts IS 
'Counter for failed PIN authentication attempts, reset on successful auth';

COMMENT ON COLUMN auth_device_sessions.last_activity IS 
'Timestamp of last session activity, updated on token refresh and API calls';