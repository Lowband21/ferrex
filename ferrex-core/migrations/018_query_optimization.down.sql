-- Rollback query optimization indexes

-- Drop triggers and functions
DROP TRIGGER IF EXISTS update_movie_metadata_arrays_trigger ON movie_metadata;
DROP TRIGGER IF EXISTS update_series_metadata_arrays_trigger ON series_metadata;
DROP FUNCTION IF EXISTS update_movie_metadata_arrays();
DROP FUNCTION IF EXISTS update_series_metadata_arrays();

-- Drop materialized view
DROP MATERIALIZED VIEW IF EXISTS media_query_view CASCADE;
DROP FUNCTION IF EXISTS refresh_media_query_view();

-- Drop indexes on main tables
DROP INDEX IF EXISTS idx_movie_refs_title_lower;
DROP INDEX IF EXISTS idx_movie_metadata_release_date;
DROP INDEX IF EXISTS idx_movie_metadata_vote_average_stored;
DROP INDEX IF EXISTS idx_movie_metadata_runtime;
DROP INDEX IF EXISTS idx_movie_metadata_popularity;
DROP INDEX IF EXISTS idx_movie_refs_library_created;
DROP INDEX IF EXISTS idx_movie_refs_library_tmdb;

DROP INDEX IF EXISTS idx_series_refs_title_lower;
DROP INDEX IF EXISTS idx_series_metadata_first_air_date;
DROP INDEX IF EXISTS idx_series_metadata_vote_average;
DROP INDEX IF EXISTS idx_series_metadata_popularity;
DROP INDEX IF EXISTS idx_series_refs_library_created;

DROP INDEX IF EXISTS idx_season_refs_series_season;
DROP INDEX IF EXISTS idx_episode_refs_series_season_episode;
DROP INDEX IF EXISTS idx_episode_refs_file_id;

-- Drop genre indexes
DROP INDEX IF EXISTS idx_movie_metadata_genres;
DROP INDEX IF EXISTS idx_series_metadata_genres;

-- Drop year indexes
DROP INDEX IF EXISTS idx_movie_metadata_release_year;
DROP INDEX IF EXISTS idx_series_metadata_first_air_year;
DROP INDEX IF EXISTS idx_movie_metadata_year_rating;
DROP INDEX IF EXISTS idx_series_metadata_year_rating;

-- Drop full-text search indexes
DROP INDEX IF EXISTS idx_movie_refs_title_fts;
DROP INDEX IF EXISTS idx_movie_metadata_overview_fts;
DROP INDEX IF EXISTS idx_movie_refs_title_trgm;
DROP INDEX IF EXISTS idx_series_refs_title_fts;
DROP INDEX IF EXISTS idx_series_metadata_overview_fts;
DROP INDEX IF EXISTS idx_series_refs_title_trgm;

-- Drop cast indexes
DROP INDEX IF EXISTS idx_movie_metadata_cast;
DROP INDEX IF EXISTS idx_series_metadata_cast;

-- Drop watch status indexes
DROP INDEX IF EXISTS idx_watch_progress_user_last_watched;
DROP INDEX IF EXISTS idx_watch_progress_media_type;

-- Drop library indexes
DROP INDEX IF EXISTS idx_libraries_enabled;
DROP INDEX IF EXISTS idx_libraries_last_scan;

-- Drop media file indexes
DROP INDEX IF EXISTS idx_media_files_library_created;
DROP INDEX IF EXISTS idx_media_files_unprocessed;

-- Drop generated columns
ALTER TABLE movie_metadata DROP COLUMN IF EXISTS release_date;
ALTER TABLE movie_metadata DROP COLUMN IF EXISTS vote_average;
ALTER TABLE movie_metadata DROP COLUMN IF EXISTS runtime;
ALTER TABLE movie_metadata DROP COLUMN IF EXISTS popularity;
ALTER TABLE movie_metadata DROP COLUMN IF EXISTS overview;
ALTER TABLE movie_metadata DROP COLUMN IF EXISTS genre_names;
ALTER TABLE movie_metadata DROP COLUMN IF EXISTS release_year;
ALTER TABLE movie_metadata DROP COLUMN IF EXISTS cast_names;

ALTER TABLE series_metadata DROP COLUMN IF EXISTS first_air_date;
ALTER TABLE series_metadata DROP COLUMN IF EXISTS vote_average;
ALTER TABLE series_metadata DROP COLUMN IF EXISTS popularity;
ALTER TABLE series_metadata DROP COLUMN IF EXISTS overview;
ALTER TABLE series_metadata DROP COLUMN IF EXISTS status;
ALTER TABLE series_metadata DROP COLUMN IF EXISTS genre_names;
ALTER TABLE series_metadata DROP COLUMN IF EXISTS first_air_year;
ALTER TABLE series_metadata DROP COLUMN IF EXISTS cast_names;