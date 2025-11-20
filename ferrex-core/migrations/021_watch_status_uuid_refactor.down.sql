-- Down migration 021: Revert from UUID + media_type (u8) back to MediaID JSONB

-- Step 1: Recreate the extract functions needed for the old schema
CREATE OR REPLACE FUNCTION extract_media_id_uuid(media_id JSONB)
RETURNS UUID AS $$
BEGIN
    RETURN COALESCE(
        (media_id->>'Movie')::UUID,
        (media_id->>'Series')::UUID,
        (media_id->>'Season')::UUID,
        (media_id->>'Episode')::UUID
    );
END;
$$ LANGUAGE plpgsql IMMUTABLE;

CREATE OR REPLACE FUNCTION extract_media_id_type(media_id JSONB)
RETURNS TEXT AS $$
BEGIN
    RETURN CASE
        WHEN media_id ? 'Movie' THEN 'movie'
        WHEN media_id ? 'Episode' THEN 'episode'
        WHEN media_id ? 'Series' THEN 'series'
        WHEN media_id ? 'Season' THEN 'season'
        ELSE NULL
    END;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Step 2: Revert user_watch_progress table
ALTER TABLE user_watch_progress ADD COLUMN media_id_json JSONB;

UPDATE user_watch_progress
SET media_id_json =
    CASE media_type
        WHEN 0 THEN jsonb_build_object('Movie', media_uuid::text)
        WHEN 1 THEN jsonb_build_object('Series', media_uuid::text)
        WHEN 2 THEN jsonb_build_object('Season', media_uuid::text)
        WHEN 3 THEN jsonb_build_object('Episode', media_uuid::text)
    END
WHERE media_id_json IS NULL;

ALTER TABLE user_watch_progress ALTER COLUMN media_id_json SET NOT NULL;

ALTER TABLE user_watch_progress DROP CONSTRAINT IF EXISTS user_watch_progress_pkey;
ALTER TABLE user_watch_progress ADD PRIMARY KEY (user_id, media_id_json);

DROP INDEX IF EXISTS idx_watch_progress_media_type;
DROP INDEX IF EXISTS idx_watch_progress_user_media;

ALTER TABLE user_watch_progress DROP CONSTRAINT IF EXISTS check_media_type;
ALTER TABLE user_watch_progress DROP COLUMN media_uuid;
ALTER TABLE user_watch_progress DROP COLUMN media_type;

-- Recreate old indexes
CREATE INDEX idx_watch_progress_media_uuid ON user_watch_progress((extract_media_id_uuid(media_id_json)));
CREATE INDEX idx_watch_progress_media_type ON user_watch_progress((extract_media_id_type(media_id_json)));

-- Step 3: Revert user_completed_media table
ALTER TABLE user_completed_media ADD COLUMN media_id_json JSONB;

UPDATE user_completed_media
SET media_id_json =
    CASE media_type
        WHEN 0 THEN jsonb_build_object('Movie', media_uuid::text)
        WHEN 1 THEN jsonb_build_object('Series', media_uuid::text)
        WHEN 2 THEN jsonb_build_object('Season', media_uuid::text)
        WHEN 3 THEN jsonb_build_object('Episode', media_uuid::text)
    END
WHERE media_id_json IS NULL;

ALTER TABLE user_completed_media ALTER COLUMN media_id_json SET NOT NULL;

ALTER TABLE user_completed_media DROP CONSTRAINT IF EXISTS user_completed_media_pkey;
ALTER TABLE user_completed_media ADD PRIMARY KEY (user_id, media_id_json);

DROP INDEX IF EXISTS idx_completed_media_uuid;
DROP INDEX IF EXISTS idx_completed_media_type;

ALTER TABLE user_completed_media DROP CONSTRAINT IF EXISTS check_media_type;
ALTER TABLE user_completed_media DROP COLUMN media_uuid;
ALTER TABLE user_completed_media DROP COLUMN media_type;

-- Recreate old indexes
CREATE INDEX idx_completed_media_uuid ON user_completed_media((extract_media_id_uuid(media_id_json)));

-- Step 4: Revert user_view_history table
ALTER TABLE user_view_history ADD COLUMN media_id_json JSONB;

UPDATE user_view_history
SET media_id_json =
    CASE media_type
        WHEN 0 THEN jsonb_build_object('Movie', media_uuid::text)
        WHEN 1 THEN jsonb_build_object('Series', media_uuid::text)
        WHEN 2 THEN jsonb_build_object('Season', media_uuid::text)
        WHEN 3 THEN jsonb_build_object('Episode', media_uuid::text)
    END
WHERE media_id_json IS NULL;

ALTER TABLE user_view_history ALTER COLUMN media_id_json SET NOT NULL;

DROP INDEX IF EXISTS idx_view_history_media_uuid;
DROP INDEX IF EXISTS idx_view_history_media_type;

ALTER TABLE user_view_history DROP CONSTRAINT IF EXISTS check_media_type;
ALTER TABLE user_view_history DROP COLUMN media_uuid;
ALTER TABLE user_view_history DROP COLUMN media_type;

-- Recreate old index
CREATE INDEX idx_view_history_media_uuid ON user_view_history((extract_media_id_uuid(media_id_json)));

-- Step 5: Revert sync_sessions table
ALTER TABLE sync_sessions ADD COLUMN media_id_json JSONB;

UPDATE sync_sessions
SET media_id_json =
    CASE media_type
        WHEN 0 THEN jsonb_build_object('Movie', media_uuid::text)
        WHEN 1 THEN jsonb_build_object('Series', media_uuid::text)
        WHEN 2 THEN jsonb_build_object('Season', media_uuid::text)
        WHEN 3 THEN jsonb_build_object('Episode', media_uuid::text)
    END
WHERE media_id_json IS NULL;

ALTER TABLE sync_sessions ALTER COLUMN media_id_json SET NOT NULL;

DROP INDEX IF EXISTS idx_sync_sessions_media_uuid;
DROP INDEX IF EXISTS idx_sync_sessions_media_type;

ALTER TABLE sync_sessions DROP CONSTRAINT IF EXISTS check_media_type;
ALTER TABLE sync_sessions DROP COLUMN media_uuid;
ALTER TABLE sync_sessions DROP COLUMN media_type;

-- Recreate old index
CREATE INDEX idx_sync_sessions_media_uuid ON sync_sessions((extract_media_id_uuid(media_id_json)));

-- Step 6: Revert the trigger function to original version
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

-- Remove comments about the migration
COMMENT ON TABLE user_watch_progress IS NULL;
COMMENT ON TABLE user_completed_media IS NULL;
COMMENT ON TABLE user_view_history IS NULL;
COMMENT ON TABLE sync_sessions IS NULL;
