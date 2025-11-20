-- Rollback initial schema

-- Drop triggers
DROP TRIGGER IF EXISTS update_episode_metadata_updated_at ON episode_metadata;
DROP TRIGGER IF EXISTS update_episode_references_updated_at ON episode_references;
DROP TRIGGER IF EXISTS update_season_metadata_updated_at ON season_metadata;
DROP TRIGGER IF EXISTS update_season_references_updated_at ON season_references;
DROP TRIGGER IF EXISTS update_series_metadata_updated_at ON series_metadata;
DROP TRIGGER IF EXISTS update_series_references_updated_at ON series_references;
DROP TRIGGER IF EXISTS update_movie_metadata_updated_at ON movie_metadata;
DROP TRIGGER IF EXISTS update_movie_references_updated_at ON movie_references;
DROP TRIGGER IF EXISTS update_media_files_updated_at ON media_files;
DROP TRIGGER IF EXISTS update_libraries_updated_at ON libraries;

-- Drop function
DROP FUNCTION IF EXISTS update_updated_at_column();

-- Drop tables in dependency order
DROP TABLE IF EXISTS episode_metadata;
DROP TABLE IF EXISTS episode_references;
DROP TABLE IF EXISTS season_metadata;
DROP TABLE IF EXISTS season_references;
DROP TABLE IF EXISTS series_metadata;
DROP TABLE IF EXISTS series_references;
DROP TABLE IF EXISTS movie_metadata;
DROP TABLE IF EXISTS movie_references;
DROP TABLE IF EXISTS media_files;
DROP TABLE IF EXISTS libraries;