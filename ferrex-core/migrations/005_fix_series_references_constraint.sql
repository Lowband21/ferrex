-- Fix series_references unique constraint to allow multiple series without TMDB matches
-- This allows series that don't have TMDB IDs to coexist in the same library

-- Drop the existing constraint
ALTER TABLE series_references DROP CONSTRAINT IF EXISTS series_references_tmdb_id_library_id_key;

-- Make tmdb_id nullable to better represent series without TMDB matches
ALTER TABLE series_references ALTER COLUMN tmdb_id DROP NOT NULL;

-- Update any existing tmdb_id = 0 to NULL
UPDATE series_references SET tmdb_id = NULL WHERE tmdb_id = 0;

-- Add new constraint that only enforces uniqueness when tmdb_id is not NULL
-- Note: PostgreSQL requires using CREATE UNIQUE INDEX for partial constraints
CREATE UNIQUE INDEX series_references_tmdb_id_library_id_key 
    ON series_references(tmdb_id, library_id) 
    WHERE tmdb_id IS NOT NULL;

-- Create index on library_id and title for efficient lookups by name
CREATE INDEX IF NOT EXISTS idx_series_references_library_title ON series_references(library_id, title);

-- Down migration
-- To revert this migration, run:
-- DROP INDEX IF EXISTS idx_series_references_library_title;
-- DROP INDEX IF EXISTS series_references_tmdb_id_library_id_key;
-- ALTER TABLE series_references ALTER COLUMN tmdb_id SET NOT NULL;
-- ALTER TABLE series_references ADD CONSTRAINT series_references_tmdb_id_library_id_key UNIQUE (tmdb_id, library_id);