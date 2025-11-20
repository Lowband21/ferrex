-- Fix schema mismatches between Rust code and database

-- 1. Fix media_metadata table
-- The column exists as 'bitrate' but Rust struct has 'bitrate_bps'
-- Keep the column name as 'bitrate' to match the query

-- 2. Fix media_files table - make media_type nullable
ALTER TABLE media_files ALTER COLUMN media_type DROP NOT NULL;

-- 3. Rename parent_directory to parent_dir
ALTER TABLE media_files RENAME COLUMN parent_directory TO parent_dir;

-- 4. Fix tv_shows.tmdb_id type from TEXT to INTEGER
-- First drop the unique constraint
ALTER TABLE tv_shows DROP CONSTRAINT IF EXISTS tv_shows_tmdb_id_key;
-- Convert the column (this might fail if there's non-numeric data)
ALTER TABLE tv_shows ALTER COLUMN tmdb_id TYPE INTEGER USING tmdb_id::INTEGER;
-- Re-add the unique constraint
ALTER TABLE tv_shows ADD CONSTRAINT tv_shows_tmdb_id_key UNIQUE (tmdb_id);

-- 5. Fix tv_episodes to match Rust expectations
-- Drop the unique constraint on season_id + episode_number
ALTER TABLE tv_episodes DROP CONSTRAINT IF EXISTS tv_episodes_season_id_episode_number_key;
-- Add season_number column if it doesn't exist
ALTER TABLE tv_episodes ADD COLUMN IF NOT EXISTS season_number INTEGER;
-- Update season_number from tv_seasons table
UPDATE tv_episodes e 
SET season_number = s.season_number 
FROM tv_seasons s 
WHERE e.season_id = s.id 
AND e.season_number IS NULL;
-- Make season_number NOT NULL
ALTER TABLE tv_episodes ALTER COLUMN season_number SET NOT NULL;
-- Add the unique constraint on tv_show_id + season_number + episode_number
ALTER TABLE tv_episodes ADD CONSTRAINT tv_episodes_show_season_episode_key 
    UNIQUE (tv_show_id, season_number, episode_number);

-- 6. Add missing columns to match Rust structs
-- Add library-related tables that are referenced in Rust but missing from migrations
CREATE TABLE IF NOT EXISTS libraries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    library_type TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name)
);

CREATE TABLE IF NOT EXISTS library_paths (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(library_id, path)
);

-- Add library_id and parent_media_id to media_files if they don't exist
ALTER TABLE media_files ADD COLUMN IF NOT EXISTS library_id UUID REFERENCES libraries(id) ON DELETE SET NULL;
ALTER TABLE media_files ADD COLUMN IF NOT EXISTS parent_media_id UUID REFERENCES media_files(id) ON DELETE SET NULL;

-- Create indexes for new columns
CREATE INDEX IF NOT EXISTS idx_media_files_library_id ON media_files(library_id);
CREATE INDEX IF NOT EXISTS idx_media_files_parent_media_id ON media_files(parent_media_id);

-- 7. Fix media_metadata primary key
-- The Rust code expects media_file_id to be the primary key, not a separate id
-- First, drop the existing primary key if it's not media_file_id
DO $$ 
BEGIN
    -- Check if the primary key is on 'id' column
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'media_metadata' 
        AND column_name = 'id' 
        AND table_schema = 'public'
    ) THEN
        -- Drop the old primary key
        ALTER TABLE media_metadata DROP CONSTRAINT IF EXISTS media_metadata_pkey;
        -- Drop the id column
        ALTER TABLE media_metadata DROP COLUMN IF EXISTS id;
        -- Add primary key on media_file_id
        ALTER TABLE media_metadata ADD PRIMARY KEY (media_file_id);
    END IF;
END $$;

-- 8. Rename columns to match Rust field names
ALTER TABLE media_metadata RENAME COLUMN frame_rate TO framerate;

-- 9. Add missing columns for HDR metadata
ALTER TABLE media_metadata ADD COLUMN IF NOT EXISTS color_transfer TEXT;
ALTER TABLE media_metadata ADD COLUMN IF NOT EXISTS color_space TEXT;
ALTER TABLE media_metadata ADD COLUMN IF NOT EXISTS color_primaries TEXT;