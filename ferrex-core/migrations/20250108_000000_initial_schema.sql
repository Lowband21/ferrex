-- Create initial schema for Ferrex Media Server

-- Libraries table with paths as an array
CREATE TABLE IF NOT EXISTS libraries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    library_type TEXT NOT NULL,
    paths TEXT[] NOT NULL DEFAULT '{}',
    scan_interval_minutes INTEGER NOT NULL DEFAULT 60,
    last_scan TIMESTAMPTZ,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Media files table
CREATE TABLE IF NOT EXISTS media_files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_path TEXT NOT NULL UNIQUE,
    file_name TEXT NOT NULL,
    file_size BIGINT NOT NULL,
    media_type TEXT,
    parent_directory TEXT,
    library_id UUID REFERENCES libraries(id) ON DELETE SET NULL,
    parent_media_id UUID REFERENCES media_files(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_scanned TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for media_files
CREATE INDEX idx_media_files_library_id ON media_files(library_id);
CREATE INDEX idx_media_files_parent_directory ON media_files(parent_directory);
CREATE INDEX idx_media_files_media_type ON media_files(media_type);
CREATE INDEX idx_media_files_parent_media_id ON media_files(parent_media_id);

-- Media metadata table
CREATE TABLE IF NOT EXISTS media_metadata (
    media_file_id UUID PRIMARY KEY REFERENCES media_files(id) ON DELETE CASCADE,
    duration_seconds DOUBLE PRECISION,
    width INTEGER,
    height INTEGER,
    video_codec TEXT,
    audio_codec TEXT,
    bitrate_bps BIGINT,
    framerate DOUBLE PRECISION,
    parsed_info JSONB,
    bit_depth INTEGER,
    color_transfer TEXT,
    color_space TEXT,
    color_primaries TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for parsed_info
CREATE INDEX idx_parsed_info_show_name ON media_metadata 
    USING gin ((parsed_info->'show_name'));
CREATE INDEX idx_parsed_info_media_type ON media_metadata 
    USING gin ((parsed_info->'media_type'));

-- Create indexes for HDR filtering
CREATE INDEX idx_media_metadata_bit_depth ON media_metadata(bit_depth) WHERE bit_depth IS NOT NULL;
CREATE INDEX idx_media_metadata_color_transfer ON media_metadata(color_transfer) WHERE color_transfer IS NOT NULL;
CREATE INDEX idx_media_metadata_hdr ON media_metadata(bit_depth, color_transfer) 
    WHERE bit_depth >= 10 OR color_transfer IN ('smpte2084', 'arib-std-b67');

-- External metadata table
CREATE TABLE IF NOT EXISTS external_metadata (
    media_file_id UUID PRIMARY KEY REFERENCES media_files(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    external_id TEXT,
    title TEXT,
    overview TEXT,
    release_date DATE,
    poster_path TEXT,
    backdrop_path TEXT,
    vote_average DECIMAL(3,1),
    vote_count INTEGER,
    popularity DECIMAL(10,3),
    genres JSONB,
    show_description TEXT,
    show_poster_path TEXT,
    season_poster_path TEXT,
    episode_still_path TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for external metadata
CREATE INDEX idx_external_metadata_source ON external_metadata(source);
CREATE INDEX idx_external_metadata_external_id ON external_metadata(external_id);

-- TV Shows table
CREATE TABLE IF NOT EXISTS tv_shows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tmdb_id INTEGER UNIQUE,
    name TEXT NOT NULL,
    overview TEXT,
    poster_path TEXT,
    backdrop_path TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- TV Seasons table
CREATE TABLE IF NOT EXISTS tv_seasons (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tv_show_id UUID NOT NULL REFERENCES tv_shows(id) ON DELETE CASCADE,
    season_number INTEGER NOT NULL,
    name TEXT,
    episode_count INTEGER,
    poster_path TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(tv_show_id, season_number)
);

-- TV Episodes table
CREATE TABLE IF NOT EXISTS tv_episodes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tv_show_id UUID NOT NULL REFERENCES tv_shows(id) ON DELETE CASCADE,
    season_id UUID REFERENCES tv_seasons(id) ON DELETE CASCADE,
    season_number INTEGER NOT NULL,
    episode_number INTEGER NOT NULL,
    media_file_id UUID REFERENCES media_files(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(tv_show_id, season_number, episode_number)
);

-- Metadata refresh log table
CREATE TABLE IF NOT EXISTS metadata_refresh_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    media_file_id UUID NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Function to update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Triggers for automatic updated_at
CREATE TRIGGER update_libraries_updated_at BEFORE UPDATE ON libraries
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

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