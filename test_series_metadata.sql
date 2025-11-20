-- Test to verify series metadata is being stored correctly
-- Run this after scanning some TV shows

SELECT 
    sr.id,
    sr.title,
    sr.tmdb_id,
    CASE 
        WHEN sm.tmdb_details IS NOT NULL THEN 'Has metadata'
        ELSE 'No metadata'
    END as metadata_status,
    sm.tmdb_details->>'overview' as overview,
    sm.tmdb_details->>'name' as name
FROM series_references sr
LEFT JOIN series_metadata sm ON sr.id = sm.series_id
ORDER BY sr.title;