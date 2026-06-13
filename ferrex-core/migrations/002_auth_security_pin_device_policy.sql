-- Bring auth/device/PIN schema forward without relying on edits to the
-- already-applied 001_full_schema migration.  Existing deployments may have
-- recorded migration 1 before later auth objects were added to that snapshot,
-- and newer baselines move application objects from public into ferrex.  Keep
-- this migration idempotent and schema-search-path aware.

CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;
CREATE EXTENSION IF NOT EXISTS pg_uuidv7 WITH SCHEMA public;
CREATE SCHEMA IF NOT EXISTS ferrex;

-- Target the schema that already owns the application tables.  Newer
-- baselines move them into ferrex; older deployed databases still have them
-- in public.
DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname
    INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'auth_sessions'
      AND c.relkind IN ('r', 'p')
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

CREATE OR REPLACE FUNCTION uuidv7() RETURNS uuid
    LANGUAGE sql VOLATILE PARALLEL SAFE
    AS $$ SELECT public.uuid_generate_v7(); $$;

-- Device possession uses an enum for supported public-key algorithms.
DO $$
BEGIN
    CREATE TYPE auth_device_key_alg AS ENUM ('ed25519');
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

-- Session/refresh-token scope was added after the original full-schema
-- migration was already applied in some environments.
ALTER TABLE auth_sessions
    ADD COLUMN IF NOT EXISTS scope text DEFAULT 'full'::text NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'auth_sessions_scope_valid'
          AND conrelid = 'auth_sessions'::regclass
    ) THEN
        ALTER TABLE auth_sessions
            ADD CONSTRAINT auth_sessions_scope_valid
            CHECK ((scope = 'full'::text) OR (scope = 'playback'::text));
    END IF;
END $$;

COMMENT ON COLUMN auth_sessions.scope IS 'Session scope controlling access level (full or playback).';

ALTER TABLE auth_refresh_tokens
    ADD COLUMN IF NOT EXISTS origin_scope text DEFAULT 'full'::text NOT NULL;

UPDATE auth_refresh_tokens art
SET origin_scope = asess.scope
FROM auth_sessions asess
WHERE art.session_id IS NOT NULL
  AND art.session_id = asess.id
  AND art.origin_scope <> asess.scope;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'auth_refresh_tokens_origin_scope_valid'
          AND conrelid = 'auth_refresh_tokens'::regclass
    ) THEN
        ALTER TABLE auth_refresh_tokens
            ADD CONSTRAINT auth_refresh_tokens_origin_scope_valid
            CHECK ((origin_scope = 'full'::text) OR (origin_scope = 'playback'::text));
    END IF;
END $$;

COMMENT ON COLUMN auth_refresh_tokens.origin_scope IS 'Sticky origin scope for the refresh token (full or playback).';

-- User-level PIN credentials replaced earlier per-device PIN storage.
ALTER TABLE user_credentials
    ADD COLUMN IF NOT EXISTS pin_hash text,
    ADD COLUMN IF NOT EXISTS pin_client_salt bytea,
    ADD COLUMN IF NOT EXISTS pin_updated_at timestamp with time zone;

UPDATE user_credentials
SET pin_client_salt = gen_random_bytes(16)
WHERE pin_client_salt IS NULL;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_attribute
        WHERE attrelid = 'auth_device_sessions'::regclass
          AND attname = 'pin_hash'
          AND NOT attisdropped
    ) THEN
        EXECUTE $backfill$
            UPDATE user_credentials uc
            SET pin_hash = latest.pin_hash,
                pin_updated_at = COALESCE(latest.pin_set_at, latest.pin_last_used_at, uc.updated_at)
            FROM (
                SELECT DISTINCT ON (user_id)
                    user_id,
                    pin_hash,
                    pin_set_at,
                    pin_last_used_at,
                    updated_at,
                    created_at
                FROM auth_device_sessions
                WHERE pin_hash IS NOT NULL
                ORDER BY user_id,
                    COALESCE(pin_last_used_at, pin_set_at, updated_at, created_at) DESC NULLS LAST
            ) latest
            WHERE uc.user_id = latest.user_id
              AND uc.pin_hash IS NULL
        $backfill$;
    END IF;
END $$;

ALTER TABLE user_credentials
    ALTER COLUMN pin_client_salt SET DEFAULT gen_random_bytes(16),
    ALTER COLUMN pin_client_salt SET NOT NULL;

-- Device trust records need key material columns for challenge signatures.
ALTER TABLE auth_device_sessions
    ADD COLUMN IF NOT EXISTS device_public_key text,
    ADD COLUMN IF NOT EXISTS device_key_alg auth_device_key_alg DEFAULT 'ed25519';

