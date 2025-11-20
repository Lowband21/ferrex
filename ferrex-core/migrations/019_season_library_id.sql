-- Add library_id to season_references table to properly model data hierarchy
-- This allows seasons to know their library without complex runtime derivation

-- Add the library_id column
ALTER TABLE season_references 
ADD COLUMN library_id UUID;

-- Populate library_id from parent series
UPDATE season_references sr
SET library_id = s.library_id
FROM series_references s
WHERE sr.series_id = s.id;

-- Make library_id NOT NULL after populating
ALTER TABLE season_references 
ALTER COLUMN library_id SET NOT NULL;

-- Add foreign key constraint
ALTER TABLE season_references
ADD CONSTRAINT fk_season_library 
FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;

-- Add index for performance
CREATE INDEX idx_season_references_library_id ON season_references(library_id);