-- Rollback scan state tracking and media processing status tables

-- Remove columns from libraries table
ALTER TABLE libraries DROP COLUMN IF EXISTS auto_scan;
ALTER TABLE libraries DROP COLUMN IF EXISTS watch_for_changes;
ALTER TABLE libraries DROP COLUMN IF EXISTS analyze_on_scan;
ALTER TABLE libraries DROP COLUMN IF EXISTS max_retry_attempts;

-- Drop tables in reverse order of creation (respecting foreign key constraints)
DROP TABLE IF EXISTS file_watch_events;
DROP TABLE IF EXISTS media_processing_status;
DROP TABLE IF EXISTS scan_state;