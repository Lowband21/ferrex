-- Phase 5: Query Optimization Indexes
-- Adds indexes for efficient server-side query execution
-- Builds upon existing indices from migration 011

-- ====================
-- MOVIE INDEXES
-- ====================

-- Extract commonly queried fields from JSONB for efficient indexing
-- Check if columns exist before adding to avoid errors
DO $$ 
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'movie_metadata' AND column_name = 'release_date') THEN
        ALTER TABLE movie_metadata ADD COLUMN release_date DATE;
    END IF;
    
    -- vote_average might already exist from migration 011, but as expression index
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'movie_metadata' AND column_name = 'vote_average') THEN
        ALTER TABLE movie_metadata ADD COLUMN vote_average NUMERIC(3,1);
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'movie_metadata' AND column_name = 'runtime') THEN
        ALTER TABLE movie_metadata ADD COLUMN runtime INTEGER;
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'movie_metadata' AND column_name = 'popularity') THEN
        ALTER TABLE movie_metadata ADD COLUMN popularity NUMERIC(10,3);
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'movie_metadata' AND column_name = 'overview') THEN
        ALTER TABLE movie_metadata ADD COLUMN overview TEXT;
    END IF;
END $$;

-- Populate the new columns from existing JSONB data
UPDATE movie_metadata 
SET 
    release_date = (tmdb_details->>'release_date')::DATE,
    vote_average = (tmdb_details->>'vote_average')::NUMERIC(3,1),
    runtime = (tmdb_details->>'runtime')::INTEGER,
    popularity = (tmdb_details->>'popularity')::NUMERIC(10,3),
    overview = tmdb_details->>'overview'
WHERE tmdb_details IS NOT NULL;

-- Indexes for common sort fields (avoid duplicates with migration 011)
CREATE INDEX IF NOT EXISTS idx_movie_refs_title_lower 
    ON movie_references(LOWER(title));

CREATE INDEX IF NOT EXISTS idx_movie_metadata_release_date 
    ON movie_metadata(release_date DESC NULLS LAST);

-- Skip vote_average index if idx_movie_metadata_rating exists from migration 011
CREATE INDEX IF NOT EXISTS idx_movie_metadata_vote_average_stored 
    ON movie_metadata(vote_average DESC NULLS LAST);

CREATE INDEX IF NOT EXISTS idx_movie_metadata_runtime 
    ON movie_metadata(runtime);

CREATE INDEX IF NOT EXISTS idx_movie_metadata_popularity 
    ON movie_metadata(popularity DESC NULLS LAST);

-- Composite index for library+created (011 has library+title already)
CREATE INDEX IF NOT EXISTS idx_movie_refs_library_created 
    ON movie_references(library_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_movie_refs_library_tmdb 
    ON movie_references(library_id, tmdb_id);

-- ====================
-- TV SHOW INDEXES
-- ====================

-- Extract commonly queried fields from series JSONB
DO $$ 
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'series_metadata' AND column_name = 'first_air_date') THEN
        ALTER TABLE series_metadata ADD COLUMN first_air_date DATE;
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'series_metadata' AND column_name = 'vote_average') THEN
        ALTER TABLE series_metadata ADD COLUMN vote_average NUMERIC(3,1);
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'series_metadata' AND column_name = 'popularity') THEN
        ALTER TABLE series_metadata ADD COLUMN popularity NUMERIC(10,3);
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'series_metadata' AND column_name = 'overview') THEN
        ALTER TABLE series_metadata ADD COLUMN overview TEXT;
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'series_metadata' AND column_name = 'status') THEN
        ALTER TABLE series_metadata ADD COLUMN status TEXT;
    END IF;
END $$;

-- Populate the new columns from existing JSONB data
UPDATE series_metadata 
SET 
    first_air_date = (tmdb_details->>'first_air_date')::DATE,
    vote_average = (tmdb_details->>'vote_average')::NUMERIC(3,1),
    popularity = (tmdb_details->>'popularity')::NUMERIC(10,3),
    overview = tmdb_details->>'overview',
    status = tmdb_details->>'status'
WHERE tmdb_details IS NOT NULL;

-- Indexes for TV shows (avoid duplicates with migration 011)
CREATE INDEX IF NOT EXISTS idx_series_refs_title_lower 
    ON series_references(LOWER(title));

CREATE INDEX IF NOT EXISTS idx_series_metadata_first_air_date 
    ON series_metadata(first_air_date DESC NULLS LAST);

CREATE INDEX IF NOT EXISTS idx_series_metadata_vote_average 
    ON series_metadata(vote_average DESC NULLS LAST);

CREATE INDEX IF NOT EXISTS idx_series_metadata_popularity 
    ON series_metadata(popularity DESC NULLS LAST);

