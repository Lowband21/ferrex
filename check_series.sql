-- Simple check for series data

-- Check series count by library
SELECT 
    l.id as library_id,
    l.name as library_name,
    l.library_type,
    COUNT(sr.id) as series_count
FROM libraries l
LEFT JOIN series_references sr ON sr.library_id = l.id
WHERE l.library_type = 'tvshows'
GROUP BY l.id, l.name, l.library_type;

-- Show all series with their library
SELECT 
    sr.id,
    sr.library_id,
    sr.tmdb_id,
    sr.title,
    sr.theme_color,
    l.name as library_name
FROM series_references sr
JOIN libraries l ON l.id = sr.library_id
ORDER BY sr.title;

-- Check for series without matching library
SELECT COUNT(*) as orphaned_series
FROM series_references sr
WHERE NOT EXISTS (
    SELECT 1 FROM libraries l WHERE l.id = sr.library_id
);

-- Specific check for library 2d530a2a-67de-4c9d-8663-db6cfc2a0b38
SELECT 
    sr.id,
    sr.title,
    sr.tmdb_id,
    sr.library_id
FROM series_references sr
WHERE sr.library_id = '2d530a2a-67de-4c9d-8663-db6cfc2a0b38'::uuid;