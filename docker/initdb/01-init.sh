#!/usr/bin/env bash
set -euo pipefail

# This script runs only on first-time database initialization.
# It creates the application role and ensures privileges on the target database.

FERREX_DB=${FERREX_DB:-ferrex}
FERREX_APP_USER=${FERREX_APP_USER:-ferrex_app}
FERREX_APP_PASSWORD=${FERREX_APP_PASSWORD:-ferrex_app_password}

echo "[initdb] Creating app role '${FERREX_APP_USER}' and granting privileges on database '${FERREX_DB}'"

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" <<-EOSQL
DO
$$
BEGIN
   IF NOT EXISTS (
      SELECT FROM pg_catalog.pg_roles WHERE rolname = '${FERREX_APP_USER}'
   ) THEN
      CREATE ROLE ${FERREX_APP_USER} LOGIN PASSWORD '${FERREX_APP_PASSWORD}';
   END IF;
END
$$;
GRANT ALL PRIVILEGES ON DATABASE ${FERREX_DB} TO ${FERREX_APP_USER};
ALTER DATABASE ${FERREX_DB} OWNER TO ${FERREX_APP_USER};
EOSQL

# Apply privileges inside the application database (schema, future tables)
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$FERREX_DB" <<-EOSQL
GRANT USAGE, CREATE ON SCHEMA public TO ${FERREX_APP_USER};
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO ${FERREX_APP_USER};
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO ${FERREX_APP_USER};
EOSQL

echo "[initdb] App role and database provisioning completed."
