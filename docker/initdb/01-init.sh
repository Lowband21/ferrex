#!/usr/bin/env bash
set -euo pipefail

log() {
  printf '[initdb] %s\n' "$*"
}

: "${DATABASE_ADMIN_USER:?DATABASE_ADMIN_USER is required}"
: "${DATABASE_NAME:?DATABASE_NAME is required}"
: "${DATABASE_ADMIN_PASSWORD:?DATABASE_ADMIN_PASSWORD is required}"
: "${DATABASE_APP_USER:?DATABASE_APP_USER is required}"
# psql will read PGPASSWORD when provided; keeps scripts non-interactive.
export PGPASSWORD="$DATABASE_ADMIN_PASSWORD"
if [ -z "${DATABASE_APP_PASSWORD:-}" ] && [ -n "${DATABASE_APP_PASSWORD_FILE:-}" ]; then
  if [ -r "$DATABASE_APP_PASSWORD_FILE" ]; then
    DATABASE_APP_PASSWORD="$(<"$DATABASE_APP_PASSWORD_FILE")"
  else
    log "Warning: DATABASE_APP_PASSWORD_FILE '$DATABASE_APP_PASSWORD_FILE' not readable"
  fi
fi
: "${DATABASE_APP_PASSWORD:?DATABASE_APP_PASSWORD or DATABASE_APP_PASSWORD_FILE is required}"

log "Configuring application role '${DATABASE_APP_USER}' for database '${DATABASE_NAME}'."

psql \
  --username "$DATABASE_ADMIN_USER" \
  --dbname "${DATABASE_NAME:-ferrex}" \
  --set=app_user="$DATABASE_APP_USER" \
  --set=app_password="$DATABASE_APP_PASSWORD" \
  --set=app_database="$DATABASE_NAME" <<'SQL'
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
  --username "$DATABASE_ADMIN_USER" \
  --dbname "$DATABASE_NAME" \
  --set=app_user="$DATABASE_APP_USER" <<'SQL'
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