COMMENT ON COLUMN auth_device_sessions.device_public_key IS 'Device-bound public key used to validate possession.';
COMMENT ON COLUMN auth_device_sessions.device_key_alg IS 'Algorithm for device public key (e.g., ed25519).';

-- Challenge nonces back device-possession checks for PIN operations.
CREATE TABLE IF NOT EXISTS auth_device_challenges (
    id uuid DEFAULT uuidv7() NOT NULL,
    device_session_id uuid NOT NULL,
    nonce bytea NOT NULL,
    issued_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    used boolean DEFAULT false NOT NULL
);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'auth_device_challenges_pkey'
          AND conrelid = 'auth_device_challenges'::regclass
    ) THEN
        ALTER TABLE auth_device_challenges
            ADD CONSTRAINT auth_device_challenges_pkey PRIMARY KEY (id);
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'auth_device_challenges_nonce_min_len'
          AND conrelid = 'auth_device_challenges'::regclass
    ) THEN
        ALTER TABLE auth_device_challenges
            ADD CONSTRAINT auth_device_challenges_nonce_min_len
            CHECK (octet_length(nonce) >= 32);
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'auth_device_challenges_device_session_id_fkey'
          AND conrelid = 'auth_device_challenges'::regclass
    ) THEN
        ALTER TABLE auth_device_challenges
            ADD CONSTRAINT auth_device_challenges_device_session_id_fkey
            FOREIGN KEY (device_session_id)
            REFERENCES auth_device_sessions(id)
            ON DELETE CASCADE;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_auth_device_challenges_device_session
    ON auth_device_challenges USING btree (device_session_id);
CREATE INDEX IF NOT EXISTS idx_auth_device_challenges_active
    ON auth_device_challenges USING btree (device_session_id, expires_at)
    WHERE (used = false);

COMMENT ON TABLE auth_device_challenges IS 'Ephemeral nonces for device possession challenges.';
COMMENT ON COLUMN auth_device_challenges.nonce IS 'Opaque random nonce bytes to be signed by the device key.';

-- Persist authentication policy for official-client PIN validation and device trust.
CREATE TABLE IF NOT EXISTS auth_security_settings (
    id uuid DEFAULT uuidv7() NOT NULL,
    admin_password_policy jsonb NOT NULL,
    user_password_policy jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_by uuid,
    CONSTRAINT auth_security_settings_pkey PRIMARY KEY (id)
);

ALTER TABLE auth_security_settings
    ADD COLUMN IF NOT EXISTS updated_by uuid,
    ADD COLUMN IF NOT EXISTS pin_policy jsonb NOT NULL DEFAULT '{
        "min_length": 4,
        "max_length": 8,
        "require_numeric": true,
        "reject_repeated_digits": true,
        "max_consecutive_identical": 2,
        "reject_sequential_digits": true
    }'::jsonb,
    ADD COLUMN IF NOT EXISTS device_trust_policy jsonb NOT NULL DEFAULT '{
        "remember_device_default": false,
        "trust_duration_days": 30,
        "pin_max_attempts": 3,
        "pin_lockout_minutes": 5,
        "admin_pin_unlock_enabled": false
    }'::jsonb;

ALTER TABLE auth_security_settings
    ALTER COLUMN pin_policy DROP DEFAULT,
    ALTER COLUMN device_trust_policy DROP DEFAULT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'auth_security_settings_updated_by_fkey'
          AND conrelid = 'auth_security_settings'::regclass
    ) THEN
        ALTER TABLE auth_security_settings
            ADD CONSTRAINT auth_security_settings_updated_by_fkey
            FOREIGN KEY (updated_by)
            REFERENCES users(id)
            ON DELETE SET NULL;
    END IF;
END $$;

COMMENT ON TABLE auth_security_settings IS 'Authentication policy settings for passwords, official-client PIN validation, device trust, remember-device, lockout, and admin PIN-unlock behavior.';
COMMENT ON COLUMN auth_security_settings.admin_password_policy IS 'JSON payload describing password policy for admin accounts (including first-run binding).';
COMMENT ON COLUMN auth_security_settings.user_password_policy IS 'JSON payload describing password policy for regular user accounts.';
COMMENT ON COLUMN auth_security_settings.updated_by IS 'Admin user who last changed the security settings (nullable during first run).';
COMMENT ON COLUMN auth_security_settings.pin_policy IS 'JSON payload describing raw PIN validation rules enforced by official clients before deriving proof-route PIN material.';
COMMENT ON COLUMN auth_security_settings.device_trust_policy IS 'JSON payload describing remember-device defaults, trusted_until duration, PIN lockout, and admin PIN-unlock behavior.';
