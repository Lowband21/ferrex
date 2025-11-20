-- Rollback: Add Missing Performance Indices
-- Migration 011 rollback

-- Drop all indices created in the up migration
-- Order doesn't matter for index drops

-- 1. Media Reference Lookups
DROP INDEX IF EXISTS idx_movie_references_file_id;
DROP INDEX IF EXISTS idx_episode_references_composite;

-- 2. Library Filtering
DROP INDEX IF EXISTS idx_media_files_library_type;
DROP INDEX IF EXISTS idx_movie_references_library_title;

-- 3. Sorting Operations
DROP INDEX IF EXISTS idx_media_files_created_at;
DROP INDEX IF EXISTS idx_movie_metadata_rating;

-- 4. Watch Status Queries
DROP INDEX IF EXISTS idx_watch_progress_user_media;
DROP INDEX IF EXISTS idx_watch_progress_continue;

-- 5. Authentication Performance
DROP INDEX IF EXISTS idx_users_username_lower;
DROP INDEX IF EXISTS idx_user_sessions_refresh_token;

-- 6. Additional indices
DROP INDEX IF EXISTS idx_episode_references_file_id;
DROP INDEX IF EXISTS idx_series_references_library_title;
DROP INDEX IF EXISTS idx_media_files_updated_at;
DROP INDEX IF EXISTS idx_completed_user_time;