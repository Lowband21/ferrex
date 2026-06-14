-- Track scanner tombstones without deleting logical media references or watch history.
-- The baseline migration moves application objects into ferrex, while some
-- development databases may still own them in public. Resolve the owning schema
-- once and run the availability/rank update there.
CREATE SCHEMA IF NOT EXISTS ferrex;

DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname
    INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'media_files'
      AND c.relkind IN ('r', 'p')
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

ALTER TABLE media_files
    ADD COLUMN IF NOT EXISTS is_available boolean NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS tombstoned_at timestamp with time zone,
    ADD COLUMN IF NOT EXISTS tombstone_reason text,
    ADD COLUMN IF NOT EXISTS fingerprint_device_id text,
    ADD COLUMN IF NOT EXISTS fingerprint_inode bigint,
    ADD COLUMN IF NOT EXISTS fingerprint_size bigint,
    ADD COLUMN IF NOT EXISTS fingerprint_mtime_ms bigint,
    ADD COLUMN IF NOT EXISTS fingerprint_weak_hash text;

CREATE INDEX IF NOT EXISTS idx_media_files_library_available_path
    ON media_files (library_id, is_available, file_path);

CREATE INDEX IF NOT EXISTS idx_media_files_available_fingerprint
    ON media_files (library_id, fingerprint_size, fingerprint_mtime_ms)
    WHERE is_available = true;

-- Keep precomputed movie ranks aligned to the available media slice. Tombstoned
-- files retain their logical movie/watch-history rows, but no longer receive or
-- keep sort positions.
CREATE OR REPLACE FUNCTION rebuild_movie_sort_positions(p_library_id uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    WITH ranks AS (
        SELECT
            mr.library_id,
            mr.id AS movie_id,
            mr.batch_id,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY LOWER(mr.title), mr.id
            ) AS title_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY LOWER(mr.title) DESC, mr.id DESC
            ) AS title_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mf.discovered_at, mr.id
            ) AS date_added_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mf.discovered_at DESC, mr.id DESC
            ) AS date_added_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mf.created_at, mr.id
            ) AS created_at_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mf.created_at DESC, mr.id DESC
            ) AS created_at_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.release_date NULLS LAST, mr.id
            ) AS release_date_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.release_date DESC NULLS LAST, mr.id DESC
            ) AS release_date_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.vote_average NULLS LAST, mr.id
            ) AS rating_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.vote_average DESC NULLS LAST, mr.id DESC
            ) AS rating_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.runtime NULLS LAST, mr.id
            ) AS runtime_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.runtime DESC NULLS LAST, mr.id DESC
            ) AS runtime_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.popularity NULLS LAST, mr.id
            ) AS popularity_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.popularity DESC NULLS LAST, mr.id DESC
            ) AS popularity_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY (mf.technical_metadata->>'bitrate')::BIGINT NULLS LAST, mr.id
            ) AS bitrate_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY (mf.technical_metadata->>'bitrate')::BIGINT DESC NULLS LAST, mr.id DESC
            ) AS bitrate_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mf.file_size NULLS LAST, mr.id
            ) AS file_size_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mf.file_size DESC NULLS LAST, mr.id DESC
            ) AS file_size_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.primary_certification NULLS LAST, mr.id
            ) AS content_rating_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY mm.primary_certification DESC NULLS LAST, mr.id DESC
            ) AS content_rating_pos_desc,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY (mf.technical_metadata->>'height')::INTEGER NULLS LAST, mr.id
            ) AS resolution_pos,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY (mf.technical_metadata->>'height')::INTEGER DESC NULLS LAST, mr.id DESC
            ) AS resolution_pos_desc
        FROM movie_references mr
        JOIN media_files mf
          ON mf.id = mr.file_id
         AND mf.library_id = mr.library_id
        LEFT JOIN movie_metadata mm
          ON mm.movie_id = mr.id
         AND mm.library_id = mr.library_id
        WHERE mr.library_id = p_library_id
          AND mf.is_available = TRUE
    )
    INSERT INTO movie_sort_positions AS msp (
        movie_id, library_id, batch_id, title_pos, title_pos_desc,
        date_added_pos, date_added_pos_desc,
        created_at_pos, created_at_pos_desc,
        release_date_pos, release_date_pos_desc,
        rating_pos, rating_pos_desc,
        runtime_pos, runtime_pos_desc,
        popularity_pos, popularity_pos_desc,
        bitrate_pos, bitrate_pos_desc,
        file_size_pos, file_size_pos_desc,
        content_rating_pos, content_rating_pos_desc,
        resolution_pos, resolution_pos_desc,
        updated_at
    )
    SELECT
        r.movie_id, r.library_id, r.batch_id, r.title_pos, r.title_pos_desc,
        r.date_added_pos, r.date_added_pos_desc,
        r.created_at_pos, r.created_at_pos_desc,
        r.release_date_pos, r.release_date_pos_desc,
        r.rating_pos, r.rating_pos_desc,
        r.runtime_pos, r.runtime_pos_desc,
        r.popularity_pos, r.popularity_pos_desc,
        r.bitrate_pos, r.bitrate_pos_desc,
        r.file_size_pos, r.file_size_pos_desc,
        r.content_rating_pos, r.content_rating_pos_desc,
        r.resolution_pos, r.resolution_pos_desc,
        NOW()
    FROM ranks r
    ON CONFLICT (movie_id) DO UPDATE SET
        library_id = EXCLUDED.library_id,
        batch_id = EXCLUDED.batch_id,
        title_pos = EXCLUDED.title_pos,
        title_pos_desc = EXCLUDED.title_pos_desc,
        date_added_pos = EXCLUDED.date_added_pos,
        date_added_pos_desc = EXCLUDED.date_added_pos_desc,
        created_at_pos = EXCLUDED.created_at_pos,
        created_at_pos_desc = EXCLUDED.created_at_pos_desc,
        release_date_pos = EXCLUDED.release_date_pos,
        release_date_pos_desc = EXCLUDED.release_date_pos_desc,
        rating_pos = EXCLUDED.rating_pos,
        rating_pos_desc = EXCLUDED.rating_pos_desc,
        runtime_pos = EXCLUDED.runtime_pos,
        runtime_pos_desc = EXCLUDED.runtime_pos_desc,
        popularity_pos = EXCLUDED.popularity_pos,
        popularity_pos_desc = EXCLUDED.popularity_pos_desc,
        bitrate_pos = EXCLUDED.bitrate_pos,
        bitrate_pos_desc = EXCLUDED.bitrate_pos_desc,
        file_size_pos = EXCLUDED.file_size_pos,
        file_size_pos_desc = EXCLUDED.file_size_pos_desc,
        content_rating_pos = EXCLUDED.content_rating_pos,
        content_rating_pos_desc = EXCLUDED.content_rating_pos_desc,
        resolution_pos = EXCLUDED.resolution_pos,
        resolution_pos_desc = EXCLUDED.resolution_pos_desc,
        updated_at = NOW();

    DELETE FROM movie_sort_positions m
    WHERE m.library_id = p_library_id
      AND NOT EXISTS (
          SELECT 1
          FROM movie_references mr
          JOIN media_files mf
            ON mf.id = mr.file_id
           AND mf.library_id = mr.library_id
          WHERE mr.id = m.movie_id
            AND mr.library_id = m.library_id
            AND mf.is_available = TRUE
      );
END;
$$;

COMMENT ON FUNCTION rebuild_movie_sort_positions(p_library_id uuid) IS 'Rebuilds precomputed ranks for the given library using available media files';
