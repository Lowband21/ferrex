-- Allow device-bound playback ticket sessions to coexist with the active
-- full-access session for the same device while preserving one active full
-- session per user/device pair.
CREATE SCHEMA IF NOT EXISTS ferrex;

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

DROP INDEX IF EXISTS auth_sessions_active_per_device;

CREATE UNIQUE INDEX auth_sessions_active_per_device
    ON auth_sessions USING btree (user_id, device_session_id)
    WHERE (
        device_session_id IS NOT NULL
        AND scope = 'full'::text
        AND revoked = false
        AND revoked_at IS NULL
    );

CREATE INDEX IF NOT EXISTS idx_auth_sessions_device_active
    ON auth_sessions USING btree (device_session_id)
    WHERE (
        device_session_id IS NOT NULL
        AND revoked = false
        AND revoked_at IS NULL
    );
