-- Migration 021: Refactor watch status from MediaID JSONB to UUID + media_type (u8-encoded)
-- This removes dependency on MediaID enum structure and PersonID variant
-- Affects: user_watch_progress, user_completed_media, user_view_history, sync_sessions

-- Step 1: Add new columns to user_watch_progress
ALTER TABLE user_watch_progress
    ADD COLUMN IF NOT EXISTS media_uuid UUID,
    ADD COLUMN IF NOT EXISTS media_type SMALLINT; -- u8 encoding (0..3 for movie/series/season/episode)

-- Step 2: Migrate existing data, filtering out Person types (which are being removed from MediaID)
UPDATE user_watch_progress
SET
    media_uuid = extract_media_id_uuid(media_id_json),
    media_type = CASE
        WHEN media_id_json ? 'Movie' THEN 0
        WHEN media_id_json ? 'Series' THEN 1
        WHEN media_id_json ? 'Season' THEN 2
        WHEN media_id_json ? 'Episode' THEN 3
        ELSE NULL
    END
WHERE media_uuid IS NULL
    AND NOT (media_id_json ? 'Person');

-- Step 3: Delete any entries that couldn't be migrated (Person types or invalid data)
DELETE FROM user_watch_progress
WHERE media_uuid IS NULL OR media_type IS NULL OR (media_id_json ? 'Person');

-- Step 4: Add constraints and update primary key
ALTER TABLE user_watch_progress
    ADD CONSTRAINT check_media_type CHECK (media_type IN (0, 1, 2, 3));

-- Only set NOT NULL after cleaning up the data
ALTER TABLE user_watch_progress
    ALTER COLUMN media_uuid SET NOT NULL,
    ALTER COLUMN media_type SET NOT NULL;

ALTER TABLE user_watch_progress DROP CONSTRAINT IF EXISTS user_watch_progress_pkey;
ALTER TABLE user_watch_progress ADD PRIMARY KEY (user_id, media_uuid);

-- Step 5: Drop old column and indexes
DROP INDEX IF EXISTS idx_watch_progress_media_uuid;
DROP INDEX IF EXISTS idx_watch_progress_media_type;
ALTER TABLE user_watch_progress DROP COLUMN media_id_json;

-- Step 6: Create new indexes
CREATE INDEX idx_watch_progress_media_type ON user_watch_progress(media_type);
CREATE INDEX idx_watch_progress_user_media ON user_watch_progress(user_id, media_uuid);

-- Step 7: Repeat for user_completed_media table
ALTER TABLE user_completed_media
    ADD COLUMN IF NOT EXISTS media_uuid UUID,
    ADD COLUMN IF NOT EXISTS media_type SMALLINT;


UPDATE user_completed_media
SET
    media_uuid = extract_media_id_uuid(media_id_json),
    media_type = CASE
        WHEN media_id_json ? 'Movie' THEN 0
        WHEN media_id_json ? 'Series' THEN 1
        WHEN media_id_json ? 'Season' THEN 2
        WHEN media_id_json ? 'Episode' THEN 3
        ELSE NULL
    END
WHERE media_uuid IS NULL
    AND NOT (media_id_json ? 'Person');

DELETE FROM user_completed_media
WHERE media_uuid IS NULL OR media_type IS NULL OR (media_id_json ? 'Person');

ALTER TABLE user_completed_media
    ADD CONSTRAINT check_media_type CHECK (media_type IN (0, 1, 2, 3));

ALTER TABLE user_completed_media
    ALTER COLUMN media_uuid SET NOT NULL,
    ALTER COLUMN media_type SET NOT NULL;

ALTER TABLE user_completed_media DROP CONSTRAINT IF EXISTS user_completed_media_pkey;
ALTER TABLE user_completed_media ADD PRIMARY KEY (user_id, media_uuid);

DROP INDEX IF EXISTS idx_completed_media_uuid;
ALTER TABLE user_completed_media DROP COLUMN media_id_json;

CREATE INDEX idx_completed_media_uuid ON user_completed_media(media_uuid);
CREATE INDEX idx_completed_media_type ON user_completed_media(media_type);

-- Step 8: Repeat for user_view_history table
ALTER TABLE user_view_history
    ADD COLUMN IF NOT EXISTS media_uuid UUID,
    ADD COLUMN IF NOT EXISTS media_type SMALLINT; -- u8 encoding

