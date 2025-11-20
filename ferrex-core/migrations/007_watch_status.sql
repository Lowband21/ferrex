-- Watch Status Tracking Schema
-- Epic 2: Watch Status Tracking

-- Efficient watch state storage for in-progress items
CREATE TABLE user_watch_progress (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    media_id_json JSONB NOT NULL,
    position REAL NOT NULL CHECK (position >= 0),
    duration REAL NOT NULL CHECK (duration > 0),
    last_watched BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (user_id, media_id_json)
);

-- Function to extract UUID from MediaId JSONB
CREATE OR REPLACE FUNCTION extract_media_id_uuid(media_id JSONB)
RETURNS UUID AS $$
BEGIN
    RETURN COALESCE(
        (media_id->>'Movie')::UUID,
        (media_id->>'Series')::UUID,
        (media_id->>'Season')::UUID,
        (media_id->>'Episode')::UUID,
        (media_id->>'Person')::UUID
    );
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Function to extract MediaId type
CREATE OR REPLACE FUNCTION extract_media_id_type(media_id JSONB)
RETURNS TEXT AS $$
BEGIN
    RETURN CASE
        WHEN media_id ? 'Movie' THEN 'movie'
        WHEN media_id ? 'Episode' THEN 'episode'
        WHEN media_id ? 'Series' THEN 'series'
        WHEN media_id ? 'Season' THEN 'season'
        WHEN media_id ? 'Person' THEN 'person'
        ELSE NULL
    END;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Indexes for common queries
CREATE INDEX idx_watch_progress_user_last ON user_watch_progress(user_id, last_watched DESC);
CREATE INDEX idx_watch_progress_media_uuid ON user_watch_progress((extract_media_id_uuid(media_id_json)));
CREATE INDEX idx_watch_progress_media_type ON user_watch_progress((extract_media_id_type(media_id_json)));

-- Separate table for completed items (much larger dataset)
CREATE TABLE user_completed_media (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    media_id_json JSONB NOT NULL,
    completed_at BIGINT NOT NULL,
    PRIMARY KEY (user_id, media_id_json)
);

-- Index for completed media lookups
CREATE INDEX idx_completed_user ON user_completed_media(user_id);
CREATE INDEX idx_completed_media_uuid ON user_completed_media((extract_media_id_uuid(media_id_json)));

-- View history table (optional - for future analytics)
CREATE TABLE user_view_history (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    media_id_json JSONB NOT NULL,
    start_position REAL NOT NULL,
    end_position REAL NOT NULL,
    duration REAL NOT NULL,
    viewed_at BIGINT NOT NULL,
    session_duration INTEGER NOT NULL -- seconds spent watching
);

-- Indexes for view history
CREATE INDEX idx_view_history_user ON user_view_history(user_id, viewed_at DESC);
CREATE INDEX idx_view_history_media_uuid ON user_view_history((extract_media_id_uuid(media_id_json)));

-- Function to move completed items from progress to completed
CREATE OR REPLACE FUNCTION check_and_move_completed()
RETURNS TRIGGER AS $$
BEGIN
    -- If progress is > 95%, move to completed
    IF (NEW.position / NEW.duration) > 0.95 THEN
        -- Insert into completed table
        INSERT INTO user_completed_media (user_id, media_id_json, completed_at)
        VALUES (NEW.user_id, NEW.media_id_json, NEW.last_watched)
        ON CONFLICT (user_id, media_id_json) DO UPDATE
        SET completed_at = EXCLUDED.completed_at;
        
        -- Delete from progress table
        DELETE FROM user_watch_progress 
        WHERE user_id = NEW.user_id AND media_id_json = NEW.media_id_json;
        
        -- Return NULL to cancel the insert/update on progress table
        RETURN NULL;
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to automatically move completed items
CREATE TRIGGER move_completed_items
    BEFORE INSERT OR UPDATE ON user_watch_progress
    FOR EACH ROW
    EXECUTE FUNCTION check_and_move_completed();