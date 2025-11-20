-- Revert series_references constraint changes

-- Drop the index
DROP INDEX IF EXISTS idx_series_references_library_title;

-- Drop the partial unique index
DROP INDEX IF EXISTS series_references_tmdb_id_library_id_key;

-- Set any NULL tmdb_id values to 0 (this might fail if there are duplicates)
UPDATE series_references SET tmdb_id = 0 WHERE tmdb_id IS NULL;

-- Make tmdb_id NOT NULL again
ALTER TABLE series_references ALTER COLUMN tmdb_id SET NOT NULL;

-- Restore the original constraint
-- Note: Using CREATE UNIQUE INDEX to match the original constraint behavior
CREATE UNIQUE INDEX series_references_tmdb_id_library_id_key 
    ON series_references(tmdb_id, library_id);