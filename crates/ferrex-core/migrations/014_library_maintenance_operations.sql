-- Durable idempotency keys for destructive library maintenance commands.
--
-- Reset preserves the library row's identity by deleting and recreating it in
-- one transaction. This independent operation record survives that delete so
-- a lost HTTP response can be replayed without clearing the library twice.

CREATE SCHEMA IF NOT EXISTS ferrex;

DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname
    INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'libraries'
      AND c.relkind IN ('r', 'p')
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

CREATE TABLE IF NOT EXISTS library_maintenance_operations (
    operation_id uuid PRIMARY KEY,
    library_id uuid NOT NULL,
    operation varchar(32) NOT NULL,
    completed_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT library_maintenance_operations_kind_check
        CHECK (operation IN ('reset'))
);

CREATE INDEX IF NOT EXISTS idx_library_maintenance_operations_library
    ON library_maintenance_operations (library_id, completed_at DESC);

COMMENT ON TABLE library_maintenance_operations IS
    'Completed idempotent destructive library commands. Intentionally has no library FK so reset records survive the delete/recreate transaction.';
