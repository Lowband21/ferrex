-- Drop sync sessions schema

-- Drop functions
DROP FUNCTION IF EXISTS default_playback_state();
DROP FUNCTION IF EXISTS generate_unique_room_code();
DROP FUNCTION IF EXISTS cleanup_expired_sessions();

-- Drop tables in reverse order of dependencies
DROP TABLE IF EXISTS sync_session_history;
DROP TABLE IF EXISTS sync_participants;
DROP TABLE IF EXISTS sync_sessions;