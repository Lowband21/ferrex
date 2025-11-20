-- Create table for storing device-specific authentication states
-- This table tracks the current auth state for each device
-- Used by the state machine to persist and recover states

CREATE TABLE auth_device_states (
    device_id UUID PRIMARY KEY,
    state_data JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index on updated_at for cleanup queries and recovery operations
CREATE INDEX idx_auth_device_states_updated_at ON auth_device_states(updated_at DESC);

-- Note: We cannot create a partial index with NOW() as it's not IMMUTABLE
-- The regular index on updated_at will still allow efficient queries for recent states

-- GIN index on state_data JSONB for queries on specific state properties
-- This allows efficient queries on user_id, state type, etc.
CREATE INDEX idx_auth_device_states_state_data ON auth_device_states USING GIN(state_data);

-- Add a trigger to automatically update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_auth_device_states_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_auth_device_states_updated_at
    BEFORE UPDATE ON auth_device_states
    FOR EACH ROW
    EXECUTE FUNCTION update_auth_device_states_updated_at();