-- Composite index for library+created (011 already has library+title)
CREATE INDEX IF NOT EXISTS idx_series_refs_library_created 
    ON series_references(library_id, created_at DESC);

-- Season and episode indexes
CREATE INDEX IF NOT EXISTS idx_season_refs_series_season 
    ON season_references(series_id, season_number);

CREATE INDEX IF NOT EXISTS idx_episode_refs_series_season_episode 
    ON episode_references(series_id, season_number, episode_number);

CREATE INDEX IF NOT EXISTS idx_episode_refs_file_id 
    ON episode_references(file_id);

-- ====================
-- GENRE FILTERING
-- ====================

-- Extract genres as arrays for efficient filtering
DO $$ 
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'movie_metadata' AND column_name = 'genre_names') THEN
        ALTER TABLE movie_metadata ADD COLUMN genre_names TEXT[];
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'series_metadata' AND column_name = 'genre_names') THEN
        ALTER TABLE series_metadata ADD COLUMN genre_names TEXT[];
    END IF;
END $$;

-- Populate genre_names from JSONB
UPDATE movie_metadata 
SET genre_names = ARRAY(
    SELECT jsonb_array_elements(tmdb_details->'genres')->>'name'
)
WHERE genre_names IS NULL AND tmdb_details->'genres' IS NOT NULL;

UPDATE series_metadata 
SET genre_names = ARRAY(
    SELECT jsonb_array_elements(tmdb_details->'genres')->>'name'
)
WHERE genre_names IS NULL AND tmdb_details->'genres' IS NOT NULL;

-- GIN indexes for genre filtering
CREATE INDEX IF NOT EXISTS idx_movie_metadata_genres 
    ON movie_metadata USING GIN (genre_names);

CREATE INDEX IF NOT EXISTS idx_series_metadata_genres 
    ON series_metadata USING GIN (genre_names);

-- ====================
-- YEAR RANGE FILTERING
-- ====================

-- Extract year for efficient year range queries
DO $$ 
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'movie_metadata' AND column_name = 'release_year') THEN
        ALTER TABLE movie_metadata ADD COLUMN release_year INTEGER;
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'series_metadata' AND column_name = 'first_air_year') THEN
        ALTER TABLE series_metadata ADD COLUMN first_air_year INTEGER;
    END IF;
END $$;

-- Populate year columns from date columns
UPDATE movie_metadata 
SET release_year = EXTRACT(YEAR FROM release_date)
WHERE release_date IS NOT NULL AND release_year IS NULL;

UPDATE series_metadata 
SET first_air_year = EXTRACT(YEAR FROM first_air_date)
WHERE first_air_date IS NOT NULL AND first_air_year IS NULL;

CREATE INDEX IF NOT EXISTS idx_movie_metadata_release_year 
    ON movie_metadata(release_year);

CREATE INDEX IF NOT EXISTS idx_series_metadata_first_air_year 
    ON series_metadata(first_air_year);

-- Combined year-rating index for common filter combinations
CREATE INDEX IF NOT EXISTS idx_movie_metadata_year_rating 
    ON movie_metadata(release_year, vote_average DESC NULLS LAST);

CREATE INDEX IF NOT EXISTS idx_series_metadata_year_rating 
    ON series_metadata(first_air_year, vote_average DESC NULLS LAST);

-- ====================
-- FULL-TEXT SEARCH
-- ====================

-- Enable required extensions
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Full-text search indexes for movies
CREATE INDEX IF NOT EXISTS idx_movie_refs_title_fts 
    ON movie_references USING GIN (to_tsvector('english', title));

CREATE INDEX IF NOT EXISTS idx_movie_metadata_overview_fts 
    ON movie_metadata USING GIN (to_tsvector('english', COALESCE(overview, '')));

-- Fuzzy search indexes using trigrams
CREATE INDEX IF NOT EXISTS idx_movie_refs_title_trgm 
    ON movie_references USING GIN (title gin_trgm_ops);

-- Full-text search indexes for TV shows
CREATE INDEX IF NOT EXISTS idx_series_refs_title_fts 
    ON series_references USING GIN (to_tsvector('english', title));

CREATE INDEX IF NOT EXISTS idx_series_metadata_overview_fts 
    ON series_metadata USING GIN (to_tsvector('english', COALESCE(overview, '')));

-- Fuzzy search indexes for series
CREATE INDEX IF NOT EXISTS idx_series_refs_title_trgm 
    ON series_references USING GIN (title gin_trgm_ops);

-- ====================
-- CAST/CREW SEARCH
-- ====================

-- Extract cast names for searching
DO $$ 
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'movie_metadata' AND column_name = 'cast_names') THEN
        ALTER TABLE movie_metadata ADD COLUMN cast_names TEXT[];
    END IF;
    
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'series_metadata' AND column_name = 'cast_names') THEN
        ALTER TABLE series_metadata ADD COLUMN cast_names TEXT[];
    END IF;
