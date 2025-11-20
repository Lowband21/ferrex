-- Synchronized Playback Sessions Schema
-- Epic 3: Synchronized Playback Architecture

-- Main session table
CREATE TABLE sync_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    room_code VARCHAR(6) UNIQUE NOT NULL,
    host_id UUID NOT NULL REFERENCES users(id),
    media_id_json JSONB NOT NULL,
    playback_state JSONB NOT NULL DEFAULT '{"position": 0, "is_playing": false, "playback_rate": 1.0}',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
    is_active BOOLEAN DEFAULT true
);

-- Indexes for sync sessions
CREATE INDEX idx_sync_sessions_room_code ON sync_sessions(room_code) WHERE is_active = true;
CREATE INDEX idx_sync_sessions_expires ON sync_sessions(expires_at) WHERE is_active = true;
CREATE INDEX idx_sync_sessions_host ON sync_sessions(host_id);
CREATE INDEX idx_sync_sessions_media_uuid ON sync_sessions((extract_media_id_uuid(media_id_json)));

-- Session participants
CREATE TABLE sync_participants (
    session_id UUID NOT NULL REFERENCES sync_sessions(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id),
    joined_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    last_ping TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    is_ready BOOLEAN DEFAULT false,
    latency_ms INTEGER DEFAULT 0,
    PRIMARY KEY (session_id, user_id)
);

-- Indexes for participants
CREATE INDEX idx_sync_participants_session ON sync_participants(session_id);
CREATE INDEX idx_sync_participants_user ON sync_participants(user_id);

-- Session history for analytics
CREATE TABLE sync_session_history (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    session_id UUID NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    event_data JSONB,
    user_id UUID REFERENCES users(id),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Index for session history
CREATE INDEX idx_sync_history_session ON sync_session_history(session_id, created_at DESC);

-- Function to clean up expired sessions
CREATE OR REPLACE FUNCTION cleanup_expired_sessions()
RETURNS void AS $$
BEGIN
    -- Mark expired sessions as inactive
    UPDATE sync_sessions
    SET is_active = false
    WHERE expires_at < NOW() AND is_active = true;
    
    -- Delete very old inactive sessions (> 7 days)
    DELETE FROM sync_sessions
    WHERE expires_at < (NOW() - INTERVAL '7 days');
END;
$$ LANGUAGE plpgsql;

-- Function to ensure room code uniqueness
CREATE OR REPLACE FUNCTION generate_unique_room_code()
RETURNS VARCHAR(6) AS $$
DECLARE
    new_code VARCHAR(6);
    code_exists BOOLEAN;
BEGIN
    LOOP
        -- Generate a random 6-character code
        -- Using only uppercase letters and numbers (excluding confusing characters)
        new_code := '';
        FOR i IN 1..6 LOOP
            new_code := new_code || (
                ARRAY['A','B','C','D','E','F','G','H','J','K','L','M','N','P','Q','R','S','T','U','V','W','X','Y','Z','2','3','4','5','6','7','8','9']
            )[floor(random() * 32 + 1)];
        END LOOP;
        
        -- Check if code already exists in active sessions
        SELECT EXISTS(
            SELECT 1 FROM sync_sessions 
            WHERE room_code = new_code AND is_active = true
        ) INTO code_exists;
        
        -- If code doesn't exist, we can use it
        IF NOT code_exists THEN
            RETURN new_code;
        END IF;
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Default playback state constant
CREATE OR REPLACE FUNCTION default_playback_state()
RETURNS JSONB AS $$
BEGIN
    RETURN jsonb_build_object(
        'position', 0,
        'is_playing', false,
        'playback_rate', 1.0,
        'last_sync', EXTRACT(EPOCH FROM NOW())::BIGINT
    );
END;
$$ LANGUAGE plpgsql IMMUTABLE;