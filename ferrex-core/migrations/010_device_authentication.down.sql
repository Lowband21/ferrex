-- Drop device authentication tables and related objects

-- Drop triggers
DROP TRIGGER IF EXISTS update_authenticated_devices_updated_at ON authenticated_devices;
DROP TRIGGER IF EXISTS update_device_user_credentials_updated_at ON device_user_credentials;

-- Drop tables in reverse dependency order
DROP TABLE IF EXISTS auth_events;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS device_user_credentials;
DROP TABLE IF EXISTS authenticated_devices;

-- Note: We keep the update_updated_at_column() function as it might be used by other tables