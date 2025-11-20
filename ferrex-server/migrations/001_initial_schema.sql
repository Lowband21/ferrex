-- Create media files table
CREATE TABLE IF NOT EXISTS media_files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_path TEXT NOT NULL UNIQUE,
    file_name TEXT NOT NULL,
    file_size BIGINT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('movie', 'tv_show', 'unknown')),
    parent_directory TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_scanned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create index for faster path lookups
CREATE INDEX idx_media_files_path ON media_files(file_path);
CREATE INDEX idx_media_files_parent_dir ON media_files(parent_directory);
CREATE INDEX idx_media_files_media_type ON media_files(media_type);

-- Create media metadata table for technical metadata
CREATE TABLE IF NOT EXISTS media_metadata (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    media_file_id UUID NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
    duration_seconds DOUBLE PRECISION,
    width INTEGER,
    height INTEGER,
    video_codec TEXT,
    audio_codec TEXT,
    bitrate BIGINT,
    frame_rate DOUBLE PRECISION,
    aspect_ratio TEXT,
    audio_channels INTEGER,
    audio_sample_rate INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(media_file_id)
);

-- Create external metadata table for TMDB data
CREATE TABLE IF NOT EXISTS external_metadata (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    media_file_id UUID NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT 'tmdb',
    external_id TEXT NOT NULL,
    title TEXT NOT NULL,
    original_title TEXT,
    overview TEXT,
    release_date DATE,
    runtime INTEGER,
    vote_average DECIMAL(3,1),
    vote_count INTEGER,
    popularity DECIMAL(10,3),
    poster_path TEXT,
    backdrop_path TEXT,
    genres JSONB,
    production_companies JSONB,
    spoken_languages JSONB,
    metadata_json JSONB,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(media_file_id, source)
);

-- Create TV shows table
CREATE TABLE IF NOT EXISTS tv_shows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tmdb_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    original_name TEXT,
    overview TEXT,
    first_air_date DATE,
    last_air_date DATE,
    status TEXT,
    number_of_seasons INTEGER,
    number_of_episodes INTEGER,
    vote_average DECIMAL(3,1),
    vote_count INTEGER,
    popularity DECIMAL(10,3),
    poster_path TEXT,
    backdrop_path TEXT,
    genres JSONB,
    networks JSONB,
    created_by JSONB,
    metadata_json JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create seasons table
CREATE TABLE IF NOT EXISTS tv_seasons (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tv_show_id UUID NOT NULL REFERENCES tv_shows(id) ON DELETE CASCADE,
    season_number INTEGER NOT NULL,
    name TEXT,
    overview TEXT,
    air_date DATE,
    episode_count INTEGER,
    poster_path TEXT,
    metadata_json JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(tv_show_id, season_number)
);

-- Create episodes table
CREATE TABLE IF NOT EXISTS tv_episodes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tv_show_id UUID NOT NULL REFERENCES tv_shows(id) ON DELETE CASCADE,
    season_id UUID NOT NULL REFERENCES tv_seasons(id) ON DELETE CASCADE,
    media_file_id UUID REFERENCES media_files(id) ON DELETE SET NULL,
    episode_number INTEGER NOT NULL,
    name TEXT,
    overview TEXT,
    air_date DATE,
    runtime INTEGER,
    vote_average DECIMAL(3,1),
    vote_count INTEGER,
    still_path TEXT,
    metadata_json JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(season_id, episode_number)
);

-- Create indexes for TV show relationships
CREATE INDEX idx_tv_seasons_show_id ON tv_seasons(tv_show_id);
CREATE INDEX idx_tv_episodes_show_id ON tv_episodes(tv_show_id);
CREATE INDEX idx_tv_episodes_season_id ON tv_episodes(season_id);
CREATE INDEX idx_tv_episodes_media_file_id ON tv_episodes(media_file_id);

-- Create metadata refresh tracking table
CREATE TABLE IF NOT EXISTS metadata_refresh_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    media_file_id UUID REFERENCES media_files(id) ON DELETE CASCADE,
    tv_show_id UUID REFERENCES tv_shows(id) ON DELETE CASCADE,
    refresh_type TEXT NOT NULL CHECK (refresh_type IN ('technical', 'external', 'full')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'completed', 'failed')),
    error_message TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK ((media_file_id IS NOT NULL AND tv_show_id IS NULL) OR (media_file_id IS NULL AND tv_show_id IS NOT NULL))
);

-- Create cache invalidation triggers
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_media_files_updated_at BEFORE UPDATE ON media_files
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_media_metadata_updated_at BEFORE UPDATE ON media_metadata
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_external_metadata_updated_at BEFORE UPDATE ON external_metadata
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_tv_shows_updated_at BEFORE UPDATE ON tv_shows
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_tv_seasons_updated_at BEFORE UPDATE ON tv_seasons
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_tv_episodes_updated_at BEFORE UPDATE ON tv_episodes
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();