END $$;

-- Populate cast_names from JSONB
UPDATE movie_metadata 
SET cast_names = ARRAY(
    SELECT jsonb_array_elements(cast_crew->'cast')->>'name'
)
WHERE cast_names IS NULL AND cast_crew->'cast' IS NOT NULL;

UPDATE series_metadata 
SET cast_names = ARRAY(
    SELECT jsonb_array_elements(cast_crew->'cast')->>'name'
)
WHERE cast_names IS NULL AND cast_crew->'cast' IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_movie_metadata_cast 
    ON movie_metadata USING GIN (cast_names);

CREATE INDEX IF NOT EXISTS idx_series_metadata_cast 
    ON series_metadata USING GIN (cast_names);

-- ====================
-- WATCH STATUS OPTIMIZATION
-- ====================

-- Optimize watch progress queries (avoid duplicates with migration 011)
-- Migration 011 already has idx_watch_progress_continue for in-progress items
CREATE INDEX IF NOT EXISTS idx_watch_progress_user_last_watched 
    ON user_watch_progress(user_id, last_watched DESC);

-- Function-based index for media type extraction
CREATE INDEX IF NOT EXISTS idx_watch_progress_media_type 
    ON user_watch_progress(user_id, extract_media_id_type(media_id_json));

-- ====================
-- LIBRARY QUERIES
-- ====================

-- Optimize library listing queries
CREATE INDEX IF NOT EXISTS idx_libraries_enabled 
    ON libraries(enabled, library_type);

CREATE INDEX IF NOT EXISTS idx_libraries_last_scan 
    ON libraries(last_scan DESC NULLS LAST) 
    WHERE enabled = true;

-- ====================
-- MEDIA FILES
-- ====================

-- Optimize file lookups (migration 011 already has idx_media_files_created_at)
-- This composite index is for library-specific queries
CREATE INDEX IF NOT EXISTS idx_media_files_library_created 
    ON media_files(library_id, created_at DESC);

-- Partial index for unprocessed files
CREATE INDEX IF NOT EXISTS idx_media_files_unprocessed 
    ON media_files(library_id, created_at) 
    WHERE technical_metadata IS NULL;

-- ====================
-- QUERY MATERIALIZED VIEW
-- ====================

-- Create materialized view for complex queries
DROP MATERIALIZED VIEW IF EXISTS media_query_view;

CREATE MATERIALIZED VIEW media_query_view AS
WITH movie_data AS (
    SELECT 
        'movie'::text as media_type,
        mr.id,
        mr.library_id,
        mr.tmdb_id,
        mr.title,
        mm.release_date,
        mm.release_year,
        mm.vote_average,
        mm.runtime,
        mm.popularity,
        COALESCE(mm.genre_names, ARRAY[]::TEXT[]) as genre_names,
        mm.overview,
        COALESCE(mm.cast_names, ARRAY[]::TEXT[]) as cast_names,
        mr.created_at,
        mf.file_path,
        mf.file_size,
        to_tsvector('english', mr.title || ' ' || COALESCE(mm.overview, '')) as search_vector
    FROM movie_references mr
    INNER JOIN media_files mf ON mr.file_id = mf.id
    LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id
),
series_data AS (
    SELECT 
        'series'::text as media_type,
        sr.id,
        sr.library_id,
        sr.tmdb_id,
        sr.title,
        sm.first_air_date as release_date,
        sm.first_air_year as release_year,
        sm.vote_average,
        NULL::integer as runtime,
        sm.popularity,
        COALESCE(sm.genre_names, ARRAY[]::TEXT[]) as genre_names,
        sm.overview,
        COALESCE(sm.cast_names, ARRAY[]::TEXT[]) as cast_names,
        sr.created_at,
        NULL::text as file_path,
        NULL::bigint as file_size,
        to_tsvector('english', sr.title || ' ' || COALESCE(sm.overview, '')) as search_vector
    FROM series_references sr
    LEFT JOIN series_metadata sm ON sr.id = sm.series_id
)
SELECT * FROM movie_data
UNION ALL
SELECT * FROM series_data;

-- Index the materialized view
CREATE INDEX idx_media_query_view_library ON media_query_view(library_id);
CREATE INDEX idx_media_query_view_type ON media_query_view(media_type);
CREATE INDEX idx_media_query_view_title ON media_query_view(LOWER(title));
CREATE INDEX idx_media_query_view_year ON media_query_view(release_year);
CREATE INDEX idx_media_query_view_rating ON media_query_view(vote_average DESC NULLS LAST);
CREATE INDEX idx_media_query_view_popularity ON media_query_view(popularity DESC NULLS LAST);
CREATE INDEX idx_media_query_view_created ON media_query_view(created_at DESC);
CREATE INDEX idx_media_query_view_genres ON media_query_view USING GIN(genre_names);
CREATE INDEX idx_media_query_view_search ON media_query_view USING GIN(search_vector);