UPDATE user_view_history
SET
    media_uuid = extract_media_id_uuid(media_id_json),
    media_type = CASE
        WHEN media_id_json ? 'Movie' THEN 0
        WHEN media_id_json ? 'Series' THEN 1
        WHEN media_id_json ? 'Season' THEN 2
        WHEN media_id_json ? 'Episode' THEN 3
        ELSE NULL
    END
WHERE media_uuid IS NULL
    AND NOT (media_id_json ? 'Person');

DELETE FROM user_view_history
WHERE media_uuid IS NULL OR media_type IS NULL OR (media_id_json ? 'Person');

ALTER TABLE user_view_history
    ADD CONSTRAINT check_media_type CHECK (media_type IN (0, 1, 2, 3));

ALTER TABLE user_view_history
    ALTER COLUMN media_uuid SET NOT NULL,
    ALTER COLUMN media_type SET NOT NULL;

DROP INDEX IF EXISTS idx_view_history_media_uuid;
ALTER TABLE user_view_history DROP COLUMN media_id_json;

CREATE INDEX idx_view_history_media_uuid ON user_view_history(media_uuid);
CREATE INDEX idx_view_history_media_type ON user_view_history(media_type);

-- Step 9: Handle sync_sessions table (from migration 008)
ALTER TABLE sync_sessions
    ADD COLUMN IF NOT EXISTS media_uuid UUID,
    ADD COLUMN IF NOT EXISTS media_type SMALLINT; -- u8 encoding

UPDATE sync_sessions
SET
    media_uuid = extract_media_id_uuid(media_id_json),
    media_type = CASE
        WHEN media_id_json ? 'Movie' THEN 0
        WHEN media_id_json ? 'Series' THEN 1
        WHEN media_id_json ? 'Season' THEN 2
        WHEN media_id_json ? 'Episode' THEN 3
        ELSE NULL
    END
WHERE media_uuid IS NULL
    AND NOT (media_id_json ? 'Person');

DELETE FROM sync_sessions
WHERE media_uuid IS NULL OR media_type IS NULL OR (media_id_json ? 'Person');

ALTER TABLE sync_sessions
    ADD CONSTRAINT check_media_type CHECK (media_type IN (0, 1, 2, 3));

ALTER TABLE sync_sessions
    ALTER COLUMN media_uuid SET NOT NULL,
    ALTER COLUMN media_type SET NOT NULL;

DROP INDEX IF EXISTS idx_sync_sessions_media_uuid;
ALTER TABLE sync_sessions DROP COLUMN media_id_json;

CREATE INDEX idx_sync_sessions_media_uuid ON sync_sessions(media_uuid);
CREATE INDEX idx_sync_sessions_media_type ON sync_sessions(media_type);

-- Step 10: Update the trigger function for completed items
CREATE OR REPLACE FUNCTION check_and_move_completed()
RETURNS TRIGGER AS $$
BEGIN
    -- If progress is > 95%, move to completed
    IF (NEW.position / NEW.duration) > 0.95 THEN
        -- Insert into completed table
        INSERT INTO user_completed_media (user_id, media_uuid, media_type, completed_at)
        VALUES (NEW.user_id, NEW.media_uuid, NEW.media_type, NEW.last_watched)
        ON CONFLICT (user_id, media_uuid) DO UPDATE
        SET completed_at = EXCLUDED.completed_at;

        -- Delete from progress table
        DELETE FROM user_watch_progress
        WHERE user_id = NEW.user_id AND media_uuid = NEW.media_uuid;

        -- Return NULL to cancel the insert/update on progress table
        RETURN NULL;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Step 11: Drop the old extract functions as they're no longer needed
DROP FUNCTION IF EXISTS extract_media_id_uuid(JSONB);
DROP FUNCTION IF EXISTS extract_media_id_type(JSONB);

-- Add comment explaining the schema change
COMMENT ON TABLE user_watch_progress IS 'Tracks user watch progress using UUID + media_type (u8) instead of MediaID JSONB';
COMMENT ON TABLE user_completed_media IS 'Tracks completed media using UUID + media_type (u8) instead of MediaID JSONB';
COMMENT ON TABLE user_view_history IS 'Tracks view history using UUID + media_type (u8) instead of MediaID JSONB';
COMMENT ON TABLE sync_sessions IS 'Sync sessions now use UUID + media_type (u8) instead of MediaID JSONB';
