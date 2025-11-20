-- Remove library_id from season_references table

-- Drop the index
DROP INDEX IF EXISTS idx_season_references_library_id;

-- Drop the foreign key constraint
ALTER TABLE season_references 
DROP CONSTRAINT IF EXISTS fk_season_library;

-- Drop the column
ALTER TABLE season_references 
DROP COLUMN IF EXISTS library_id;