-- Create unique index for concurrent refresh
CREATE UNIQUE INDEX idx_media_query_view_unique ON media_query_view(media_type, id);

-- Refresh the view
REFRESH MATERIALIZED VIEW media_query_view;

-- Function to refresh the view concurrently
CREATE OR REPLACE FUNCTION refresh_media_query_view()
RETURNS void AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY media_query_view;
END;
$$ LANGUAGE plpgsql;

-- ====================
-- TRIGGERS TO MAINTAIN DERIVED COLUMNS
-- ====================

-- Function to update derived columns on movie_metadata
CREATE OR REPLACE FUNCTION update_movie_metadata_arrays()
RETURNS TRIGGER AS $$
BEGIN
    -- Update from tmdb_details
    IF NEW.tmdb_details IS NOT NULL THEN
        NEW.release_date := (NEW.tmdb_details->>'release_date')::DATE;
        NEW.vote_average := (NEW.tmdb_details->>'vote_average')::NUMERIC(3,1);
        NEW.runtime := (NEW.tmdb_details->>'runtime')::INTEGER;
        NEW.popularity := (NEW.tmdb_details->>'popularity')::NUMERIC(10,3);
        NEW.overview := NEW.tmdb_details->>'overview';
        
        -- Update year from date
        IF NEW.release_date IS NOT NULL THEN
            NEW.release_year := EXTRACT(YEAR FROM NEW.release_date);
        END IF;
        
        -- Update genre_names
        IF NEW.tmdb_details->'genres' IS NOT NULL THEN
            NEW.genre_names := ARRAY(
                SELECT jsonb_array_elements(NEW.tmdb_details->'genres')->>'name'
            );
        END IF;
    END IF;
    
    -- Update cast_names
    IF NEW.cast_crew->'cast' IS NOT NULL THEN
        NEW.cast_names := ARRAY(
            SELECT jsonb_array_elements(NEW.cast_crew->'cast')->>'name'
        );
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Function to update derived columns on series_metadata
CREATE OR REPLACE FUNCTION update_series_metadata_arrays()
RETURNS TRIGGER AS $$
BEGIN
    -- Update from tmdb_details
    IF NEW.tmdb_details IS NOT NULL THEN
        NEW.first_air_date := (NEW.tmdb_details->>'first_air_date')::DATE;
        NEW.vote_average := (NEW.tmdb_details->>'vote_average')::NUMERIC(3,1);
        NEW.popularity := (NEW.tmdb_details->>'popularity')::NUMERIC(10,3);
        NEW.overview := NEW.tmdb_details->>'overview';
        NEW.status := NEW.tmdb_details->>'status';
        
        -- Update year from date
        IF NEW.first_air_date IS NOT NULL THEN
            NEW.first_air_year := EXTRACT(YEAR FROM NEW.first_air_date);
        END IF;
        
        -- Update genre_names
        IF NEW.tmdb_details->'genres' IS NOT NULL THEN
            NEW.genre_names := ARRAY(
                SELECT jsonb_array_elements(NEW.tmdb_details->'genres')->>'name'
            );
        END IF;
    END IF;
    
    -- Update cast_names
    IF NEW.cast_crew->'cast' IS NOT NULL THEN
        NEW.cast_names := ARRAY(
            SELECT jsonb_array_elements(NEW.cast_crew->'cast')->>'name'
        );
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create triggers
CREATE TRIGGER update_movie_metadata_arrays_trigger
    BEFORE INSERT OR UPDATE OF tmdb_details, cast_crew ON movie_metadata
    FOR EACH ROW
    EXECUTE FUNCTION update_movie_metadata_arrays();

CREATE TRIGGER update_series_metadata_arrays_trigger
    BEFORE INSERT OR UPDATE OF tmdb_details, cast_crew ON series_metadata
    FOR EACH ROW
    EXECUTE FUNCTION update_series_metadata_arrays();

-- ====================
-- ANALYZE STATISTICS
-- ====================

-- Update table statistics for query planner
ANALYZE movie_references;
ANALYZE movie_metadata;
ANALYZE series_references;
ANALYZE series_metadata;
ANALYZE season_references;
ANALYZE episode_references;
ANALYZE user_watch_progress;
ANALYZE media_files;
ANALYZE libraries;

-- Comment for documentation
COMMENT ON MATERIALIZED VIEW media_query_view IS 'Optimized view for fast media queries with pre-computed search vectors and extracted metadata';