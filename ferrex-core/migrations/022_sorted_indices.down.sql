-- Rollback migration 022: Remove sorted indices table

-- Drop the trigger first
DROP TRIGGER IF EXISTS update_sorted_indices_timestamp_trigger ON library_sorted_indices;

-- Drop the function
DROP FUNCTION IF EXISTS update_sorted_indices_timestamp();

-- Drop the table (this will also drop all indices)
DROP TABLE IF EXISTS library_sorted_indices;