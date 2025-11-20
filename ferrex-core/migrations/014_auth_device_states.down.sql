-- Rollback migration for auth_device_states table

-- Drop the trigger and function
DROP TRIGGER IF EXISTS trg_auth_device_states_updated_at ON auth_device_states;
DROP FUNCTION IF EXISTS update_auth_device_states_updated_at();

-- Drop the indexes
DROP INDEX IF EXISTS idx_auth_device_states_updated_at;
DROP INDEX IF EXISTS idx_auth_device_states_state_data;

-- Drop the table
DROP TABLE IF EXISTS auth_device_states;