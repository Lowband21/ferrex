-- Security audit logging and enhanced security features
-- This migration adds comprehensive security event tracking and rate limiting support

-- Security audit log for tracking all security-related events
CREATE TABLE IF NOT EXISTS security_audit_log (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    
    -- User associated with the event (may be null for anonymous events)
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    
    -- Device session associated with the event
    device_session_id UUID REFERENCES auth_device_sessions(id) ON DELETE SET NULL,
    
    -- Event type enumeration
    event_type TEXT NOT NULL CHECK (event_type IN (
        -- Authentication events
        'login_success',
        'login_failed',
        'logout',
        'session_expired',
        'session_revoked',
        
        -- Device trust events
        'device_registered',
        'device_trusted',
        'device_trust_revoked',
        'device_trust_expired',
        'device_removed',
        
        -- PIN authentication events
        'pin_set',
        'pin_changed',
        'pin_auth_success',
        'pin_auth_failed',
        'pin_lockout',
        
        -- Token events
        'token_refreshed',
        'token_revoked',
        'refresh_token_expired',
        
        -- Rate limiting events
        'rate_limit_exceeded',
        'suspicious_activity',
        
        -- User management events
        'user_created',
        'user_updated',
        'user_deleted',
        'password_changed',
        'role_changed',
        
        -- Security configuration events
        'security_settings_changed',
        'permissions_changed'
    )),
    
    -- Event severity level
    severity TEXT NOT NULL DEFAULT 'info' CHECK (severity IN ('debug', 'info', 'warning', 'error', 'critical')),
    
    -- JSON data containing event-specific details
    event_data JSONB,
    
    -- Network information
    ip_address INET,
    user_agent TEXT,
    
    -- Request identifier for correlation
    request_id UUID,
    
    -- Result of the event
    success BOOLEAN NOT NULL DEFAULT true,
    
    -- Error message if event failed
    error_message TEXT,
    
    -- Timestamp of the event
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for efficient querying
CREATE INDEX idx_security_audit_user_id ON security_audit_log(user_id) WHERE user_id IS NOT NULL;
CREATE INDEX idx_security_audit_device_session ON security_audit_log(device_session_id) WHERE device_session_id IS NOT NULL;
CREATE INDEX idx_security_audit_event_type ON security_audit_log(event_type);
CREATE INDEX idx_security_audit_severity ON security_audit_log(severity) WHERE severity IN ('warning', 'error', 'critical');
CREATE INDEX idx_security_audit_created_at ON security_audit_log(created_at DESC);
CREATE INDEX idx_security_audit_ip_address ON security_audit_log(ip_address) WHERE ip_address IS NOT NULL;

-- Composite index for common queries
CREATE INDEX idx_security_audit_user_event_time ON security_audit_log(user_id, event_type, created_at DESC) 
WHERE user_id IS NOT NULL;

-- Rate limiting state table (for persistent rate limiting)
CREATE TABLE IF NOT EXISTS rate_limit_state (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    
    -- Key identifying the rate-limited resource (user_id, ip, etc.)
    key TEXT NOT NULL,
    
    -- Endpoint or resource being rate limited
    endpoint TEXT NOT NULL,
    
    -- Number of requests in current window
    request_count INTEGER NOT NULL DEFAULT 0,
    
    -- Start of current window
    window_start TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Number of consecutive violations
    violation_count INTEGER NOT NULL DEFAULT 0,
    
    -- Whether currently blocked due to violations
    blocked_until TIMESTAMPTZ,
    
    -- Last request timestamp
    last_request TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Metadata about the rate limiting
    metadata JSONB,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Unique constraint on key and endpoint
    UNIQUE(key, endpoint)
);

-- Indexes for rate limiting
CREATE INDEX idx_rate_limit_key_endpoint ON rate_limit_state(key, endpoint);
CREATE INDEX idx_rate_limit_window_start ON rate_limit_state(window_start);
CREATE INDEX idx_rate_limit_blocked_until ON rate_limit_state(blocked_until) WHERE blocked_until IS NOT NULL;

-- Device trust enhancements
-- Add columns to track trust expiration and validation
ALTER TABLE auth_device_sessions ADD COLUMN IF NOT EXISTS trusted_until TIMESTAMPTZ;
ALTER TABLE auth_device_sessions ADD COLUMN IF NOT EXISTS trust_extended_count INTEGER DEFAULT 0;
ALTER TABLE auth_device_sessions ADD COLUMN IF NOT EXISTS revoked_at TIMESTAMPTZ;
ALTER TABLE auth_device_sessions ADD COLUMN IF NOT EXISTS revoked_reason TEXT;

-- Index for finding expired trust
CREATE INDEX IF NOT EXISTS idx_auth_device_sessions_trusted_until 
ON auth_device_sessions(trusted_until) 
WHERE status = 'trusted' AND trusted_until IS NOT NULL;

-- Enhance existing refresh_tokens table for security features
-- The table was created in migration 006, we're adding security enhancements
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS id UUID DEFAULT uuid_generate_v4();
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS device_session_id UUID REFERENCES auth_device_sessions(id) ON DELETE CASCADE;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS token_hash TEXT;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS family_id UUID;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS generation INTEGER DEFAULT 1;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS used_at TIMESTAMPTZ;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS used_count INTEGER DEFAULT 0;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS revoked BOOLEAN DEFAULT FALSE;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS revoked_at TIMESTAMPTZ;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS revoked_reason TEXT;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS metadata JSONB;
ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ DEFAULT NOW();

-- Create unique constraint on id if it doesn't exist
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint 
        WHERE conname = 'refresh_tokens_id_unique'
    ) THEN
        ALTER TABLE refresh_tokens ADD CONSTRAINT refresh_tokens_id_unique UNIQUE (id);
    END IF;
END $$;

-- Create unique constraint on token_hash if it doesn't exist
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint 
        WHERE conname = 'refresh_tokens_token_hash_unique'
    ) THEN
        ALTER TABLE refresh_tokens ADD CONSTRAINT refresh_tokens_token_hash_unique UNIQUE (token_hash);
    END IF;
END $$;

-- Indexes for enhanced refresh token functionality (check existence first)
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_device_session ON refresh_tokens(device_session_id) WHERE device_session_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_token_hash ON refresh_tokens(token_hash) WHERE token_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_family_id ON refresh_tokens(family_id) WHERE family_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_active ON refresh_tokens(token_hash) WHERE revoked = FALSE;

-- Function to automatically update updated_at timestamps
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Apply the trigger to relevant tables (skip auth_device_sessions as it already has a trigger from migration 16)
-- The existing trigger trg_auth_device_sessions_updated_at already handles this

CREATE TRIGGER update_rate_limit_state_updated_at BEFORE UPDATE ON rate_limit_state
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Skip creating trigger for refresh_tokens if it already exists
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger 
        WHERE tgname = 'update_refresh_tokens_updated_at' 
        AND tgrelid = 'refresh_tokens'::regclass
    ) THEN
        CREATE TRIGGER update_refresh_tokens_updated_at BEFORE UPDATE ON refresh_tokens
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;

-- Add comments for documentation
COMMENT ON TABLE security_audit_log IS 'Comprehensive security event tracking for audit and compliance';
COMMENT ON TABLE rate_limit_state IS 'Persistent state for distributed rate limiting';
COMMENT ON TABLE refresh_tokens IS 'JWT refresh token tracking with rotation support';
COMMENT ON COLUMN auth_device_sessions.trusted_until IS 'Expiration timestamp for device trust (30 days by default)';
COMMENT ON COLUMN refresh_tokens.family_id IS 'Token family for detecting reuse of rotated tokens';