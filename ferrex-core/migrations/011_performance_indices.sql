-- Add Missing Performance Indices
-- Migration 011: Performance optimization indices

-- Note: CONCURRENTLY cannot be used with sqlx migrations since they run in a transaction
-- For production with large tables, consider running the indices manually with CONCURRENTLY
-- or use the provided production script: 011_add_performance_indices_production.sh

-- 1. Media Reference Lookups
-- Movie references by file (not already indexed)
CREATE INDEX IF NOT EXISTS idx_movie_references_file_id ON movie_references(file_id);

-- Series episodes composite lookup (improve on existing idx_episode_references_episode_number)
CREATE INDEX IF NOT EXISTS idx_episode_references_composite ON episode_references(series_id, season_number, episode_number);

-- 2. Library Filtering
-- Media files by library and type
CREATE INDEX IF NOT EXISTS idx_media_files_library_type ON media_files(library_id, (parsed_info->>'media_type'));

-- Movies by library and title for sorting (plain btree index for sorting)
CREATE INDEX IF NOT EXISTS idx_movie_references_library_title ON movie_references(library_id, title);

-- 3. Sorting Operations
-- For "recently added" queries
CREATE INDEX IF NOT EXISTS idx_media_files_created_at ON media_files(created_at DESC);

-- For rating-based sorting
-- Note: Creating index on expression requires explicit casting
CREATE INDEX IF NOT EXISTS idx_movie_metadata_rating ON movie_metadata(CAST(tmdb_details->>'vote_average' AS float));

-- 4. Watch Status Queries (adjusted for actual schema)
-- User watch progress lookups (improve on existing indices)
CREATE INDEX IF NOT EXISTS idx_watch_progress_user_media ON user_watch_progress(user_id, media_id_json);

-- Continue watching queries (partial index for efficiency)
CREATE INDEX IF NOT EXISTS idx_watch_progress_continue ON user_watch_progress(user_id, last_watched DESC) 
WHERE position > 0 AND (position / duration) < 0.95;

-- 5. Authentication Performance
-- User lookups by username (case-insensitive) - idx_users_username already exists but not lower
CREATE INDEX IF NOT EXISTS idx_users_username_lower ON users(LOWER(username));

-- Session lookups by refresh token (improve on existing)
CREATE INDEX IF NOT EXISTS idx_user_sessions_refresh_token ON user_sessions(refresh_token) 
WHERE refresh_token IS NOT NULL;

-- 6. Additional useful indices based on actual schema

-- Episode references by file (for quick file to episode lookups)
CREATE INDEX IF NOT EXISTS idx_episode_references_file_id ON episode_references(file_id);

-- Series by title for sorting
CREATE INDEX IF NOT EXISTS idx_series_references_library_title ON series_references(library_id, title);

-- Media files updated_at for sync operations
CREATE INDEX IF NOT EXISTS idx_media_files_updated_at ON media_files(updated_at DESC);

-- User completed media by user and completion time
CREATE INDEX IF NOT EXISTS idx_completed_user_time ON user_completed_media(user_id, completed_at DESC);