-- Device-based authentication system
-- Enables secure device trust with convenient PIN-based user switching

-- Authenticated devices table
CREATE TABLE authenticated_devices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    fingerprint TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,
    app_version TEXT,
    first_authenticated_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    first_authenticated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    trusted_until TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    revoked_by UUID REFERENCES users(id),
    revoked_at TIMESTAMPTZ,
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Device-user specific credentials (PINs)
CREATE TABLE device_user_credentials (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID NOT NULL REFERENCES authenticated_devices(id) ON DELETE CASCADE,
    pin_hash TEXT,
    pin_set_at TIMESTAMPTZ,
    pin_last_used_at TIMESTAMPTZ,
    failed_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    auto_login_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, device_id)
);

-- Sessions with device tracking
CREATE TABLE sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    token_hash TEXT UNIQUE NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID NOT NULL REFERENCES authenticated_devices(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    last_activity TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ip_address INET,
    user_agent TEXT,
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    revoked_at TIMESTAMPTZ
);

-- Authentication event audit log
CREATE TABLE auth_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    device_id UUID REFERENCES authenticated_devices(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL CHECK (event_type IN (
        'password_login_success',
        'password_login_failure',
        'pin_login_success',
        'pin_login_failure',
        'device_registered',
        'device_revoked',
        'pin_set',
        'pin_removed',
        'session_created',
        'session_revoked',
        'auto_login'
    )),
    success BOOLEAN NOT NULL,
    failure_reason TEXT,
    ip_address INET,
    user_agent TEXT,
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX idx_authenticated_devices_fingerprint ON authenticated_devices(fingerprint) WHERE NOT revoked;
CREATE INDEX idx_authenticated_devices_trusted_until ON authenticated_devices(trusted_until) WHERE NOT revoked;
CREATE INDEX idx_sessions_token_hash ON sessions(token_hash) WHERE NOT revoked;
CREATE INDEX idx_sessions_user_device ON sessions(user_id, device_id) WHERE NOT revoked;
CREATE INDEX idx_sessions_expires_at ON sessions(expires_at);
CREATE INDEX idx_device_user_credentials_device ON device_user_credentials(device_id);
CREATE INDEX idx_auth_events_user_id ON auth_events(user_id, created_at DESC);
CREATE INDEX idx_auth_events_device_id ON auth_events(device_id, created_at DESC);
CREATE INDEX idx_auth_events_created_at ON auth_events(created_at DESC);

-- Update trigger for updated_at columns
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_authenticated_devices_updated_at BEFORE UPDATE ON authenticated_devices
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_device_user_credentials_updated_at BEFORE UPDATE ON device_user_credentials
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Add comment documentation
COMMENT ON TABLE authenticated_devices IS 'Tracks devices that have been authenticated with a user password';
COMMENT ON TABLE device_user_credentials IS 'Stores per-device, per-user credentials like PINs for quick switching';
COMMENT ON TABLE sessions IS 'Active user sessions tied to specific devices';
COMMENT ON TABLE auth_events IS 'Audit log of all authentication-related events';

COMMENT ON COLUMN authenticated_devices.fingerprint IS 'Unique device identifier hash (platform + hardware ID + installation ID)';
COMMENT ON COLUMN authenticated_devices.trusted_until IS 'When device trust expires and password re-authentication is required';
COMMENT ON COLUMN device_user_credentials.pin_hash IS 'Argon2 hash of 4-digit PIN with device-specific salt';
COMMENT ON COLUMN device_user_credentials.locked_until IS 'Temporary lockout after failed PIN attempts';
COMMENT ON COLUMN sessions.token_hash IS 'SHA256 hash of the session token for secure storage';