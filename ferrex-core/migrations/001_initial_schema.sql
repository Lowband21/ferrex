-- Initial schema for Ferrex Media Server
-- Clean architecture with PostgreSQL and JSONB metadata storage

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Libraries table
CREATE TABLE libraries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL UNIQUE,
    library_type VARCHAR(20) NOT NULL CHECK (library_type IN ('movies', 'tvshows')),
    paths TEXT[] NOT NULL,
    scan_interval_minutes INTEGER NOT NULL DEFAULT 60,
    last_scan TIMESTAMP WITH TIME ZONE,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Media files table (actual video files on disk)
CREATE TABLE media_files (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL UNIQUE,
    filename VARCHAR(1000) NOT NULL,
    file_size BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Technical metadata from FFmpeg (JSONB for flexibility)
    technical_metadata JSONB,
    
    -- Parsed filename information (JSONB for flexibility)
    parsed_info JSONB
);

-- Movie references table (lightweight movie objects)
CREATE TABLE movie_references (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    file_id UUID NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
    tmdb_id BIGINT NOT NULL,
    title VARCHAR(1000) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    UNIQUE(tmdb_id, library_id)
);

-- Movie metadata table (full TMDB details)
CREATE TABLE movie_metadata (
    movie_id UUID PRIMARY KEY REFERENCES movie_references(id) ON DELETE CASCADE,
    tmdb_details JSONB NOT NULL,
    images JSONB NOT NULL DEFAULT '{"posters":[],"backdrops":[],"logos":[]}',
    cast_crew JSONB NOT NULL DEFAULT '{"cast":[],"crew":[]}',
    videos JSONB NOT NULL DEFAULT '[]',
    keywords TEXT[] DEFAULT '{}',
    external_ids JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Series references table (lightweight TV series objects)
CREATE TABLE series_references (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    tmdb_id BIGINT NOT NULL,
    title VARCHAR(1000) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    UNIQUE(tmdb_id, library_id)
);

-- Series metadata table (full TMDB details)
CREATE TABLE series_metadata (
    series_id UUID PRIMARY KEY REFERENCES series_references(id) ON DELETE CASCADE,
    tmdb_details JSONB NOT NULL,
    images JSONB NOT NULL DEFAULT '{"posters":[],"backdrops":[],"logos":[]}',
    cast_crew JSONB NOT NULL DEFAULT '{"cast":[],"crew":[]}',
    videos JSONB NOT NULL DEFAULT '[]',
    keywords TEXT[] DEFAULT '{}',
    external_ids JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Season references table
CREATE TABLE season_references (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    series_id UUID NOT NULL REFERENCES series_references(id) ON DELETE CASCADE,
    season_number SMALLINT NOT NULL,
    tmdb_series_id BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    UNIQUE(series_id, season_number)
);

-- Season metadata table
CREATE TABLE season_metadata (
    season_id UUID PRIMARY KEY REFERENCES season_references(id) ON DELETE CASCADE,
    tmdb_details JSONB NOT NULL,
    images JSONB NOT NULL DEFAULT '{"posters":[]}',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Episode references table
CREATE TABLE episode_references (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    series_id UUID NOT NULL REFERENCES series_references(id) ON DELETE CASCADE,
    season_id UUID NOT NULL REFERENCES season_references(id) ON DELETE CASCADE,
    file_id UUID NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
    season_number SMALLINT NOT NULL,
    episode_number SMALLINT NOT NULL,
    tmdb_series_id BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    UNIQUE(series_id, season_number, episode_number)
);

-- Episode metadata table
CREATE TABLE episode_metadata (
    episode_id UUID PRIMARY KEY REFERENCES episode_references(id) ON DELETE CASCADE,
    tmdb_details JSONB NOT NULL,
    still_images JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Performance indices
CREATE INDEX idx_media_files_library_id ON media_files(library_id);
CREATE INDEX idx_media_files_path ON media_files USING HASH (file_path);
CREATE INDEX idx_media_files_technical_metadata ON media_files USING GIN (technical_metadata);
CREATE INDEX idx_media_files_parsed_info ON media_files USING GIN (parsed_info);

CREATE INDEX idx_movie_references_library_id ON movie_references(library_id);
CREATE INDEX idx_movie_references_tmdb_id ON movie_references(tmdb_id);
CREATE INDEX idx_movie_references_title ON movie_references USING GIN (to_tsvector('english', title));

CREATE INDEX idx_movie_metadata_tmdb_details ON movie_metadata USING GIN (tmdb_details);
CREATE INDEX idx_movie_metadata_keywords ON movie_metadata USING GIN (keywords);

CREATE INDEX idx_series_references_library_id ON series_references(library_id);
CREATE INDEX idx_series_references_tmdb_id ON series_references(tmdb_id);
CREATE INDEX idx_series_references_title ON series_references USING GIN (to_tsvector('english', title));

CREATE INDEX idx_series_metadata_tmdb_details ON series_metadata USING GIN (tmdb_details);
CREATE INDEX idx_series_metadata_keywords ON series_metadata USING GIN (keywords);

CREATE INDEX idx_season_references_series_id ON season_references(series_id);
CREATE INDEX idx_season_references_season_number ON season_references(season_number);

CREATE INDEX idx_episode_references_series_id ON episode_references(series_id);
CREATE INDEX idx_episode_references_season_id ON episode_references(season_id);
CREATE INDEX idx_episode_references_episode_number ON episode_references(season_number, episode_number);

-- Update triggers to maintain updated_at timestamps
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_libraries_updated_at BEFORE UPDATE ON libraries FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_media_files_updated_at BEFORE UPDATE ON media_files FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_movie_references_updated_at BEFORE UPDATE ON movie_references FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_movie_metadata_updated_at BEFORE UPDATE ON movie_metadata FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_series_references_updated_at BEFORE UPDATE ON series_references FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_series_metadata_updated_at BEFORE UPDATE ON series_metadata FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_season_references_updated_at BEFORE UPDATE ON season_references FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_season_metadata_updated_at BEFORE UPDATE ON season_metadata FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_episode_references_updated_at BEFORE UPDATE ON episode_references FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_episode_metadata_updated_at BEFORE UPDATE ON episode_metadata FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();