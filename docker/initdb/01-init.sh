#!/usr/bin/env bash
set -euo pipefail

log() {
  printf '[initdb] %s\n' "$*"
}

: "${POSTGRES_USER:?POSTGRES_USER is required}"
: "${FERREX_DB:?FERREX_DB is required}"
: "${FERREX_APP_USER:?FERREX_APP_USER is required}"
if [ -z "${FERREX_APP_PASSWORD:-}" ] && [ -n "${FERREX_APP_PASSWORD_FILE:-}" ]; then
  if [ -r "$FERREX_APP_PASSWORD_FILE" ]; then
    FERREX_APP_PASSWORD="$(<"$FERREX_APP_PASSWORD_FILE")"
  else
    log "Warning: FERREX_APP_PASSWORD_FILE '$FERREX_APP_PASSWORD_FILE' not readable"
  fi
fi
: "${FERREX_APP_PASSWORD:?FERREX_APP_PASSWORD or FERREX_APP_PASSWORD_FILE is required}"

log "Configuring application role '${FERREX_APP_USER}' for database '${FERREX_DB}'."

psql \
  --username "$POSTGRES_USER" \
  --dbname "${POSTGRES_DB:-postgres}" \
  --set=app_user="$FERREX_APP_USER" \
  --set=app_password="$FERREX_APP_PASSWORD" \
  --set=app_database="$FERREX_DB" <<'SQL'
\set ON_ERROR_STOP on
\set schema_name 'public'
SET client_min_messages TO WARNING;

SELECT format($q$
DO $$
DECLARE
  role_name text := %1$L;
  role_password text := %2$L;
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
    EXECUTE format(
      'CREATE ROLE %%I WITH LOGIN NOSUPERUSER NOCREATEROLE NOCREATEDB NOINHERIT NOREPLICATION PASSWORD %%L',
      role_name,
      role_password
    );
  ELSE
    EXECUTE format(
      'ALTER ROLE %%I WITH LOGIN NOSUPERUSER NOCREATEROLE NOCREATEDB NOINHERIT NOREPLICATION PASSWORD %%L',
      role_name,
      role_password
    );
  END IF;
END
$$;
$q$, :'app_user', :'app_password')\gexec

SELECT format($q$
DO $$
DECLARE
  db_name text := %1$L;
  owner text := %2$L;
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = db_name) THEN
    EXECUTE format('CREATE DATABASE %%I OWNER %%I', db_name, owner);
  ELSE
    EXECUTE format('ALTER DATABASE %%I OWNER TO %%I', db_name, owner);
  END IF;
END
$$;
$q$, :'app_database', :'app_user')\gexec

SELECT format($q$
DO $$
DECLARE
  db_name text := %1$L;
  owner text := %2$L;
BEGIN
  EXECUTE format('REVOKE ALL PRIVILEGES ON DATABASE %%I FROM PUBLIC', db_name);
  EXECUTE format('GRANT ALL PRIVILEGES ON DATABASE %%I TO %%I', db_name, owner);
END
$$;
$q$, :'app_database', :'app_user')\gexec
SQL

psql \
  --username "$POSTGRES_USER" \
  --dbname "$FERREX_DB" \
  --set=app_user="$FERREX_APP_USER" <<'SQL'
\set ON_ERROR_STOP on
\set schema_name 'public'
SET client_min_messages TO WARNING;

CREATE EXTENSION IF NOT EXISTS citext WITH SCHEMA public;
CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;
CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;

SELECT format($q$
DO $$
DECLARE
  target_schema text := %1$L;
BEGIN
  EXECUTE format('REVOKE ALL ON SCHEMA %%I FROM PUBLIC', target_schema);
  EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA %%I REVOKE ALL ON TABLES FROM PUBLIC', target_schema);
  EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA %%I REVOKE ALL ON SEQUENCES FROM PUBLIC', target_schema);
END
$$;
$q$, :'schema_name')\gexec

SELECT format($q$
DO $$
DECLARE
  target_schema text := %1$L;
  role_name text := %2$L;
BEGIN
  EXECUTE format('GRANT USAGE, CREATE ON SCHEMA %%I TO %%I', target_schema, role_name);
  EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA %%I TO %%I', target_schema, role_name);
  EXECUTE format('GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA %%I TO %%I', target_schema, role_name);
  EXECUTE format(
    'ALTER DEFAULT PRIVILEGES FOR ROLE %%I IN SCHEMA %%I GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO %%I',
    role_name, target_schema, role_name
  );
  EXECUTE format(
    'ALTER DEFAULT PRIVILEGES FOR ROLE %%I IN SCHEMA %%I GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO %%I',
    role_name, target_schema, role_name
  );
  EXECUTE format(
    'ALTER DEFAULT PRIVILEGES FOR ROLE %%I IN SCHEMA %%I REVOKE ALL ON TABLES FROM PUBLIC',
    role_name, target_schema
  );
  EXECUTE format(
    'ALTER DEFAULT PRIVILEGES FOR ROLE %%I IN SCHEMA %%I REVOKE ALL ON SEQUENCES FROM PUBLIC',
    role_name, target_schema
  );
END
$$;
$q$, :'schema_name', :'app_user')\gexec

-- Ensure search_path resolves application objects in dedicated schema if present
-- Prefer ferrex first, then public
SELECT format($q$
DO $$
DECLARE
  db_name text := current_database();
  role_name text := %1$L;
BEGIN
  -- This requires superuser or database owner
  EXECUTE format('ALTER ROLE %%I IN DATABASE %%I SET search_path = ferrex, public', role_name, db_name);
END
$$;
$q$, :'app_user')\gexec
SQL

log "Provisioning complete."
