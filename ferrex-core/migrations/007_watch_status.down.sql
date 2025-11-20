-- Drop watch status schema

-- Drop trigger first
DROP TRIGGER IF EXISTS move_completed_items ON user_watch_progress;
DROP FUNCTION IF EXISTS check_and_move_completed();

-- Drop tables
DROP TABLE IF EXISTS user_view_history;
DROP TABLE IF EXISTS user_completed_media;
DROP TABLE IF EXISTS user_watch_progress;

-- Drop helper functions
DROP FUNCTION IF EXISTS extract_media_id_type(JSONB);
DROP FUNCTION IF EXISTS extract_media_id_uuid(JSONB);