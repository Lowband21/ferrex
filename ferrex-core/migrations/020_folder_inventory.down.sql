-- Rollback migration for folder_inventory table

-- Drop trigger first
DROP TRIGGER IF EXISTS update_folder_inventory_updated_at ON folder_inventory;

-- Drop all indexes
DROP INDEX IF EXISTS idx_folder_inventory_library_id;
DROP INDEX IF EXISTS idx_folder_inventory_parent_folder_id;
DROP INDEX IF EXISTS idx_folder_inventory_processing_queue;
DROP INDEX IF EXISTS idx_folder_inventory_needs_scan;
DROP INDEX IF EXISTS idx_folder_inventory_folder_type;
DROP INDEX IF EXISTS idx_folder_inventory_discovery_source;
DROP INDEX IF EXISTS idx_folder_inventory_retry;
DROP INDEX IF EXISTS idx_folder_inventory_size;
DROP INDEX IF EXISTS idx_folder_inventory_path_gin;

-- Drop the table
DROP TABLE IF EXISTS folder_inventory;