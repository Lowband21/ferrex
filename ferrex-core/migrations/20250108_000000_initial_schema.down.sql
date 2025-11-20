-- Drop all tables and functions in reverse order

DROP TRIGGER IF EXISTS update_tv_episodes_updated_at ON tv_episodes;
DROP TRIGGER IF EXISTS update_tv_seasons_updated_at ON tv_seasons;
DROP TRIGGER IF EXISTS update_tv_shows_updated_at ON tv_shows;
DROP TRIGGER IF EXISTS update_external_metadata_updated_at ON external_metadata;
DROP TRIGGER IF EXISTS update_media_metadata_updated_at ON media_metadata;
DROP TRIGGER IF EXISTS update_media_files_updated_at ON media_files;
DROP TRIGGER IF EXISTS update_libraries_updated_at ON libraries;

DROP FUNCTION IF EXISTS update_updated_at_column();

DROP TABLE IF EXISTS metadata_refresh_log;
DROP TABLE IF EXISTS tv_episodes;
DROP TABLE IF EXISTS tv_seasons;
DROP TABLE IF EXISTS tv_shows;
DROP TABLE IF EXISTS external_metadata;
DROP TABLE IF EXISTS media_metadata;
DROP TABLE IF EXISTS media_files;
DROP TABLE IF EXISTS libraries;