-- Debug script to check series storage issue

-- Check if migration was applied
SELECT conname, contype, pg_get_constraintdef(oid) 
FROM pg_constraint 
WHERE conrelid = 'series_references'::regclass;

-- Check indexes on series_references
SELECT indexname, indexdef 
FROM pg_indexes 
WHERE tablename = 'series_references';

-- Check if tmdb_id is nullable
SELECT column_name, data_type, is_nullable 
FROM information_schema.columns 
WHERE table_name = 'series_references' AND column_name = 'tmdb_id';

-- Count series by library
SELECT library_id, COUNT(*) as series_count 
FROM series_references 
GROUP BY library_id;

-- Check for series with null tmdb_id
SELECT COUNT(*) as null_tmdb_count 
FROM series_references 
WHERE tmdb_id IS NULL;

-- Sample series data
SELECT id, library_id, tmdb_id, title 
FROM series_references 
LIMIT 10;

-- Check for orphaned episodes (episodes without matching series)
SELECT COUNT(*) as orphaned_episodes
FROM episode_references er
WHERE NOT EXISTS (
    SELECT 1 FROM series_references sr 
    WHERE sr.id = er.series_id
);

-- Check library details
SELECT id, name, library_type 
FROM libraries;