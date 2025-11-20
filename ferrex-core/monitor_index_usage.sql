-- Index Usage Monitoring Queries
-- Run these after deploying the performance indices to verify they're being used

-- 1. Check index usage statistics
SELECT 
    schemaname,
    tablename,
    indexname,
    idx_scan,
    idx_tup_read,
    idx_tup_fetch,
    CASE 
        WHEN idx_scan = 0 THEN 'UNUSED'
        WHEN idx_scan < 100 THEN 'LOW USAGE'
        WHEN idx_scan < 1000 THEN 'MODERATE USAGE'
        ELSE 'HIGH USAGE'
    END as usage_level
FROM pg_stat_user_indexes
WHERE schemaname = 'public'
ORDER BY idx_scan DESC;

-- 2. Find potentially missing indices (high sequential scans)
SELECT 
    schemaname,
    tablename,
    seq_scan,
    seq_tup_read,
    idx_scan,
    CASE 
        WHEN seq_scan = 0 THEN 0
        ELSE ROUND(100.0 * seq_scan / (seq_scan + idx_scan), 2)
    END as seq_scan_percentage
FROM pg_stat_user_tables
WHERE schemaname = 'public'
    AND seq_scan > 0
ORDER BY seq_tup_read DESC;

-- 3. Index size and bloat check
SELECT 
    schemaname,
    tablename,
    indexname,
    pg_size_pretty(pg_relation_size(indexrelid)) AS index_size,
    idx_scan
FROM pg_stat_user_indexes
WHERE schemaname = 'public'
ORDER BY pg_relation_size(indexrelid) DESC;

-- 4. Query performance before/after indices
-- Run EXPLAIN ANALYZE on these common queries:

-- Movie lookup by file
-- EXPLAIN ANALYZE
-- SELECT mr.*, mm.tmdb_details 
-- FROM movie_references mr
-- JOIN movie_metadata mm ON mr.id = mm.movie_id
-- WHERE mr.file_id = 'some-uuid';

-- Episode navigation
-- EXPLAIN ANALYZE
-- SELECT * FROM episode_references
-- WHERE series_id = 'some-uuid' 
--   AND season_number = 1 
--   AND episode_number = 1;

-- Continue watching query
-- EXPLAIN ANALYZE
-- SELECT * FROM user_watch_progress
-- WHERE user_id = 'some-uuid'
--   AND position > 0 
--   AND (position / duration) < 0.95
-- ORDER BY last_watched DESC
-- LIMIT 10;

-- 5. Index maintenance commands
-- REINDEX INDEX CONCURRENTLY idx_name; -- PostgreSQL 12+
-- Or for older versions:
-- CREATE INDEX CONCURRENTLY idx_name_new ON table(columns);
-- DROP INDEX CONCURRENTLY idx_name;
-- ALTER INDEX idx_name_new RENAME TO idx_name;