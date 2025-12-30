-- Name: citext; Type: EXTENSION; Schema: -; Owner: -
CREATE EXTENSION IF NOT EXISTS citext WITH SCHEMA public;

-- Name: pg_trgm; Type: EXTENSION; Schema: -; Owner: -
CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;

-- Name: pgcrypto; Type: EXTENSION; Schema: -; Owner: -
CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;

-- Create the dedicated schema; owner will be the creating role
CREATE SCHEMA IF NOT EXISTS ferrex;

-- Name: check_and_move_completed(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.check_and_move_completed() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- If progress is > 95%, move to completed
    IF (NEW.position / NEW.duration) > 0.95 THEN
        -- Insert into completed table
        INSERT INTO user_completed_media (user_id, media_uuid, media_type, completed_at)
        VALUES (NEW.user_id, NEW.media_uuid, NEW.media_type, NEW.last_watched)
        ON CONFLICT (user_id, media_uuid) DO UPDATE
        SET completed_at = EXCLUDED.completed_at;
        -- Delete from progress table
        DELETE FROM user_watch_progress
        WHERE user_id = NEW.user_id AND media_uuid = NEW.media_uuid;
        -- Return NULL to cancel the insert/update on progress table
        RETURN NULL;
    END IF;

    RETURN NEW;
END;
$$;

-- Name: cleanup_expired_sessions(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.cleanup_expired_sessions() RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- Mark expired sessions as inactive
    UPDATE sync_sessions
    SET is_active = false
    WHERE expires_at < NOW() AND is_active = true;
    -- Delete very old inactive sessions (> 7 days)
    DELETE FROM sync_sessions
    WHERE expires_at < (NOW() - INTERVAL '7 days');
END;
$$;

-- Name: default_playback_state(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.default_playback_state() RETURNS jsonb
    LANGUAGE plpgsql IMMUTABLE
    AS $$
BEGIN
    RETURN jsonb_build_object(
        'position', 0,
        'is_playing', false,
        'playback_rate', 1.0,
        'last_sync', EXTRACT(EPOCH FROM NOW())::BIGINT
    );
END;
$$;

-- Name: generate_unique_room_code(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.generate_unique_room_code() RETURNS character varying
    LANGUAGE plpgsql
    AS $$
DECLARE
    new_code VARCHAR(6);
    code_exists BOOLEAN;
BEGIN
    LOOP
        -- Generate a random 6-character code Using only uppercase letters and numbers (excluding confusing characters)
        new_code := '';
        FOR i IN 1..6 LOOP
            new_code := new_code || (
                ARRAY['A','B','C','D','E','F','G','H','J','K','L','M','N','P','Q','R','S','T','U','V','W','X','Y','Z','2','3','4','5','6','7','8','9']
            )[floor(random() * 32 + 1)];
        END LOOP;
        -- Check if code already exists in active sessions
        SELECT EXISTS(
            SELECT 1 FROM sync_sessions
            WHERE room_code = new_code AND is_active = true
        ) INTO code_exists;
        -- If code doesn't exist, we can use it
        IF NOT code_exists THEN
            RETURN new_code;
        END IF;
    END LOOP;
END;
$$;

-- Name: rebuild_movie_sort_positions(uuid); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.rebuild_movie_sort_positions(p_library_id uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- Compute ranks per sort dimension (ascending) within the library
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
            -- CreatedAt positions (file created_at)
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
            ) AS resolution_pos
            ,
            ROW_NUMBER() OVER (
                PARTITION BY mr.library_id
                ORDER BY (mf.technical_metadata->>'height')::INTEGER DESC NULLS LAST, mr.id DESC
            ) AS resolution_pos_desc
        FROM movie_references mr
        JOIN media_files mf ON mf.id = mr.file_id
        LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
        WHERE mr.library_id = p_library_id
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
    -- Remove rows for movies no longer in the library
    DELETE FROM movie_sort_positions m
    WHERE m.library_id = p_library_id
      AND NOT EXISTS (SELECT 1 FROM movie_references mr WHERE mr.id = m.movie_id);
END;
$$;

-- Name: FUNCTION rebuild_movie_sort_positions(p_library_id uuid); Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON FUNCTION public.rebuild_movie_sort_positions(p_library_id uuid) IS 'Rebuilds precomputed ranks for the given library';

-- Movie reference batching (per-library linear batch_id allocation)
CREATE FUNCTION public.prevent_movie_ref_batch_size_change_after_first_reference() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.movie_ref_batch_size IS DISTINCT FROM OLD.movie_ref_batch_size THEN
        IF EXISTS (
            SELECT 1
            FROM movie_references mr
            WHERE mr.library_id = NEW.id
            LIMIT 1
        ) THEN
            RAISE EXCEPTION 'Cannot change libraries.movie_ref_batch_size after first movie reference is created for library %', NEW.id
                USING ERRCODE = 'check_violation';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

CREATE FUNCTION public.allocate_or_get_movie_reference_batch_id(
    p_library_id uuid,
    p_tmdb_id bigint
) RETURNS bigint
    LANGUAGE plpgsql
    AS $$
DECLARE
    existing_batch_id bigint;
    current_batch_id bigint;
    current_count integer;
    batch_size integer;
    assigned_batch_id bigint;
    next_batch_id bigint;
    next_count integer;
BEGIN
    -- If this movie reference already exists (upsert conflict path), reuse its existing batch id and do not advance allocation.
    SELECT mr.batch_id
    INTO existing_batch_id
    FROM movie_references mr
    WHERE mr.library_id = p_library_id
      AND mr.tmdb_id = p_tmdb_id;

    IF existing_batch_id IS NOT NULL THEN
        RETURN existing_batch_id;
    END IF;
    -- Ensure a cursor row exists for this library (idempotent), then lock it.
    INSERT INTO movie_reference_batch_cursor (
        library_id, current_batch_id, current_count, batch_size
    )
    SELECT
        l.id,
        1,
        0,
        l.movie_ref_batch_size
    FROM libraries l
    WHERE l.id = p_library_id
    ON CONFLICT (library_id) DO NOTHING;

    SELECT
        c.current_batch_id,
        c.current_count,
        c.batch_size
    INTO
        current_batch_id,
        current_count,
        batch_size
    FROM movie_reference_batch_cursor c
    WHERE c.library_id = p_library_id
    FOR UPDATE;

    IF current_batch_id IS NULL THEN
        RAISE EXCEPTION 'Missing movie batching cursor for library %', p_library_id
            USING ERRCODE = 'foreign_key_violation';
    END IF;
    -- Ensure a batches row exists for the current batch.
    INSERT INTO movie_reference_batches (library_id, batch_id, batch_size)
    VALUES (p_library_id, current_batch_id, batch_size)
    ON CONFLICT (library_id, batch_id) DO NOTHING;
    -- Defensive: if cursor is ever at/over capacity, advance first.
    IF current_count >= batch_size THEN
        UPDATE movie_reference_batches
        SET finalized_at = now()
        WHERE library_id = p_library_id
          AND batch_id = current_batch_id
          AND finalized_at IS NULL;

        next_batch_id := current_batch_id + 1;

        UPDATE movie_reference_batch_cursor
        SET current_batch_id = next_batch_id,
            current_count = 0,
            updated_at = now()
        WHERE library_id = p_library_id;

        INSERT INTO movie_reference_batches (library_id, batch_id, batch_size)
        VALUES (p_library_id, next_batch_id, batch_size)
        ON CONFLICT (library_id, batch_id) DO NOTHING;

        current_batch_id := next_batch_id;
        current_count := 0;
    END IF;

    assigned_batch_id := current_batch_id;
    next_count := current_count + 1;
    -- If we just filled the batch, finalize it and advance the cursor for the next insert.
    IF next_count >= batch_size THEN
        UPDATE movie_reference_batches
        SET finalized_at = now()
        WHERE library_id = p_library_id
          AND batch_id = assigned_batch_id
          AND finalized_at IS NULL;

        next_batch_id := assigned_batch_id + 1;

        UPDATE movie_reference_batch_cursor
        SET current_batch_id = next_batch_id,
            current_count = 0,
            updated_at = now()
        WHERE library_id = p_library_id;

        INSERT INTO movie_reference_batches (library_id, batch_id, batch_size)
        VALUES (p_library_id, next_batch_id, batch_size)
        ON CONFLICT (library_id, batch_id) DO NOTHING;
    ELSE
        UPDATE movie_reference_batch_cursor
        SET current_count = next_count,
            updated_at = now()
        WHERE library_id = p_library_id;
    END IF;

    RETURN assigned_batch_id;
END;
$$;

CREATE FUNCTION public.set_movie_reference_batch_id() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.batch_id IS NULL THEN
        NEW.batch_id := allocate_or_get_movie_reference_batch_id(NEW.library_id, NEW.tmdb_id);
    END IF;
    RETURN NEW;
END;
$$;

-- Name: refresh_media_query_view(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.refresh_media_query_view() RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY media_query_view;
END;
$$;

-- Name: update_auth_device_sessions_updated_at(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.update_auth_device_sessions_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

-- Name: update_movie_metadata_arrays(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.update_movie_metadata_arrays() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- Update from tmdb_details
    IF NEW.tmdb_details IS NOT NULL THEN
        NEW.release_date := (NEW.tmdb_details->>'release_date')::DATE;
        NEW.vote_average := (NEW.tmdb_details->>'vote_average')::NUMERIC(3,1);
        NEW.runtime := (NEW.tmdb_details->>'runtime')::INTEGER;
        NEW.popularity := (NEW.tmdb_details->>'popularity')::NUMERIC(10,3);
        NEW.overview := NEW.tmdb_details->>'overview';
        -- Update year from date
        IF NEW.release_date IS NOT NULL THEN
            NEW.release_year := EXTRACT(YEAR FROM NEW.release_date);
        END IF;
        -- Update genre_names
        IF NEW.tmdb_details->'genres' IS NOT NULL THEN
            NEW.genre_names := ARRAY(
                SELECT jsonb_array_elements(NEW.tmdb_details->'genres')->>'name'
            );
        END IF;
    END IF;
    -- Update cast_names
    IF NEW.cast_crew->'cast' IS NOT NULL THEN
        NEW.cast_names := ARRAY(
            SELECT jsonb_array_elements(NEW.cast_crew->'cast')->>'name'
        );
    END IF;

    RETURN NEW;
END;
$$;

-- Name: update_series_metadata_arrays(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.update_series_metadata_arrays() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- Update from tmdb_details
    IF NEW.tmdb_details IS NOT NULL THEN
        NEW.first_air_date := (NEW.tmdb_details->>'first_air_date')::DATE;
        NEW.vote_average := (NEW.tmdb_details->>'vote_average')::NUMERIC(3,1);
        NEW.popularity := (NEW.tmdb_details->>'popularity')::NUMERIC(10,3);
        NEW.overview := NEW.tmdb_details->>'overview';
        NEW.status := NEW.tmdb_details->>'status';
        -- Update year from date
        IF NEW.first_air_date IS NOT NULL THEN
            NEW.first_air_year := EXTRACT(YEAR FROM NEW.first_air_date);
        END IF;
        -- Update genre_names
        IF NEW.tmdb_details->'genres' IS NOT NULL THEN
            NEW.genre_names := ARRAY(
                SELECT jsonb_array_elements(NEW.tmdb_details->'genres')->>'name'
            );
        END IF;
    END IF;
    -- Update cast_names
    IF NEW.cast_crew->'cast' IS NOT NULL THEN
        NEW.cast_names := ARRAY(
            SELECT jsonb_array_elements(NEW.cast_crew->'cast')->>'name'
        );
    END IF;

    RETURN NEW;
END;
$$;

-- Name: update_updated_at_column(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.update_updated_at_column() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

-- Name: update_updated_at_timestamp(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.update_updated_at_timestamp() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;


-- Name: propagate_cached_image_theme_color(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.propagate_cached_image_theme_color() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- Only propagate poster-derived theme colors.
    IF NEW.image_variant <> 'poster'::image_variant THEN
        RETURN NEW;
    END IF;

    -- Some application paths may persist "no theme color" as an empty string.
    -- Treat it as missing and do not propagate.
    IF NEW.theme_color IS NULL OR NEW.theme_color = '' THEN
        RETURN NEW;
    END IF;

    -- Movie posters: propagate to the movie reference if it doesn't already have a theme color.
    UPDATE movie_references mr
    SET theme_color = NEW.theme_color
    FROM movie_metadata mm
    WHERE mm.primary_poster_image_id = NEW.image_id
      AND mr.id = mm.movie_id
      AND (mr.theme_color IS NULL OR mr.theme_color = '');

    -- Series posters: propagate to the series reference if it doesn't already have a theme color.
    UPDATE series s
    SET theme_color = NEW.theme_color
    FROM series_metadata sm
    WHERE sm.primary_poster_image_id = NEW.image_id
      AND s.id = sm.series_id
      AND (s.theme_color IS NULL OR s.theme_color = '');

    RETURN NEW;
END;
$$;

-- Name: propagate_movie_reference_theme_color(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.propagate_movie_reference_theme_color() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    selected_theme_color text;
BEGIN
    -- Ensure ordering-independence: if the metadata row is inserted/updated after
    -- the image is already cached, pull the theme color from cached_images.
    SELECT ci.theme_color
    INTO selected_theme_color
    FROM cached_images ci
    WHERE ci.image_id = NEW.primary_poster_image_id
      AND ci.image_variant = 'poster'::image_variant
      AND ci.theme_color IS NOT NULL
      AND ci.theme_color <> ''
    ORDER BY
        (ci.size_variant = 'original'::size_variant) DESC,
        ci.modified_at DESC
    LIMIT 1;

    IF selected_theme_color IS NULL THEN
        RETURN NEW;
    END IF;

    UPDATE movie_references mr
    SET theme_color = selected_theme_color
    WHERE mr.id = NEW.movie_id
      AND (mr.theme_color IS NULL OR mr.theme_color = '');

    RETURN NEW;
END;
$$;

-- Name: propagate_series_reference_theme_color(); Type: FUNCTION; Schema: public; Owner: postgres
CREATE FUNCTION public.propagate_series_reference_theme_color() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    selected_theme_color text;
BEGIN
    -- Ensure ordering-independence: if the metadata row is inserted/updated after
    -- the image is already cached, pull the theme color from cached_images.
    SELECT ci.theme_color
    INTO selected_theme_color
    FROM cached_images ci
    WHERE ci.image_id = NEW.primary_poster_image_id
      AND ci.image_variant = 'poster'::image_variant
      AND ci.theme_color IS NOT NULL
      AND ci.theme_color <> ''
    ORDER BY
        (ci.size_variant = 'original'::size_variant) DESC,
        ci.modified_at DESC
    LIMIT 1;

    IF selected_theme_color IS NULL THEN
        RETURN NEW;
    END IF;

    UPDATE series s
    SET theme_color = selected_theme_color
    WHERE s.id = NEW.series_id
      AND (s.theme_color IS NULL OR s.theme_color = '');

    RETURN NEW;
END;
$$;


-- Name: admin_actions; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.admin_actions (
    id uuid DEFAULT uuidv7() PRIMARY KEY,
    admin_id uuid NOT NULL,
    action_type character varying(100) NOT NULL,
    target_type character varying(50),
    target_id uuid,
    description text,
    metadata jsonb,
    ip_address inet,
    created_at timestamp with time zone DEFAULT now()
);

-- Name: TABLE admin_actions; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.admin_actions IS 'Audit log for administrative actions';

-- Name: auth_device_status; Type: TYPE; Schema: public; Owner: postgres
CREATE TYPE public.auth_device_status AS ENUM (
    'pending',
    'trusted',
    'revoked'
);

-- Name: auth_event_type; Type: TYPE; Schema: public; Owner: postgres
CREATE TYPE public.auth_event_type AS ENUM (
    'password_login_success',
    'password_login_failure',
    'pin_login_success',
    'pin_login_failure',
    'device_registered',
    'device_revoked',
    'pin_set',
    'pin_removed',
    'session_created',
    'session_revoked',
    'auto_login'
);

-- Constrain device key algorithm to a known enum
DO $$ BEGIN
    CREATE TYPE public.auth_device_key_alg AS ENUM ('ed25519');
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

-- Name: auth_device_sessions; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.auth_device_sessions (
    id uuid DEFAULT uuidv7() PRIMARY KEY,
    user_id uuid NOT NULL,
    device_fingerprint text NOT NULL,
    device_name text NOT NULL,
    platform text,
    app_version text,
    hardware_id text,
    -- Device-bound public key for possession checks (PEM or base64 per alg)
    device_public_key text,
    -- Public key algorithm identifier
    device_key_alg public.auth_device_key_alg DEFAULT 'ed25519',
    status public.auth_device_status NOT NULL DEFAULT 'pending',
    pin_last_used_at timestamp with time zone,
    failed_attempts smallint DEFAULT 0 NOT NULL,
    locked_until timestamp with time zone,
    trusted_until timestamp with time zone,
    auto_login_enabled boolean DEFAULT false NOT NULL,
    first_authenticated_by uuid NOT NULL,
    first_authenticated_at timestamp with time zone DEFAULT now() NOT NULL,
    last_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    last_activity timestamp with time zone DEFAULT now() NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    revoked_at timestamp with time zone,
    revoked_by uuid,
    revoked_reason text,
    CONSTRAINT auth_device_sessions_failed_attempts_non_negative CHECK ((failed_attempts >= 0)),
    CONSTRAINT auth_device_sessions_fingerprint_length CHECK ((char_length(device_fingerprint) = 64)),
    CONSTRAINT auth_device_sessions_trust_after_first_auth CHECK ((trusted_until IS NULL) OR (trusted_until >= first_authenticated_at))
);

-- Name: TABLE auth_device_sessions; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.auth_device_sessions IS 'Per-device trust record combining PIN policy, lockout state, and session metadata';

-- Name: COLUMN auth_device_sessions.device_fingerprint; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.device_fingerprint IS 'SHA256 device fingerprint stored as lowercase hex (64 characters)';

-- Name: COLUMN auth_device_sessions.status; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.status IS 'Device trust lifecycle status (pending, trusted, revoked)';

-- Name: COLUMN auth_device_sessions.failed_attempts; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.failed_attempts IS 'Failed PIN attempts since the last successful authentication';

-- Name: COLUMN auth_device_sessions.locked_until; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.locked_until IS 'When the device PIN becomes available again after lockout';

-- Name: COLUMN auth_device_sessions.trusted_until; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.trusted_until IS 'Expiration timestamp for device trust before password revalidation is required';

-- Name: COLUMN auth_device_sessions.auto_login_enabled; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.auto_login_enabled IS 'Whether auto-login is allowed for this device without prompting the password again';

-- Name: COLUMN auth_device_sessions.last_seen_at; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.last_seen_at IS 'Last time the device checked in with the server';

-- Name: COLUMN auth_device_sessions.metadata; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.metadata IS 'Additional device metadata such as hardware hints and client identifiers';

-- Name: auth_events; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.auth_events (
    id uuid DEFAULT uuidv7() PRIMARY KEY,
    user_id uuid,
    device_session_id uuid,
    session_id uuid,
    event_type public.auth_event_type NOT NULL,
    success boolean NOT NULL,
    failure_reason text,
    ip_address inet,
    user_agent text,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

-- Name: TABLE auth_events; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.auth_events IS 'Audit log of authentication activity with user, device, and session context';

-- Name: COLUMN auth_events.device_session_id; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_events.device_session_id IS 'Device session associated with this event when available';

-- Name: COLUMN auth_events.session_id; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_events.session_id IS 'Auth session affected by this event when applicable';

-- Name: COLUMN auth_events.event_type; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_events.event_type IS 'Categorized authentication event type enforced by enum';

-- Name: episode_content_ratings; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_content_ratings (
    episode_id uuid NOT NULL,
    iso_3166_1 text NOT NULL,
    rating text,
    rating_system text,
    descriptors text[] DEFAULT ARRAY[]::text[],
    CONSTRAINT episode_content_ratings_pkey PRIMARY KEY (episode_id, iso_3166_1)
);

-- Name: episode_keywords; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_keywords (
    episode_id uuid NOT NULL,
    keyword_id bigint NOT NULL,
    name text NOT NULL,
    CONSTRAINT episode_keywords_pkey PRIMARY KEY (episode_id, keyword_id)
);

-- Name: episode_metadata; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_metadata (
    episode_id uuid PRIMARY KEY,
    tmdb_id bigint NOT NULL,
    series_tmdb_id bigint,
    season_tmdb_id bigint,
    season_number integer,
    episode_number integer,
    name text,
    overview text,
    air_date date,
    runtime integer,
    still_path text,
    primary_thumbnail_image_id uuid,
    vote_average real,
    vote_count integer,
    production_code text,
    imdb_id text,
    tvdb_id bigint,
    facebook_id text,
    instagram_id text,
    twitter_id text,
    wikidata_id text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_episode_metadata_name_trgm
    ON public.episode_metadata
    USING gin (name public.gin_trgm_ops)
    WHERE name IS NOT NULL;

-- Name: episode_references; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_references (
    id uuid DEFAULT uuidv7() CONSTRAINT episode_references_pkey PRIMARY KEY,
    series_id uuid NOT NULL,
    season_id uuid NOT NULL,
    file_id uuid NOT NULL,
    season_number smallint NOT NULL,
    episode_number smallint NOT NULL,
    tmdb_series_id bigint NOT NULL,
    -- When this episode reference was discovered/created in DB
    discovered_at timestamp with time zone DEFAULT now() NOT NULL,
    -- Optional content creation timestamp
    created_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT episode_references_series_id_season_number_episode_number_key UNIQUE (series_id, season_number, episode_number)
);

-- Name: episode_translations; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_translations (
    episode_id uuid NOT NULL,
    iso_3166_1 text NOT NULL,
    iso_639_1 text NOT NULL,
    name text,
    english_name text,
    title text,
    overview text,
    homepage text,
    tagline text,
    CONSTRAINT episode_translations_pkey PRIMARY KEY (episode_id, iso_3166_1, iso_639_1)
);

-- Name: episode_videos; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_videos (
    episode_id uuid NOT NULL,
    video_key text NOT NULL,
    site text NOT NULL,
    name text,
    video_type text,
    official boolean,
    iso_639_1 text,
    iso_3166_1 text,
    published_at timestamp with time zone,
    size integer,
    CONSTRAINT episode_videos_pkey PRIMARY KEY (episode_id, video_key, site)
);

-- Name: file_watch_events; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.file_watch_events (
    id uuid DEFAULT uuidv7() NOT NULL,
    library_id uuid NOT NULL,
    event_type character varying(20) NOT NULL,
    file_path text NOT NULL,
    old_path text,
    file_size bigint,
    detected_at timestamp with time zone DEFAULT now() NOT NULL,
    processed boolean DEFAULT false NOT NULL,
    processed_at timestamp with time zone,
    processing_attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    CONSTRAINT file_watch_events_pkey PRIMARY KEY (id),
    CONSTRAINT file_watch_events_event_type_check CHECK (((event_type)::text = ANY ((ARRAY['created'::character varying, 'modified'::character varying, 'deleted'::character varying, 'moved'::character varying])::text[]))),
    CONSTRAINT valid_move_event CHECK (((((event_type)::text = 'moved'::text) AND (old_path IS NOT NULL)) OR (((event_type)::text <> 'moved'::text) AND (old_path IS NULL))))
);

-- Name: TABLE file_watch_events; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.file_watch_events IS 'Queue of filesystem events detected by file watcher';

-- Name: COLUMN file_watch_events.event_type; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.file_watch_events.event_type IS 'Type of filesystem event detected';

-- Name: folder_inventory; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.folder_inventory (
    id uuid DEFAULT uuidv7() NOT NULL,
    library_id uuid NOT NULL,
    folder_path text NOT NULL,
    folder_type character varying(50) NOT NULL,
    parent_folder_id uuid,
    discovered_at timestamp with time zone DEFAULT now() NOT NULL,
    last_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    discovery_source character varying(50) DEFAULT 'scan'::character varying NOT NULL,
    processing_status character varying(50) DEFAULT 'pending'::character varying NOT NULL,
    last_processed_at timestamp with time zone,
    processing_error text,
    processing_attempts integer DEFAULT 0 NOT NULL,
    next_retry_at timestamp with time zone,
    total_files integer DEFAULT 0 NOT NULL,
    processed_files integer DEFAULT 0 NOT NULL,
    total_size_bytes bigint DEFAULT 0 NOT NULL,
    file_types jsonb DEFAULT '[]'::jsonb,
    last_modified timestamp with time zone,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT folder_inventory_discovery_source_check CHECK (((discovery_source)::text = ANY ((ARRAY['scan'::character varying, 'watch'::character varying, 'manual'::character varying, 'import'::character varying])::text[]))),
    CONSTRAINT folder_inventory_folder_type_check CHECK (((folder_type)::text = ANY ((ARRAY['root'::character varying, 'movie'::character varying, 'tv_show'::character varying, 'season'::character varying, 'extra'::character varying, 'unknown'::character varying])::text[]))),
    CONSTRAINT folder_inventory_processing_status_check CHECK (((processing_status)::text = ANY ((ARRAY['pending'::character varying, 'processing'::character varying, 'completed'::character varying, 'failed'::character varying, 'skipped'::character varying, 'queued'::character varying])::text[]))),
    CONSTRAINT valid_file_counts CHECK ((processed_files <= total_files)),
    CONSTRAINT valid_parent_relationship CHECK ((id <> parent_folder_id)),
    CONSTRAINT unique_library_folder_path UNIQUE (library_id, folder_path),
    CONSTRAINT folder_inventory_pkey PRIMARY KEY (id)
);

-- Name: TABLE folder_inventory; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.folder_inventory IS 'Tracks discovered folders in media libraries for efficient scanning and processing';

-- Name: COLUMN folder_inventory.folder_type; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.folder_inventory.folder_type IS 'Type of content in folder: root, movie, tv_show, season, extra, or unknown';

-- Name: COLUMN folder_inventory.discovery_source; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.folder_inventory.discovery_source IS 'How the folder was discovered: scan, watch (file watcher), manual, or import';

-- Name: COLUMN folder_inventory.processing_status; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.folder_inventory.processing_status IS 'Current processing state: pending, processing, completed, failed, skipped, or queued';

-- Name: COLUMN folder_inventory.total_size_bytes; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.folder_inventory.total_size_bytes IS 'Total size of all files in the folder in bytes';

-- Name: COLUMN folder_inventory.file_types; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.folder_inventory.file_types IS 'JSON array of file extensions found in the folder, e.g., ["mp4", "mkv", "srt"]';

-- Name: COLUMN folder_inventory.last_modified; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.folder_inventory.last_modified IS 'Filesystem last modified timestamp for the folder';

-- Name: COLUMN folder_inventory.metadata; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.folder_inventory.metadata IS 'Flexible JSON storage for additional folder metadata like permissions, attributes, etc.';

CREATE TYPE public.size_variant AS ENUM ('original', 'resized', 'tmdb');
CREATE TYPE public.image_variant AS ENUM ('poster', 'backdrop', 'thumbnail', 'profile');
CREATE TYPE public.media_type AS ENUM ('movie', 'series', 'season', 'episode', 'person');

-- Name: media_files; Type: TABLE; Schema: public; Owner: postgres
 CREATE TABLE public.media_files (
     id uuid PRIMARY KEY DEFAULT uuidv7(),
     library_id uuid NOT NULL,
     media_id uuid NOT NULL,
     media_type media_type NOT NULL CHECK (media_type = 'movie' OR media_type = 'episode'), -- Only movies and episodes can have associated media files
     file_path text NOT NULL UNIQUE,
     filename character varying(1000) NOT NULL,
     file_size bigint NOT NULL,
     -- When the row/file was first discovered by the scanner
     discovered_at timestamp with time zone DEFAULT now() NOT NULL,
     -- Filesystem or content creation timestamp
     created_at timestamp with time zone DEFAULT now() NOT NULL,
     updated_at timestamp with time zone DEFAULT now() NOT NULL,
     technical_metadata jsonb,
     parsed_info jsonb
 );

-- Name: tmdb_image_variants; Type: TABLE; Schema: public; Owner: postgres
 CREATE TABLE public.tmdb_image_variants (
     id uuid PRIMARY KEY DEFAULT uuidv7(),
     image_variant image_variant NOT NULL,
     tmdb_path character varying(48) NOT NULL UNIQUE,
     media_id uuid NOT NULL,
     media_type media_type NOT NULL,
     width smallint NOT NULL,
     height smallint NOT NULL,
     iso_lang character varying(3),
     vote_avg float NOT NULL,
     vote_cnt integer NOT NULL,
     is_primary boolean DEFAULT false
 );

-- Name: cached_images; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.cached_images (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    image_id uuid NOT NULL REFERENCES tmdb_image_variants (id),
    image_variant image_variant NOT NULL,
    width smallint NOT NULL,
    height smallint NOT NULL,
    size_variant size_variant NOT NULL,
    theme_color character varying(7),
    cache_key character varying(256) NOT NULL UNIQUE,
    integrity character varying(256) NOT NULL,
    byte_len integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    modified_at timestamp with time zone DEFAULT now() NOT NULL,
    UNIQUE (image_id, width),
    CONSTRAINT non_zero_width_and_height CHECK (width <> 0 AND height <> 0)
);

CREATE UNIQUE INDEX only_one_original ON public.cached_images (image_id)
WHERE size_variant = 'original';

CREATE OR REPLACE VIEW view_tmdb_image_variants_cached AS
SELECT iv.*,
       EXISTS (
           SELECT 1 FROM public.cached_images ci
           WHERE ci.image_id = iv.id
       ) AS cached
FROM public.tmdb_image_variants iv;

CREATE OR REPLACE FUNCTION public.update_modified_at()
RETURNS trigger AS $$
BEGIN
    IF ROW(NEW.*) IS DISTINCT FROM ROW(OLD.*) THEN
        NEW.modified_at := now();
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_cached_image_modified
BEFORE UPDATE ON public.cached_images
FOR EACH ROW
EXECUTE FUNCTION public.update_modified_at();

-- Name: tmdb_idx_image_variants_image_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX cached_images_image_id_idx ON public.cached_images USING btree (image_id, modified_at DESC);

-- Name: tmdb_idx_image_variants_image_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX cached_images_width_idx ON public.cached_images USING btree (image_id, width, modified_at DESC);

-- Name: idx_images_hash; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX cached_images_cache_key_idx ON public.cached_images USING btree (cache_key);

-- Name: cached_images_original_idx; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX cached_images_original_idx
ON public.cached_images (image_id, modified_at DESC)
WHERE size_variant = 'original';

-- Name: cached_images_resized_idx; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX cached_images_resized_idx
ON public.cached_images (image_id, width, modified_at DESC)
WHERE size_variant = 'resized';

-- Name: idx_images_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX cached_images_modified_at_idx ON public.cached_images USING btree (modified_at);

-- Name: jwt_blacklist; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.jwt_blacklist (
    jti character varying(255) NOT NULL,
    user_id uuid NOT NULL,
    revoked_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked_reason character varying(255),
    CONSTRAINT jwt_blacklist_valid_window CHECK ((expires_at >= revoked_at)),
    CONSTRAINT jwt_blacklist_pkey PRIMARY KEY (jti)
);

-- Name: libraries; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.libraries (
    id uuid DEFAULT uuidv7() NOT NULL,
    name character varying(255) NOT NULL,
    library_type character varying(20) NOT NULL,
    paths text[] NOT NULL,
    scan_interval_minutes integer DEFAULT 60 NOT NULL,
    last_scan timestamp with time zone,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    auto_scan boolean DEFAULT true NOT NULL,
    watch_for_changes boolean DEFAULT true NOT NULL,
    analyze_on_scan boolean DEFAULT false NOT NULL,
    max_retry_attempts integer DEFAULT 3 NOT NULL,
    -- Movie reference batching: fixed per-library batch size for movie reference ingestion. This is immutable after the first movie reference is created for the library.
    movie_ref_batch_size integer DEFAULT 250 NOT NULL,
    CONSTRAINT libraries_pkey PRIMARY KEY (id),
    CONSTRAINT libraries_movie_ref_batch_size_check CHECK ((movie_ref_batch_size > 0)),
    CONSTRAINT libraries_library_type_check CHECK (((library_type)::text = ANY ((ARRAY['movies'::character varying, 'tvshows'::character varying])::text[]))),
    CONSTRAINT libraries_name_key UNIQUE (name)
);

-- Name: library_sorted_indices; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.library_sorted_indices (
    id uuid DEFAULT uuidv7() NOT NULL,
    library_id uuid NOT NULL,
    sort_field character varying(50) NOT NULL,
    sort_order character varying(10) NOT NULL,
    media_ids uuid[] NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    last_updated timestamp with time zone DEFAULT now() NOT NULL,
    version integer DEFAULT 1 NOT NULL,
    CONSTRAINT library_sorted_indices_sort_order_check CHECK (((sort_order)::text = ANY ((ARRAY['ascending'::character varying, 'descending'::character varying])::text[]))),
    CONSTRAINT library_sorted_indices_pkey PRIMARY KEY (id),
    CONSTRAINT library_sorted_indices_library_id_sort_field_sort_order_met_key UNIQUE (library_id, sort_field, sort_order, metadata)
);

-- Name: TABLE library_sorted_indices; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.library_sorted_indices IS 'Stores pre-sorted media IDs for efficient client-side sorting';

-- Name: COLUMN library_sorted_indices.metadata; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.library_sorted_indices.metadata IS 'Additional context like user_id for user-specific sorts (LastWatched, WatchProgress)';

-- Durable consumer offsets for file change event bus Per-consumer-group, per-library cursor; supports at-least-once delivery and multiple consumers without coupling to processed flags on events.
CREATE TABLE public.file_watch_consumer_offsets (
    group_name text NOT NULL,
    library_id uuid NOT NULL,
    last_event_id uuid NULL,
    last_detected_at timestamptz NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT file_watch_consumer_offsets_pkey PRIMARY KEY (group_name, library_id),
    CONSTRAINT fk_fwco_library FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE,
    CONSTRAINT fk_fwco_last_event FOREIGN KEY (last_event_id) REFERENCES public.file_watch_events(id) ON DELETE SET NULL
);

COMMENT ON TABLE public.file_watch_consumer_offsets IS 'Durable per-group, per-library offsets for file change event streaming';
COMMENT ON COLUMN public.file_watch_consumer_offsets.group_name IS 'Consumer group name (logical subscriber id)';
COMMENT ON COLUMN public.file_watch_consumer_offsets.last_event_id IS 'Last acknowledged event id for this group and library';
COMMENT ON COLUMN public.file_watch_consumer_offsets.last_detected_at IS 'Detected-at timestamp of the last acknowledged event';

-- Name: login_attempts; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.login_attempts (
    id uuid DEFAULT uuidv7() NOT NULL,
    ip_address inet NOT NULL,
    username character varying(50),
    attempted_at timestamp with time zone DEFAULT now(),
    success boolean NOT NULL,
    CONSTRAINT login_attempts_pkey PRIMARY KEY (id)
);

-- Name: media_processing_status; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.media_processing_status (
    media_file_id uuid NOT NULL,
    metadata_extracted boolean DEFAULT false NOT NULL,
    metadata_extracted_at timestamp with time zone,
    tmdb_matched boolean DEFAULT false NOT NULL,
    tmdb_matched_at timestamp with time zone,
    images_cached boolean DEFAULT false NOT NULL,
    images_cached_at timestamp with time zone,
    file_analyzed boolean DEFAULT false NOT NULL,
    file_analyzed_at timestamp with time zone,
    last_error text,
    error_details jsonb,
    retry_count integer DEFAULT 0 NOT NULL,
    next_retry_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT media_processing_status_pkey PRIMARY KEY (media_file_id)
);

-- Name: TABLE media_processing_status; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.media_processing_status IS 'Tracks processing status for each media file to enable incremental scanning';

-- Name: COLUMN media_processing_status.file_analyzed; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.media_processing_status.file_analyzed IS 'Whether advanced analysis (thumbnails, previews) has been performed';

-- Name: movie_alternative_titles; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_alternative_titles (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    iso_3166_1 text,
    title text NOT NULL,
    title_type text,
    id uuid DEFAULT uuidv7() NOT NULL,
    CONSTRAINT movie_alternative_titles_primary PRIMARY KEY (id)
);

-- Name: movie_collection_membership; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_collection_membership (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    collection_id bigint NOT NULL,
    name text NOT NULL,
    poster_path text,
    backdrop_path text,
    CONSTRAINT movie_collection_membership_pkey PRIMARY KEY (movie_id)
);

-- Name: movie_genres; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_genres (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    genre_id bigint NOT NULL,
    name text NOT NULL,
    CONSTRAINT movie_genres_pkey PRIMARY KEY (movie_id, genre_id)
);

-- Name: movie_keywords; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_keywords (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    keyword_id bigint NOT NULL,
    name text NOT NULL,
    CONSTRAINT movie_keywords_pkey PRIMARY KEY (movie_id, keyword_id)
);

-- Name: movie_metadata; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_metadata (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    tmdb_id bigint NOT NULL,
    title text NOT NULL,
    original_title text,
    overview text,
    release_date date,
    runtime integer,
    vote_average real,
    vote_count integer,
    popularity real,
    primary_certification text,
    homepage text,
    status text,
    tagline text,
    budget bigint,
    revenue bigint,
    poster_path text,
    backdrop_path text,
    primary_poster_image_id uuid NOT NULL,
    primary_backdrop_image_id uuid,
    logo_path text,
    collection_id bigint,
    collection_name text,
    collection_poster_path text,
    collection_backdrop_path text,
    imdb_id text,
    facebook_id text,
    instagram_id text,
    twitter_id text,
    wikidata_id text,
    tiktok_id text,
    youtube_id text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT movie_metadata_pkey PRIMARY KEY (movie_id)
);

-- Name: movie_production_companies; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_production_companies (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    company_id bigint,
    name text NOT NULL,
    origin_country text,
    CONSTRAINT movie_production_companies_pkey PRIMARY KEY (movie_id, name)
);

-- Name: movie_production_countries; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_production_countries (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    iso_3166_1 text NOT NULL,
    name text NOT NULL,
    CONSTRAINT movie_production_countries_pkey PRIMARY KEY (movie_id, iso_3166_1)
);

-- Name: movie_recommendations; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_recommendations (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    recommended_tmdb_id bigint NOT NULL,
    title text,
    CONSTRAINT movie_recommendations_pkey PRIMARY KEY (movie_id, recommended_tmdb_id)
);

-- Name: movie_references; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_references (
    id uuid DEFAULT uuidv7() CONSTRAINT movie_references_pkey PRIMARY KEY,
    library_id uuid NOT NULL,
    file_id uuid NOT NULL,
    tmdb_id bigint NOT NULL,
    title character varying(1000) NOT NULL,
    -- Per-library monotonically increasing batch id (assigned automatically)
    batch_id bigint NOT NULL,
    -- When this reference was discovered/created in DB
    discovered_at timestamp with time zone DEFAULT now() NOT NULL,
    -- Optional content creation timestamp
    created_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    theme_color character varying(7),
    CONSTRAINT movie_references_id_library_id_batch_id_key UNIQUE (id, library_id, batch_id),
    CONSTRAINT movie_references_tmdb_id_library_id_key UNIQUE (tmdb_id, library_id)
);

-- Name: movie_reference_batches; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_reference_batches (
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    batch_size integer NOT NULL,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    finalized_at timestamp with time zone,
    updated_at timestamp with time zone NOT NULL DEFAULT now(),
    version bigint NOT NULL DEFAULT 1,
    batch_hash NUMERIC (64),
    CONSTRAINT movie_reference_batches_pkey PRIMARY KEY (library_id, batch_id),
    CONSTRAINT movie_reference_batches_batch_size_check CHECK ((batch_size > 0)),
    CONSTRAINT positive_batch_hash CHECK (batch_hash > 0),
    CONSTRAINT positive_batch_version CHECK (version > 0)
);

-- Name: movie_reference_batch_cursor; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_reference_batch_cursor (
    library_id uuid NOT NULL,
    current_batch_id bigint NOT NULL,
    current_count integer NOT NULL,
    batch_size integer NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT movie_reference_batch_cursor_pkey PRIMARY KEY (library_id),
    CONSTRAINT movie_reference_batch_cursor_batch_size_check CHECK ((batch_size > 0)),
    CONSTRAINT movie_reference_batch_cursor_current_count_check CHECK ((current_count >= 0))
);

-- Name: series; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series (
    id uuid DEFAULT uuidv7() NOT NULL CONSTRAINT series_pkey PRIMARY KEY,
    library_id uuid NOT NULL,
    tmdb_id bigint,
    title character varying(1000) NOT NULL,
    -- When this series reference was discovered/created in DB
    discovered_at timestamp with time zone DEFAULT now() NOT NULL,
    -- Optional content creation timestamp (e.g. folder creation date)
    created_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    theme_color character varying(7)
);

-- Name: movie_release_dates; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_release_dates (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    iso_3166_1 text NOT NULL,
    iso_639_1 text,
    certification text,
    release_date timestamp with time zone NOT NULL,
    release_type smallint NOT NULL,
    note text,
    descriptors text[] DEFAULT ARRAY[]::text[],
    CONSTRAINT movie_release_dates_pkey PRIMARY KEY (movie_id, iso_3166_1, release_type, release_date)
);

-- Name: movie_similar; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_similar (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    similar_tmdb_id bigint NOT NULL,
    title text,
    CONSTRAINT movie_similar_pkey PRIMARY KEY (movie_id, similar_tmdb_id)
);

-- Name: movie_sort_positions; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_sort_positions (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    title_pos integer NOT NULL,
    title_pos_desc integer NOT NULL,
    date_added_pos integer NOT NULL,
    date_added_pos_desc integer NOT NULL,
    created_at_pos integer NOT NULL,
    created_at_pos_desc integer NOT NULL,
    release_date_pos integer NOT NULL,
    release_date_pos_desc integer NOT NULL,
    rating_pos integer NOT NULL,
    rating_pos_desc integer NOT NULL,
    runtime_pos integer NOT NULL,
    runtime_pos_desc integer NOT NULL,
    popularity_pos integer NOT NULL,
    popularity_pos_desc integer NOT NULL,
    bitrate_pos integer NOT NULL,
    bitrate_pos_desc integer NOT NULL,
    file_size_pos integer NOT NULL,
    file_size_pos_desc integer NOT NULL,
    content_rating_pos integer NOT NULL,
    content_rating_pos_desc integer NOT NULL,
    resolution_pos integer NOT NULL,
    resolution_pos_desc integer NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT movie_sort_positions_pkey PRIMARY KEY (movie_id)
);

-- Name: TABLE movie_sort_positions; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.movie_sort_positions IS 'Precomputed per-library ranks for all movie sort dimensions';

-- Name: movie_spoken_languages; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_spoken_languages (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    iso_639_1 text,
    name text NOT NULL,
    CONSTRAINT movie_spoken_languages_pkey PRIMARY KEY (movie_id, name)
);

-- Name: movie_translations; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_translations (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    iso_3166_1 text NOT NULL,
    iso_639_1 text NOT NULL,
    name text,
    english_name text,
    title text,
    overview text,
    homepage text,
    tagline text,
    CONSTRAINT movie_translations_pkey PRIMARY KEY (movie_id, iso_3166_1, iso_639_1)
);

-- Name: movie_videos; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_videos (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    video_key text NOT NULL,
    site text NOT NULL,
    name text,
    video_type text,
    official boolean,
    iso_639_1 text,
    iso_3166_1 text,
    published_at timestamp with time zone,
    size integer,
    CONSTRAINT movie_videos_pkey PRIMARY KEY (movie_id, video_key, site)
);

-- Name: orchestrator_jobs; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.orchestrator_jobs (
    id uuid DEFAULT uuidv7() NOT NULL,
    library_id uuid NOT NULL,
    kind smallint NOT NULL,
    payload jsonb NOT NULL,
    priority smallint NOT NULL,
    state character varying(20) NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    available_at timestamp with time zone DEFAULT now() NOT NULL,
    lease_owner text,
    lease_id uuid,
    lease_expires_at timestamp with time zone,
    dedupe_key text NOT NULL,
    dependency_key text,
    last_error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT orchestrator_jobs_kind_check CHECK (((kind >= 0) AND (kind <= 6))),
    CONSTRAINT orchestrator_jobs_priority_check CHECK (((priority >= 0) AND (priority <= 3))),
    CONSTRAINT orchestrator_jobs_state_check CHECK (((state)::text = ANY ((ARRAY['ready'::character varying, 'deferred'::character varying, 'leased'::character varying, 'completed'::character varying, 'failed'::character varying, 'dead_letter'::character varying])::text[]))),
    CONSTRAINT orchestrator_jobs_pkey PRIMARY KEY (id)
);

-- Name: series_scan_state; Type: TABLE; Schema: public; Owner: postgres
CREATE TYPE public.series_scan_status AS ENUM (
    'discovered',
    'seeded',
    'resolved',
    'failed'
);

CREATE TABLE public.series_scan_state (
    library_id uuid NOT NULL,
    series_root_path text NOT NULL,
    status public.series_scan_status NOT NULL,
    series_id uuid,
    series_title text,
    series_slug text,
    series_year smallint,
    series_region text,
    seeded_at timestamp with time zone,
    last_attempt_at timestamp with time zone,
    attempts integer DEFAULT 0 NOT NULL,
    resolved_at timestamp with time zone,
    failed_at timestamp with time zone,
    failure_reason text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT series_scan_state_pkey PRIMARY KEY (library_id, series_root_path)
);

CREATE TABLE public.series_bundle_versioning (
    library_id uuid NOT NULL REFERENCES public.libraries (id) ON DELETE CASCADE,
    series_id uuid NOT NULL UNIQUE REFERENCES public.series (id) ON DELETE CASCADE,
    finalized boolean NOT NULL DEFAULT false,
    version bigint NOT NULL DEFAULT 1,
    updated_at timestamp with time zone NOT NULL DEFAULT now(),
    bundle_hash NUMERIC (64),
    CONSTRAINT lib_id_series_id_pkey PRIMARY KEY (library_id, series_id),
    CONSTRAINT positive_bundle_version CHECK (version > 0)
);

CREATE INDEX idx_library_id_series_id ON public.series_bundle_versioning USING btree(library_id, series_id);

-- Name: password_reset_tokens; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.password_reset_tokens (
    token character varying(255) NOT NULL,
    user_id uuid NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now(),
    used_at timestamp with time zone,
    CONSTRAINT password_reset_tokens_pkey PRIMARY KEY (token)
);

-- Name: permissions; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.permissions (
    id uuid DEFAULT uuidv7() NOT NULL,
    name character varying(100) NOT NULL,
    category character varying(50) NOT NULL,
    description text,
    created_at timestamp with time zone DEFAULT now(),
    CONSTRAINT permissions_pkey PRIMARY KEY (id),
    CONSTRAINT permissions_name_key UNIQUE (name)
);

-- Name: TABLE permissions; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.permissions IS 'Granular permissions that can be assigned to roles';

-- Name: persons; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.persons (
    id uuid DEFAULT uuidv7() UNIQUE NOT NULL,
    tmdb_id bigint UNIQUE NOT NULL,
    name text NOT NULL,
    original_name text,
    gender smallint,
    known_for_department text,
    profile_path text,
    adult boolean,
    popularity real,
    biography text,
    birthday date,
    deathday date,
    place_of_birth text,
    homepage text,
    imdb_id text,
    facebook_id text,
    instagram_id text,
    twitter_id text,
    wikidata_id text,
    tiktok_id text,
    youtube_id text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT persons_pkey PRIMARY KEY (id)
);

-- Name: person_aliases; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.person_aliases (
    tmdb_id bigint NOT NULL,
    person_id uuid NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    alias text NOT NULL,
    CONSTRAINT person_aliases_pkey PRIMARY KEY (person_id, alias)
);

-- Name: movie_cast; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_cast (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    person_tmdb_id bigint NOT NULL,
    person_id uuid NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    credit_id text,
    cast_id bigint,
    "character" text NOT NULL,
    order_index integer,
    profile_image_id uuid,
    CONSTRAINT movie_cast_pkey PRIMARY KEY (movie_id, person_tmdb_id, "character")
);

CREATE INDEX movie_cast_person_id_idx ON public.movie_cast USING btree (person_id);

-- Name: series_cast; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_cast (
    series_id uuid NOT NULL,
    person_tmdb_id bigint NOT NULL,
    person_id uuid NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    credit_id text,
    "character" text NOT NULL,
    total_episode_count integer,
    order_index integer,
    profile_image_id uuid,
    CONSTRAINT series_cast_pkey PRIMARY KEY (series_id, person_tmdb_id, "character")
);

CREATE INDEX series_cast_person_id_idx ON public.series_cast USING btree (person_id);

-- Name: episode_cast; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_cast (
    episode_id uuid NOT NULL,
    person_tmdb_id bigint NOT NULL,
    person_id uuid NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    credit_id text,
    "character" text NOT NULL,
    order_index integer,
    profile_image_id uuid,
    CONSTRAINT episode_cast_pkey PRIMARY KEY (episode_id, person_tmdb_id, "character")
);

CREATE INDEX episode_cast_person_id_idx ON public.episode_cast USING btree (person_id);

-- Name: episode_guest_stars; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_guest_stars (
    episode_id uuid NOT NULL,
    person_tmdb_id bigint NOT NULL,
    person_id uuid NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    credit_id text,
    "character" text NOT NULL,
    order_index integer,
    profile_image_id uuid,
    CONSTRAINT episode_guest_stars_pkey PRIMARY KEY (episode_id, person_tmdb_id, "character")
);

CREATE INDEX episode_guest_stars_person_id_idx ON public.episode_guest_stars USING btree (person_id);

-- Name: movie_cast movie_cast_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_cast
    ADD CONSTRAINT movie_cast_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_cast movie_cast_person_tmdb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_cast
    ADD CONSTRAINT movie_cast_person_tmdb_id_fkey FOREIGN KEY (person_tmdb_id) REFERENCES public.persons(tmdb_id) ON DELETE CASCADE;

-- Name: movie_cast movie_cast_profile_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_cast
    ADD CONSTRAINT movie_cast_profile_image_id_fkey FOREIGN KEY (profile_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: series_cast series_cast_person_tmdb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_cast
    ADD CONSTRAINT series_cast_person_tmdb_id_fkey FOREIGN KEY (person_tmdb_id) REFERENCES public.persons(tmdb_id) ON DELETE CASCADE;

-- Name: series_cast series_cast_profile_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_cast
    ADD CONSTRAINT series_cast_profile_image_id_fkey FOREIGN KEY (profile_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: series_cast series_cast_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_cast
    ADD CONSTRAINT series_cast_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: episode_cast episode_cast_episode_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_cast
    ADD CONSTRAINT episode_cast_episode_id_fkey FOREIGN KEY (episode_id) REFERENCES public.episode_references(id) ON DELETE CASCADE;

-- Name: episode_cast episode_cast_person_tmdb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_cast
    ADD CONSTRAINT episode_cast_person_tmdb_id_fkey FOREIGN KEY (person_tmdb_id) REFERENCES public.persons(tmdb_id) ON DELETE CASCADE;

-- Name: episode_cast episode_cast_profile_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_cast
    ADD CONSTRAINT episode_cast_profile_image_id_fkey FOREIGN KEY (profile_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: episode_guest_stars episode_guest_stars_episode_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_guest_stars
    ADD CONSTRAINT episode_guest_stars_episode_id_fkey FOREIGN KEY (episode_id) REFERENCES public.episode_references(id) ON DELETE CASCADE;

-- Name: episode_guest_stars episode_guest_stars_person_tmdb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_guest_stars
    ADD CONSTRAINT episode_guest_stars_person_tmdb_id_fkey FOREIGN KEY (person_tmdb_id) REFERENCES public.persons(tmdb_id) ON DELETE CASCADE;

-- Name: episode_guest_stars episode_guest_stars_profile_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_guest_stars
    ADD CONSTRAINT episode_guest_stars_profile_image_id_fkey FOREIGN KEY (profile_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: movie_crew; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.movie_crew (
    movie_id uuid NOT NULL,
    library_id uuid NOT NULL,
    batch_id bigint NOT NULL,
    person_tmdb_id bigint NOT NULL,
    person_id uuid NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    credit_id text,
    department text NOT NULL,
    job text NOT NULL,
    CONSTRAINT movie_crew_pkey PRIMARY KEY (movie_id, person_tmdb_id, department, job)
);

CREATE INDEX movie_crew_person_id_idx ON public.movie_crew USING btree (person_id);

-- Name: series_crew; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_crew (
    series_id uuid NOT NULL,
    person_tmdb_id bigint NOT NULL,
    person_id uuid NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    credit_id text,
    department text NOT NULL,
    job text NOT NULL,
    CONSTRAINT series_crew_pkey PRIMARY KEY (series_id, person_tmdb_id, department, job)
);

CREATE INDEX series_crew_person_id_idx ON public.series_crew USING btree (person_id);

-- Name: episode_crew; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.episode_crew (
    episode_id uuid NOT NULL,
    person_tmdb_id bigint NOT NULL,
    person_id uuid NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    credit_id text,
    department text NOT NULL,
    job text NOT NULL,
    CONSTRAINT episode_crew_pkey PRIMARY KEY (episode_id, person_tmdb_id, department, job)
);

CREATE INDEX episode_crew_person_id_idx ON public.episode_crew USING btree (person_id);

-- Name: movie_crew movie_crew_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_crew
    ADD CONSTRAINT movie_crew_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_crew movie_crew_person_tmdb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_crew
    ADD CONSTRAINT movie_crew_person_tmdb_id_fkey FOREIGN KEY (person_tmdb_id) REFERENCES public.persons(tmdb_id) ON DELETE CASCADE;

-- Name: series_crew series_crew_person_tmdb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_crew
    ADD CONSTRAINT series_crew_person_tmdb_id_fkey FOREIGN KEY (person_tmdb_id) REFERENCES public.persons(tmdb_id) ON DELETE CASCADE;

-- Name: series_crew series_crew_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_crew
    ADD CONSTRAINT series_crew_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: episode_crew episode_crew_episode_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_crew
    ADD CONSTRAINT episode_crew_episode_id_fkey FOREIGN KEY (episode_id) REFERENCES public.episode_references(id) ON DELETE CASCADE;

-- Name: episode_crew episode_crew_person_tmdb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_crew
    ADD CONSTRAINT episode_crew_person_tmdb_id_fkey FOREIGN KEY (person_tmdb_id) REFERENCES public.persons(tmdb_id) ON DELETE CASCADE;

-- Name: rate_limit_state; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.rate_limit_state (
    id uuid DEFAULT uuidv7() NOT NULL,
    key text NOT NULL,
    endpoint text NOT NULL,
    request_count integer DEFAULT 0 NOT NULL,
    window_start timestamp with time zone DEFAULT now() NOT NULL,
    violation_count integer DEFAULT 0 NOT NULL,
    blocked_until timestamp with time zone,
    last_request timestamp with time zone DEFAULT now() NOT NULL,
    metadata jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT rate_limit_state_pkey PRIMARY KEY (id),
    CONSTRAINT rate_limit_state_key_endpoint_key UNIQUE (key, endpoint)
);

-- Name: TABLE rate_limit_state; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.rate_limit_state IS 'Persistent state for distributed rate limiting';

-- Name: auth_refresh_tokens; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.auth_refresh_tokens (
    id uuid DEFAULT uuidv7() PRIMARY KEY,
    token_hash text NOT NULL UNIQUE,
    user_id uuid NOT NULL,
    device_session_id uuid,
    session_id uuid,
    issued_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked boolean DEFAULT false NOT NULL,
    revoked_at timestamp with time zone,
    revoked_reason text,
    device_name text,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    family_id uuid,
    generation integer DEFAULT 1,
    used_at timestamp with time zone,
    used_count integer DEFAULT 0,
    origin_scope text DEFAULT 'full'::text NOT NULL,
    CONSTRAINT auth_refresh_tokens_token_hash_length CHECK ((char_length(token_hash) = 64)),
    CONSTRAINT auth_refresh_tokens_valid_window CHECK ((expires_at > issued_at)),
    CONSTRAINT auth_refresh_tokens_generation_positive CHECK ((generation >= 1)),
    CONSTRAINT auth_refresh_tokens_origin_scope_valid CHECK ((origin_scope = 'full'::text) OR (origin_scope = 'playback'::text))
);

-- Name: TABLE auth_refresh_tokens; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.auth_refresh_tokens IS 'Refresh token store with rotation metadata and hashed tokens';

-- Name: COLUMN auth_refresh_tokens.origin_scope; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_refresh_tokens.origin_scope IS 'Sticky origin scope for the refresh token (full or playback)';

-- Name: auth_device_challenges; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.auth_device_challenges (
    id uuid DEFAULT uuidv7() NOT NULL,
    device_session_id uuid NOT NULL,
    nonce bytea NOT NULL,
    issued_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    used boolean DEFAULT false NOT NULL
);

-- Name: TABLE auth_device_challenges; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.auth_device_challenges IS 'Ephemeral nonces for device possession challenges';

-- Name: COLUMN auth_device_challenges.nonce; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_challenges.nonce IS 'Opaque random nonce bytes to be signed by the device key';

-- Enforce minimum nonce length
ALTER TABLE public.auth_device_challenges
    ADD CONSTRAINT auth_device_challenges_nonce_min_len CHECK (octet_length(nonce) >= 32);

-- Name: COLUMN auth_refresh_tokens.token_hash; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_refresh_tokens.token_hash IS 'SHA256 hex-encoded hash of the refresh token';

-- Name: auth_security_settings; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.auth_security_settings (
    id uuid DEFAULT uuidv7() NOT NULL,
    admin_password_policy jsonb NOT NULL,
    user_password_policy jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_by uuid,
    CONSTRAINT auth_security_settings_pkey PRIMARY KEY (id)
);

-- Name: TABLE auth_security_settings; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.auth_security_settings IS 'Authentication policy settings allowing admins to opt into stricter password rules.';

-- Name: COLUMN auth_security_settings.admin_password_policy; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_security_settings.admin_password_policy IS 'JSON payload describing password policy for admin accounts (including first-run binding).';

-- Name: COLUMN auth_security_settings.user_password_policy; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_security_settings.user_password_policy IS 'JSON payload describing password policy for regular user accounts.';

-- Name: COLUMN auth_security_settings.updated_by; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_security_settings.updated_by IS 'Admin user who last changed the security settings (nullable during first run).';

-- Name: setup_claims; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.setup_claims (
    id uuid DEFAULT uuidv7() NOT NULL,
    code_hash character varying(64) NOT NULL,
    claim_token_hash character varying(64),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    confirmed_at timestamp with time zone,
    client_name text,
    client_ip inet,
    attempts integer DEFAULT 0 NOT NULL,
    last_attempt_at timestamp with time zone,
    revoked_at timestamp with time zone,
    revoked_reason text,
    CONSTRAINT setup_claims_pkey PRIMARY KEY (id)
);

-- Name: TABLE setup_claims; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.setup_claims IS 'One-time setup claim codes used to bind first-run setup to a LAN client.';

-- Name: COLUMN setup_claims.code_hash; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.code_hash IS 'HMAC-SHA-256 digest of the short claim code presented to the user.';

-- Name: COLUMN setup_claims.claim_token_hash; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.claim_token_hash IS 'HMAC-SHA-256 digest of the long-lived claim token returned after confirmation.';

-- Name: COLUMN setup_claims.expires_at; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.expires_at IS 'Expiration timestamp; codes become invalid after this moment even if unconfirmed.';

-- Name: COLUMN setup_claims.confirmed_at; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.confirmed_at IS 'Timestamp when the claim was successfully confirmed and a claim token issued.';

-- Name: COLUMN setup_claims.client_name; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.client_name IS 'Friendly label supplied by the client requesting the claim (e.g., device name).';

-- Name: COLUMN setup_claims.client_ip; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.client_ip IS 'IP address of the client that initiated the claim; used for LAN enforcement and auditing.';

-- Name: COLUMN setup_claims.attempts; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.attempts IS 'Number of confirmation attempts recorded for this claim.';

-- Name: COLUMN setup_claims.last_attempt_at; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.last_attempt_at IS 'Timestamp of the most recent confirmation attempt (successful or not).';

-- Name: COLUMN setup_claims.revoked_at; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.revoked_at IS 'Timestamp when an operator explicitly revoked the claim (via CLI).';

-- Name: COLUMN setup_claims.revoked_reason; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.setup_claims.revoked_reason IS 'Optional descriptive reason provided when revoking a claim.';

-- Name: role_permissions; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.role_permissions (
    role_id uuid NOT NULL,
    permission_id uuid NOT NULL,
    granted_at timestamp with time zone DEFAULT now(),
    CONSTRAINT role_permissions_pkey PRIMARY KEY (role_id, permission_id)
);

-- Name: TABLE role_permissions; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.role_permissions IS 'Maps permissions to roles';

-- Name: roles; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.roles (
    id uuid DEFAULT uuidv7() NOT NULL,
    name character varying(50) NOT NULL,
    description text,
    is_system boolean DEFAULT false,
    created_at timestamp with time zone DEFAULT now(),
    CONSTRAINT roles_pkey PRIMARY KEY (id),
    CONSTRAINT roles_name_key UNIQUE (name)
);

-- Name: TABLE roles; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.roles IS 'System and custom roles for access control';

-- Name: scan_cursors; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.scan_cursors (
    library_id uuid NOT NULL,
    path_hash bigint NOT NULL,
    folder_path_norm text NOT NULL,
    listing_hash text NOT NULL,
    entry_count integer DEFAULT 0 NOT NULL,
    last_scan_at timestamp with time zone DEFAULT now() NOT NULL,
    last_modified_at timestamp with time zone,
    device_id text,
    CONSTRAINT scan_cursors_pkey PRIMARY KEY (library_id, path_hash)
);

-- Name: TABLE scan_cursors; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.scan_cursors IS 'Persistent scan cursor per (library, folder) for incremental scanning';

-- Name: COLUMN scan_cursors.path_hash; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.scan_cursors.path_hash IS 'Deterministic hash of normalized path(s) (see ScanCursorId) used as part of the key';

-- Name: COLUMN scan_cursors.folder_path_norm; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.scan_cursors.folder_path_norm IS 'Normalized human-readable folder path for reference only (not unique)';

-- Name: COLUMN scan_cursors.listing_hash; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.scan_cursors.listing_hash IS 'Hash of directory listing (entries + mtimes) to detect changes';

-- Name: COLUMN scan_cursors.entry_count; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.scan_cursors.entry_count IS 'Number of entries included when listing_hash was computed';

-- Name: scan_state; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.scan_state (
    id uuid DEFAULT uuidv7() NOT NULL,
    library_id uuid NOT NULL,
    scan_type character varying(20) NOT NULL,
    status character varying(20) NOT NULL,
    total_folders integer DEFAULT 0,
    processed_folders integer DEFAULT 0,
    total_files integer DEFAULT 0,
    processed_files integer DEFAULT 0,
    current_path text,
    error_count integer DEFAULT 0,
    errors jsonb DEFAULT '[]'::jsonb,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    options jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT scan_state_scan_type_check CHECK (((scan_type)::text = ANY ((ARRAY['full'::character varying, 'incremental'::character varying, 'refresh_metadata'::character varying, 'analyze'::character varying])::text[]))),
    CONSTRAINT scan_state_status_check CHECK (((status)::text = ANY ((ARRAY['pending'::character varying, 'running'::character varying, 'paused'::character varying, 'completed'::character varying, 'failed'::character varying, 'cancelled'::character varying])::text[]))),
    CONSTRAINT valid_progress CHECK (((processed_folders <= total_folders) AND (processed_files <= total_files))),
    CONSTRAINT scan_state_pkey PRIMARY KEY (id)
);

-- Name: TABLE scan_state; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.scan_state IS 'Tracks the state of library scans for resumability and monitoring';

-- Name: COLUMN scan_state.scan_type; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.scan_state.scan_type IS 'Type of scan: full, incremental, refresh_metadata, or analyze';

-- Name: COLUMN scan_state.options; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.scan_state.options IS 'JSON object with scan options like {force_refresh: bool, skip_tmdb: bool, analyze_files: bool}';

-- Name: season_keywords; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.season_keywords (
    season_id uuid NOT NULL,
    keyword_id bigint NOT NULL,
    name text NOT NULL,
    CONSTRAINT season_keywords_pkey PRIMARY KEY (season_id, keyword_id)
);

-- Name: season_metadata; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.season_metadata (
    season_id uuid NOT NULL,
    tmdb_id bigint NOT NULL,
    series_tmdb_id bigint,
    name text,
    overview text,
    air_date date,
    episode_count integer,
    poster_path text,
    primary_poster_image_id uuid NOT NULL,
    runtime integer,
    vote_average real,
    vote_count integer,
    imdb_id text,
    facebook_id text,
    instagram_id text,
    twitter_id text,
    wikidata_id text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT season_metadata_pkey PRIMARY KEY (season_id)
);

-- Name: season_references; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.season_references (
    id uuid DEFAULT uuidv7() NOT NULL,
    series_id uuid NOT NULL,
    season_number smallint NOT NULL,
    tmdb_series_id bigint NOT NULL,
    -- When this season reference was discovered/created in DB
    discovered_at timestamp with time zone DEFAULT now() NOT NULL,
    -- Optional content creation timestamp (e.g. folder creation date)
    created_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    theme_color character varying(7),
    library_id uuid NOT NULL,
    CONSTRAINT season_references_pkey PRIMARY KEY (id),
    CONSTRAINT season_references_series_id_season_number_key UNIQUE (series_id, season_number)
);

-- Name: season_translations; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.season_translations (
    season_id uuid NOT NULL,
    iso_3166_1 text NOT NULL,
    iso_639_1 text NOT NULL,
    name text,
    english_name text,
    title text,
    overview text,
    homepage text,
    tagline text,
    CONSTRAINT season_translations_pkey PRIMARY KEY (season_id, iso_3166_1, iso_639_1)
);

-- Name: season_videos; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.season_videos (
    season_id uuid NOT NULL,
    video_key text NOT NULL,
    site text NOT NULL,
    name text,
    video_type text,
    official boolean,
    iso_639_1 text,
    iso_3166_1 text,
    published_at timestamp with time zone,
    size integer,
    CONSTRAINT season_videos_pkey PRIMARY KEY (season_id, video_key, site)
);

-- Name: security_audit_log; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.security_audit_log (
    id uuid DEFAULT uuidv7() NOT NULL,
    user_id uuid,
    device_session_id uuid,
    event_type text NOT NULL,
    severity text DEFAULT 'info'::text NOT NULL,
    event_data jsonb,
    ip_address inet,
    user_agent text,
    request_id uuid,
    success boolean DEFAULT true NOT NULL,
    error_message text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT security_audit_log_event_type_check CHECK ((event_type = ANY (ARRAY['login_success'::text, 'login_failed'::text, 'logout'::text, 'session_expired'::text, 'session_revoked'::text, 'device_registered'::text, 'device_trusted'::text, 'device_trust_revoked'::text, 'device_trust_expired'::text, 'device_removed'::text, 'pin_set'::text, 'pin_changed'::text, 'pin_auth_success'::text, 'pin_auth_failed'::text, 'pin_lockout'::text, 'token_refreshed'::text, 'token_revoked'::text, 'refresh_token_expired'::text, 'rate_limit_exceeded'::text, 'suspicious_activity'::text, 'user_created'::text, 'user_updated'::text, 'user_deleted'::text, 'password_changed'::text, 'role_changed'::text, 'security_settings_changed'::text, 'permissions_changed'::text]))),
    CONSTRAINT security_audit_log_severity_check CHECK ((severity = ANY (ARRAY['debug'::text, 'info'::text, 'warning'::text, 'error'::text, 'critical'::text]))),
    CONSTRAINT security_audit_log_pkey PRIMARY KEY (id)
);

-- Name: TABLE security_audit_log; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.security_audit_log IS 'Comprehensive security event tracking for audit and compliance';

-- Name: series_content_ratings; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_content_ratings (
    series_id uuid NOT NULL,
    iso_3166_1 text NOT NULL,
    rating text,
    rating_system text,
    descriptors text[] DEFAULT ARRAY[]::text[],
    CONSTRAINT series_content_ratings_pkey PRIMARY KEY (series_id, iso_3166_1)
);

-- Name: series_episode_groups; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_episode_groups (
    series_id uuid NOT NULL,
    group_id text NOT NULL,
    name text NOT NULL,
    description text,
    group_type text,
    CONSTRAINT series_episode_groups_pkey PRIMARY KEY (series_id, group_id)
);

-- Name: series_genres; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_genres (
    series_id uuid NOT NULL,
    genre_id bigint NOT NULL,
    name text NOT NULL,
    CONSTRAINT series_genres_pkey PRIMARY KEY (series_id, genre_id)
);

-- Name: series_keywords; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_keywords (
    series_id uuid NOT NULL,
    keyword_id bigint NOT NULL,
    name text NOT NULL,
    CONSTRAINT series_keywords_pkey PRIMARY KEY (series_id, keyword_id)
);

-- Name: series_metadata; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_metadata (
    series_id uuid NOT NULL,
    tmdb_id bigint NOT NULL,
    name text NOT NULL,
    original_name text,
    overview text,
    first_air_date date,
    last_air_date date,
    number_of_seasons integer,
    number_of_episodes integer,
    vote_average real,
    vote_count integer,
    popularity real,
    primary_content_rating text,
    homepage text,
    status text,
    tagline text,
    in_production boolean,
    poster_path text,
    backdrop_path text,
    primary_poster_image_id uuid NOT NULL,
    primary_backdrop_image_id uuid,
    logo_path text,
    imdb_id text,
    tvdb_id bigint,
    facebook_id text,
    instagram_id text,
    twitter_id text,
    wikidata_id text,
    tiktok_id text,
    youtube_id text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT series_metadata_pkey PRIMARY KEY (series_id)
);

-- Name: series_networks; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_networks (
    series_id uuid NOT NULL,
    network_id bigint NOT NULL,
    name text NOT NULL,
    origin_country text,
    CONSTRAINT series_networks_pkey PRIMARY KEY (series_id, network_id)
);

-- Name: series_origin_countries; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_origin_countries (
    series_id uuid NOT NULL,
    iso_3166_1 text NOT NULL,
    CONSTRAINT series_origin_countries_pkey PRIMARY KEY (series_id, iso_3166_1)
);

-- Name: series_production_companies; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_production_companies (
    series_id uuid NOT NULL,
    company_id bigint,
    name text NOT NULL,
    origin_country text,
    CONSTRAINT series_production_companies_pkey PRIMARY KEY (series_id, name)
);

-- Name: series_production_countries; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_production_countries (
    series_id uuid NOT NULL,
    iso_3166_1 text NOT NULL,
    name text NOT NULL,
    CONSTRAINT series_production_countries_pkey PRIMARY KEY (series_id, iso_3166_1)
);

-- Name: series_recommendations; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_recommendations (
    series_id uuid NOT NULL,
    recommended_tmdb_id bigint NOT NULL,
    title text,
    CONSTRAINT series_recommendations_pkey PRIMARY KEY (series_id, recommended_tmdb_id)
);

-- Name: series_similar; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_similar (
    series_id uuid NOT NULL,
    similar_tmdb_id bigint NOT NULL,
    title text,
    CONSTRAINT series_similar_pkey PRIMARY KEY (series_id, similar_tmdb_id)
);

-- Name: series_spoken_languages; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_spoken_languages (
    series_id uuid NOT NULL,
    iso_639_1 text,
    name text NOT NULL,
    CONSTRAINT series_spoken_languages_pkey PRIMARY KEY (series_id, name)
);

-- Name: series_translations; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_translations (
    series_id uuid NOT NULL,
    iso_3166_1 text NOT NULL,
    iso_639_1 text NOT NULL,
    name text,
    english_name text,
    title text,
    overview text,
    homepage text,
    tagline text,
    CONSTRAINT series_translations_pkey PRIMARY KEY (series_id, iso_3166_1, iso_639_1)
);

-- Name: series_videos; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.series_videos (
    series_id uuid NOT NULL,
    video_key text NOT NULL,
    site text NOT NULL,
    name text,
    video_type text,
    official boolean,
    iso_639_1 text,
    iso_3166_1 text,
    published_at timestamp with time zone,
    size integer,
    CONSTRAINT series_videos_pkey PRIMARY KEY (series_id, video_key, site)
);

-- Name: auth_sessions; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.auth_sessions (
    id uuid DEFAULT uuidv7() NOT NULL,
    user_id uuid NOT NULL,
    device_session_id uuid,
    scope text DEFAULT 'full'::text NOT NULL,
    session_token_hash text NOT NULL UNIQUE,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    last_activity timestamp with time zone DEFAULT now() NOT NULL,
    ip_address inet,
    user_agent text,
    revoked boolean DEFAULT false NOT NULL,
    revoked_at timestamp with time zone,
    revoked_reason text,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT auth_sessions_pkey PRIMARY KEY (id),
    CONSTRAINT auth_sessions_expires_after_created CHECK ((expires_at > created_at)),
    CONSTRAINT auth_sessions_token_hash_length CHECK ((char_length(session_token_hash) = 64)),
    CONSTRAINT auth_sessions_scope_valid CHECK ((scope = 'full'::text) OR (scope = 'playback'::text))
);

ALTER TABLE public.auth_sessions
    SET (fillfactor = 80);

-- Name: TABLE auth_sessions; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.auth_sessions IS 'Active authentication sessions keyed by hashed tokens';

-- Name: COLUMN auth_sessions.session_token_hash; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_sessions.session_token_hash IS 'SHA256 hex-encoded hash of the bearer session token';

-- Name: COLUMN auth_sessions.scope; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_sessions.scope IS 'Session scope controlling access level (full or playback)';

-- Name: COLUMN auth_sessions.last_activity; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_sessions.last_activity IS 'Last authenticated request timestamp for the session';

-- Backfill origin_scope for existing refresh tokens once auth_sessions exists
UPDATE public.auth_refresh_tokens art
SET origin_scope = asess.scope
FROM public.auth_sessions asess
WHERE art.session_id IS NOT NULL
  AND art.session_id = asess.id
  AND art.origin_scope <> asess.scope;

-- Name: sync_participants; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.sync_participants (
    session_id uuid NOT NULL,
    user_id uuid NOT NULL,
    joined_at timestamp with time zone DEFAULT now(),
    last_ping timestamp with time zone DEFAULT now(),
    is_ready boolean DEFAULT false,
    latency_ms integer DEFAULT 0,
    CONSTRAINT sync_participants_pkey PRIMARY KEY (session_id, user_id)
);

-- Name: sync_session_history; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.sync_session_history (
    id uuid DEFAULT uuidv7() CONSTRAINT sync_session_history_pkey PRIMARY KEY,
    session_id uuid NOT NULL,
    event_type character varying(50) NOT NULL,
    event_data jsonb,
    user_id uuid,
    created_at timestamp with time zone DEFAULT now()
);

-- Name: sync_sessions; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.sync_sessions (
    id uuid DEFAULT uuidv7() CONSTRAINT sync_sessions_pkey PRIMARY KEY,
    room_code character varying(6) NOT NULL UNIQUE,
    host_id uuid NOT NULL,
    playback_state jsonb DEFAULT '{"position": 0, "is_playing": false, "playback_rate": 1.0}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now(),
    expires_at timestamp with time zone NOT NULL,
    is_active boolean DEFAULT true,
    media_uuid uuid NOT NULL,
    media_type smallint NOT NULL,
    CONSTRAINT check_media_type CHECK ((media_type = ANY (ARRAY[0, 1, 2, 3])))
);

-- Name: TABLE sync_sessions; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.sync_sessions IS 'Sync sessions now use UUID + media_type (u8) instead of MediaID JSONB';

-- Name: user_completed_media; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.user_completed_media (
    user_id uuid NOT NULL,
    completed_at bigint NOT NULL,
    media_uuid uuid NOT NULL,
    media_type smallint NOT NULL,
    CONSTRAINT user_completed_media_pkey PRIMARY KEY (user_id, media_uuid),
    CONSTRAINT check_media_type CHECK ((media_type = ANY (ARRAY[0, 1, 2, 3])))
);

-- Name: TABLE user_completed_media; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.user_completed_media IS 'Tracks completed media using UUID + media_type (u8) instead of MediaID JSONB';

-- Name: user_credentials; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.user_credentials (
    user_id uuid NOT NULL CONSTRAINT user_credentials_pkey PRIMARY KEY,
    password_hash character varying(255) NOT NULL,
    pin_hash text,
    pin_client_salt bytea DEFAULT public.gen_random_bytes(16) NOT NULL,
    pin_updated_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

-- Name: user_permissions; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.user_permissions (
    user_id uuid NOT NULL,
    permission_id uuid NOT NULL,
    granted boolean DEFAULT true NOT NULL,
    granted_by uuid,
    granted_at timestamp with time zone DEFAULT now(),
    reason text,
    CONSTRAINT user_permissions_pkey PRIMARY KEY (user_id, permission_id)
);

-- Name: TABLE user_permissions; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.user_permissions IS 'Per-user permission overrides (optional)';

-- Name: user_roles; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.user_roles (
    user_id uuid NOT NULL,
    role_id uuid NOT NULL,
    granted_by uuid,
    granted_at timestamp with time zone DEFAULT now(),
    CONSTRAINT user_roles_pkey PRIMARY KEY (user_id, role_id)
);

-- Name: TABLE user_roles; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.user_roles IS 'Assigns roles to users';

-- Name: user_view_history; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.user_view_history (
    id uuid DEFAULT uuidv7() CONSTRAINT user_view_history_pkey PRIMARY KEY,
    user_id uuid NOT NULL,
    start_position real NOT NULL,
    end_position real NOT NULL,
    duration real NOT NULL,
    viewed_at bigint NOT NULL,
    session_duration integer NOT NULL,
    media_uuid uuid NOT NULL,
    media_type smallint NOT NULL,
    CONSTRAINT check_media_type CHECK ((media_type = ANY (ARRAY[0, 1, 2, 3])))
);

COMMENT ON TABLE public.user_view_history IS 'Tracks view history using UUID + media_type (u8) instead of MediaID JSONB';

-- Name: user_watch_progress; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.user_watch_progress (
    user_id uuid NOT NULL,
    "position" real NOT NULL,
    duration real NOT NULL,
    last_watched bigint NOT NULL,
    updated_at bigint NOT NULL,
    media_uuid uuid NOT NULL,
    media_type smallint NOT NULL,
    CONSTRAINT user_watch_progress_pkey PRIMARY KEY (user_id, media_uuid),
    CONSTRAINT check_media_type CHECK ((media_type = ANY (ARRAY[0, 1, 2, 3]))),
    CONSTRAINT user_watch_progress_duration_check CHECK ((duration > (0)::double precision)),
    CONSTRAINT user_watch_progress_position_check CHECK (("position" >= (0)::double precision))
);

-- Name: TABLE user_watch_progress; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON TABLE public.user_watch_progress IS 'Tracks user watch progress using UUID + media_type (u8) instead of MediaID JSONB';

-- Name: users; Type: TABLE; Schema: public; Owner: postgres
CREATE TABLE public.users (
    id uuid DEFAULT uuidv7() CONSTRAINT users_pkey PRIMARY KEY,
    username public.citext NOT NULL UNIQUE,
    display_name character varying(100) NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    avatar_url character varying(255),
    last_login timestamp with time zone,
    is_active boolean DEFAULT true NOT NULL,
    failed_login_attempts smallint DEFAULT 0 NOT NULL,
    is_locked boolean DEFAULT false NOT NULL,
    locked_until timestamp with time zone,
    email character varying(255),
    preferences jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT users_username_lowercase CHECK (((username)::text = lower((username)::text)))
);

-- Name: idx_admin_actions_admin; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_admin_actions_admin ON public.admin_actions USING btree (admin_id);

-- Name: idx_admin_actions_created; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_admin_actions_created ON public.admin_actions USING btree (created_at DESC);

-- Name: idx_admin_actions_target; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_admin_actions_target ON public.admin_actions USING btree (target_type, target_id);

-- Name: idx_admin_actions_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_admin_actions_type ON public.admin_actions USING btree (action_type);

-- Name: idx_auth_device_sessions_fingerprint_active; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_device_sessions_fingerprint_active ON public.auth_device_sessions USING btree (device_fingerprint) WHERE (revoked_at IS NULL);

-- Name: idx_auth_device_sessions_user_status; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_device_sessions_user_status ON public.auth_device_sessions USING btree (user_id, status);

-- Name: idx_auth_device_sessions_trusted_until; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_device_sessions_trusted_until ON public.auth_device_sessions USING btree (trusted_until) WHERE ((status = 'trusted'::public.auth_device_status) AND (trusted_until IS NOT NULL) AND (revoked_at IS NULL));

-- Name: idx_auth_device_sessions_locked_until; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_device_sessions_locked_until ON public.auth_device_sessions USING btree (locked_until) WHERE ((locked_until IS NOT NULL) AND (revoked_at IS NULL));

-- Name: idx_auth_device_sessions_last_seen; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_device_sessions_last_seen ON public.auth_device_sessions USING btree (last_seen_at DESC) WHERE (revoked_at IS NULL);

-- Name: idx_auth_device_sessions_user_fingerprint_active; Type: INDEX; Schema: public; Owner: postgres
CREATE UNIQUE INDEX auth_device_sessions_unique_fingerprint ON public.auth_device_sessions USING btree (user_id, device_fingerprint) WHERE (revoked_at IS NULL);

-- Name: idx_auth_sessions_user; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_sessions_user ON public.auth_sessions USING btree (user_id);

-- Name: idx_auth_sessions_user_device_active; Type: INDEX; Schema: public; Owner: postgres
CREATE UNIQUE INDEX auth_sessions_active_per_device ON public.auth_sessions USING btree (user_id, device_session_id) WHERE ((device_session_id IS NOT NULL) AND (revoked = false) AND (revoked_at IS NULL));

-- Name: idx_auth_sessions_expires_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_sessions_expires_at ON public.auth_sessions USING btree (expires_at);

-- Name: idx_auth_sessions_last_activity; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_sessions_last_activity ON public.auth_sessions USING btree (last_activity DESC);

-- Name: idx_setup_claims_active_code; Type: INDEX; Schema: public; Owner: postgres
CREATE UNIQUE INDEX idx_setup_claims_active_code ON public.setup_claims USING btree (code_hash) WHERE ((confirmed_at IS NULL) AND (revoked_at IS NULL));

-- Name: idx_setup_claims_expires_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_setup_claims_expires_at ON public.setup_claims USING btree (expires_at);

-- Name: idx_auth_refresh_tokens_user; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_refresh_tokens_user ON public.auth_refresh_tokens USING btree (user_id);

-- Name: idx_auth_refresh_tokens_device_session; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_refresh_tokens_device_session ON public.auth_refresh_tokens USING btree (device_session_id);

-- Name: idx_auth_refresh_tokens_family_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_refresh_tokens_family_id ON public.auth_refresh_tokens USING btree (family_id) WHERE (family_id IS NOT NULL);

-- Name: idx_auth_refresh_tokens_expires_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_refresh_tokens_expires_at ON public.auth_refresh_tokens USING btree (expires_at);

-- Name: idx_auth_refresh_tokens_active; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_refresh_tokens_active ON public.auth_refresh_tokens USING btree (token_hash) WHERE (revoked = false);

-- Name: idx_auth_device_challenges_device_session; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_device_challenges_device_session ON public.auth_device_challenges USING btree (device_session_id);

-- Name: idx_auth_device_challenges_active; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_device_challenges_active ON public.auth_device_challenges USING btree (device_session_id, expires_at) WHERE (used = false);

-- Name: idx_auth_events_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_events_created_at ON public.auth_events USING btree (created_at DESC);

-- Name: idx_auth_events_user_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_events_user_created_at ON public.auth_events USING btree (user_id, created_at DESC);

-- Name: idx_auth_events_device_session_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_events_device_session_created_at ON public.auth_events USING btree (device_session_id, created_at DESC) WHERE (device_session_id IS NOT NULL);

-- Name: idx_auth_events_event_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_auth_events_event_type ON public.auth_events USING btree (event_type, created_at DESC);

-- Name: idx_completed_media_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_completed_media_type ON public.user_completed_media USING btree (media_type);

-- Name: idx_completed_media_uuid; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_completed_media_uuid ON public.user_completed_media USING btree (media_uuid);

-- Name: idx_completed_user; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_completed_user ON public.user_completed_media USING btree (user_id);

-- Name: idx_completed_user_time; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_completed_user_time ON public.user_completed_media USING btree (user_id, completed_at DESC);

-- Name: idx_episode_references_composite; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_episode_references_composite ON public.episode_references USING btree (series_id, season_number, episode_number);

-- Name: idx_episode_references_episode_number; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_episode_references_episode_number ON public.episode_references USING btree (season_number, episode_number);

-- Name: idx_episode_references_file_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_episode_references_file_id ON public.episode_references USING btree (file_id);

-- Name: idx_episode_references_season_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_episode_references_season_id ON public.episode_references USING btree (season_id);

-- Name: idx_episode_references_series_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_episode_references_series_id ON public.episode_references USING btree (series_id);

-- Name: idx_episode_refs_file_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_episode_refs_file_id ON public.episode_references USING btree (file_id);

-- Name: idx_episode_refs_series_season_episode; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_episode_refs_series_season_episode ON public.episode_references USING btree (series_id, season_number, episode_number);

-- Additional sort indices for episodes by discovery/creation time
CREATE INDEX idx_episode_refs_series_discovered_at ON public.episode_references USING btree (series_id, discovered_at DESC) INCLUDE (id, season_id, season_number, episode_number, file_id, tmdb_series_id);
CREATE INDEX idx_episode_refs_series_created_at ON public.episode_references USING btree (series_id, created_at DESC) INCLUDE (id, season_id, season_number, episode_number, file_id, tmdb_series_id);
CREATE INDEX idx_episode_refs_season_discovered_at ON public.episode_references USING btree (season_id, discovered_at DESC) INCLUDE (id, series_id, season_number, episode_number, file_id, tmdb_series_id);
CREATE INDEX idx_episode_refs_season_created_at ON public.episode_references USING btree (season_id, created_at DESC) INCLUDE (id, series_id, season_number, episode_number, file_id, tmdb_series_id);

-- Name: idx_file_watch_events_detected_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_file_watch_events_detected_at ON public.file_watch_events USING btree (detected_at DESC);

-- Name: idx_file_watch_events_file_path; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_file_watch_events_file_path ON public.file_watch_events USING btree (file_path);

-- Name: idx_file_watch_events_library_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_file_watch_events_library_id ON public.file_watch_events USING btree (library_id);

-- Name: idx_file_watch_events_unprocessed; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_file_watch_events_unprocessed ON public.file_watch_events USING btree (library_id, detected_at) WHERE (processed = false);

-- Composite index to support cursor-based streaming by library and time, with id tiebreaker
CREATE INDEX idx_fwe_library_detected ON public.file_watch_events USING btree (library_id, detected_at ASC, id ASC);

-- Event type filter index for targeted consumers and analytics
CREATE INDEX idx_fwe_event_type ON public.file_watch_events USING btree (event_type);

-- Name: idx_folder_inventory_discovery_source; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_discovery_source ON public.folder_inventory USING btree (discovery_source, discovered_at DESC);

-- Name: idx_folder_inventory_folder_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_folder_type ON public.folder_inventory USING btree (folder_type, library_id);

-- Name: idx_folder_inventory_library_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_library_id ON public.folder_inventory USING btree (library_id);

-- Name: idx_folder_inventory_needs_scan; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_needs_scan ON public.folder_inventory USING btree (library_id, last_seen_at, processing_status) WHERE ((processing_status)::text <> 'skipped'::text);

-- Name: idx_folder_inventory_parent_folder_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_parent_folder_id ON public.folder_inventory USING btree (parent_folder_id);

-- Name: idx_folder_inventory_path_gin; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_path_gin ON public.folder_inventory USING gin (to_tsvector('simple'::regconfig, folder_path));

-- Name: idx_folder_inventory_processing_queue; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_processing_queue ON public.folder_inventory USING btree (processing_status, next_retry_at) WHERE ((processing_status)::text = ANY ((ARRAY['pending'::character varying, 'queued'::character varying, 'failed'::character varying])::text[]));

-- Name: idx_folder_inventory_retry; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_retry ON public.folder_inventory USING btree (processing_attempts, next_retry_at) WHERE (((processing_status)::text = 'failed'::text) AND (next_retry_at IS NOT NULL));

-- Name: idx_folder_inventory_size; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_folder_inventory_size ON public.folder_inventory USING btree (library_id, total_size_bytes DESC);

-- Name: idx_jobs_lease_expiry; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_jobs_lease_expiry ON public.orchestrator_jobs USING btree (lease_expires_at) WHERE ((state)::text = 'leased'::text);

-- Name: idx_jobs_ready_by_library; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_jobs_ready_by_library ON public.orchestrator_jobs USING btree (library_id, priority, available_at, created_at) WHERE ((state)::text = 'ready'::text);

-- Name: idx_jobs_ready_dequeue; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_jobs_ready_dequeue ON public.orchestrator_jobs USING btree (kind, priority, available_at, created_at) WHERE ((state)::text = 'ready'::text);

-- Name: idx_jobs_state_kind; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_jobs_state_kind ON public.orchestrator_jobs USING btree (state, kind);

-- Name: idx_jobs_dependency_deferred; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_jobs_dependency_deferred ON public.orchestrator_jobs USING btree (library_id, dependency_key) WHERE ((state)::text = 'deferred'::text);

-- Name: idx_series_scan_state_series_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_scan_state_series_id ON public.series_scan_state USING btree (series_id);

-- Name: idx_series_scan_state_status; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_scan_state_status ON public.series_scan_state USING btree (status);

-- Name: series_scan_state update_series_scan_state_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_series_scan_state_updated_at BEFORE UPDATE ON public.series_scan_state FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: idx_jwt_blacklist_expires_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_jwt_blacklist_expires_at ON public.jwt_blacklist USING btree (expires_at);

-- Name: idx_jwt_blacklist_user_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_jwt_blacklist_user_id ON public.jwt_blacklist USING btree (user_id);

-- Name: idx_libraries_enabled; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_libraries_enabled ON public.libraries USING btree (enabled, library_type);

-- Name: idx_libraries_last_scan; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_libraries_last_scan ON public.libraries USING btree (last_scan DESC NULLS LAST) WHERE (enabled = true);

-- Name: idx_login_attempts_ip; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_login_attempts_ip ON public.login_attempts USING btree (ip_address, attempted_at DESC);

-- Name: idx_media_files_discovered_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_discovered_at ON public.media_files USING btree (discovered_at DESC);

-- Name: idx_media_files_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_created_at ON public.media_files USING btree (created_at DESC);

-- Name: idx_media_files_library_discovered_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_library_discovered_at ON public.media_files USING btree (library_id, discovered_at DESC);

-- Name: idx_media_files_library_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_library_created_at ON public.media_files USING btree (library_id, created_at DESC);

-- Name: idx_media_files_library_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_library_id ON public.media_files USING btree (library_id);

-- Name: idx_media_files_library_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_library_type ON public.media_files USING btree (library_id, ((parsed_info ->> 'media_type'::text)));

-- Name: idx_media_files_parsed_info; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_parsed_info ON public.media_files USING gin (parsed_info);

-- Name: idx_media_files_technical_metadata; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_technical_metadata ON public.media_files USING gin (technical_metadata);

-- Name: idx_media_files_unprocessed; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_unprocessed ON public.media_files USING btree (library_id, discovered_at) WHERE (technical_metadata IS NULL);

-- Name: idx_media_files_updated_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_files_updated_at ON public.media_files USING btree (updated_at DESC);

-- Name: idx_media_processing_status_analyzed; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_processing_status_analyzed ON public.media_processing_status USING btree (file_analyzed) WHERE (file_analyzed = false);

-- Name: idx_media_processing_status_images; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_processing_status_images ON public.media_processing_status USING btree (images_cached) WHERE (images_cached = false);

-- Name: idx_media_processing_status_metadata; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_processing_status_metadata ON public.media_processing_status USING btree (metadata_extracted) WHERE (metadata_extracted = false);

-- Name: idx_media_processing_status_retry; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_processing_status_retry ON public.media_processing_status USING btree (next_retry_at) WHERE ((retry_count > 0) AND (next_retry_at IS NOT NULL));

-- Name: idx_media_processing_status_tmdb; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_media_processing_status_tmdb ON public.media_processing_status USING btree (tmdb_matched) WHERE (tmdb_matched = false);

-- Name: idx_movie_metadata_release_date; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_metadata_release_date ON public.movie_metadata USING btree (release_date);

-- Name: idx_movie_metadata_title_search; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_metadata_title_search ON public.movie_metadata USING gin (to_tsvector('english'::regconfig, title));

-- Name: idx_movie_metadata_tmdb_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_metadata_tmdb_id ON public.movie_metadata USING btree (tmdb_id);

-- Batch-optimized fetch paths (movie reference batches)
CREATE INDEX idx_movie_metadata_library_batch_movie_id ON public.movie_metadata USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_alternative_titles_library_batch_movie_id ON public.movie_alternative_titles USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_collection_membership_library_batch_movie_id ON public.movie_collection_membership USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_genres_library_batch_movie_id ON public.movie_genres USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_keywords_library_batch_movie_id ON public.movie_keywords USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_production_companies_library_batch_movie_id ON public.movie_production_companies USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_production_countries_library_batch_movie_id ON public.movie_production_countries USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_recommendations_library_batch_movie_id ON public.movie_recommendations USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_release_dates_library_batch_movie_id ON public.movie_release_dates USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_similar_library_batch_movie_id ON public.movie_similar USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_spoken_languages_library_batch_movie_id ON public.movie_spoken_languages USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_translations_library_batch_movie_id ON public.movie_translations USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_videos_library_batch_movie_id ON public.movie_videos USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_cast_library_batch_movie_id ON public.movie_cast USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_crew_library_batch_movie_id ON public.movie_crew USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_sort_positions_library_batch_movie_id ON public.movie_sort_positions USING btree (library_id, batch_id, movie_id);
CREATE INDEX idx_movie_reference_batches_library_finalized_at ON public.movie_reference_batches USING btree (library_id, finalized_at);
CREATE INDEX idx_movie_reference_batches_open ON public.movie_reference_batches USING btree (library_id, batch_id) WHERE (finalized_at IS NULL);

-- Name: idx_movie_pos_bitrate; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_bitrate ON public.movie_sort_positions USING btree (library_id, bitrate_pos);

-- Name: idx_movie_pos_bitrate_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_bitrate_desc ON public.movie_sort_positions USING btree (library_id, bitrate_pos_desc);

-- Name: idx_movie_pos_cert; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_cert ON public.movie_sort_positions USING btree (library_id, content_rating_pos);

-- Name: idx_movie_pos_cert_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_cert_desc ON public.movie_sort_positions USING btree (library_id, content_rating_pos_desc);

-- Name: idx_movie_pos_date_added; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_date_added ON public.movie_sort_positions USING btree (library_id, date_added_pos);

-- Name: idx_movie_pos_date_added_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_date_added_desc ON public.movie_sort_positions USING btree (library_id, date_added_pos_desc);

-- Name: idx_movie_pos_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_created_at ON public.movie_sort_positions USING btree (library_id, created_at_pos);

-- Name: idx_movie_pos_created_at_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_created_at_desc ON public.movie_sort_positions USING btree (library_id, created_at_pos_desc);

-- Name: idx_movie_pos_file_size; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_file_size ON public.movie_sort_positions USING btree (library_id, file_size_pos);

-- Name: idx_movie_pos_file_size_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_file_size_desc ON public.movie_sort_positions USING btree (library_id, file_size_pos_desc);

-- Name: idx_movie_pos_popularity; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_popularity ON public.movie_sort_positions USING btree (library_id, popularity_pos);

-- Name: idx_movie_pos_popularity_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_popularity_desc ON public.movie_sort_positions USING btree (library_id, popularity_pos_desc);

-- Name: idx_movie_pos_rating; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_rating ON public.movie_sort_positions USING btree (library_id, rating_pos);

-- Name: idx_movie_pos_rating_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_rating_desc ON public.movie_sort_positions USING btree (library_id, rating_pos_desc);

-- Name: idx_movie_pos_release; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_release ON public.movie_sort_positions USING btree (library_id, release_date_pos);

-- Name: idx_movie_pos_release_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_release_desc ON public.movie_sort_positions USING btree (library_id, release_date_pos_desc);

-- Name: idx_movie_pos_resolution; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_resolution ON public.movie_sort_positions USING btree (library_id, resolution_pos);

-- Name: idx_movie_pos_resolution_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_resolution_desc ON public.movie_sort_positions USING btree (library_id, resolution_pos_desc);

-- Name: idx_movie_pos_runtime; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_runtime ON public.movie_sort_positions USING btree (library_id, runtime_pos);

-- Name: idx_movie_pos_runtime_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_runtime_desc ON public.movie_sort_positions USING btree (library_id, runtime_pos_desc);

-- Name: idx_movie_pos_title; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_title ON public.movie_sort_positions USING btree (library_id, title_pos);

-- Name: idx_movie_pos_title_desc; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_pos_title_desc ON public.movie_sort_positions USING btree (library_id, title_pos_desc);

-- Name: idx_movie_references_file_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_references_file_id ON public.movie_references USING btree (file_id);

-- Name: idx_movie_references_library_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_references_library_id ON public.movie_references USING btree (library_id);

CREATE INDEX idx_movie_references_library_batch_id ON public.movie_references USING btree (library_id, batch_id, id);

-- Name: idx_movie_references_library_title; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_references_library_title ON public.movie_references USING btree (library_id, title);

-- Name: idx_movie_references_title; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_references_title ON public.movie_references USING gin (to_tsvector('english'::regconfig, (title)::text));

-- Name: idx_movie_references_tmdb_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_references_tmdb_id ON public.movie_references USING btree (tmdb_id);

-- Name: idx_movie_refs_library_discovered_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_refs_library_discovered_at ON public.movie_references USING btree (library_id, discovered_at DESC);

-- Name: idx_movie_refs_library_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_refs_library_created_at ON public.movie_references USING btree (library_id, created_at DESC);

-- Name: idx_movie_refs_library_tmdb; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_refs_library_tmdb ON public.movie_references USING btree (library_id, tmdb_id);

-- Name: idx_movie_refs_title_fts; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_refs_title_fts ON public.movie_references USING gin (to_tsvector('english'::regconfig, (title)::text));

-- Name: idx_movie_refs_title_lower; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_refs_title_lower ON public.movie_references USING btree (lower((title)::text));

-- Name: idx_movie_refs_title_trgm; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_movie_refs_title_trgm ON public.movie_references USING gin (title public.gin_trgm_ops);

-- Name: idx_password_reset_expires; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_password_reset_expires ON public.password_reset_tokens USING btree (expires_at) WHERE (used_at IS NULL);

-- Name: idx_permissions_category; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_permissions_category ON public.permissions USING btree (category);

-- Name: idx_permissions_name; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_permissions_name ON public.permissions USING btree (name);

-- Name: idx_rate_limit_blocked_until; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_rate_limit_blocked_until ON public.rate_limit_state USING btree (blocked_until) WHERE (blocked_until IS NOT NULL);

-- Name: idx_rate_limit_key_endpoint; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_rate_limit_key_endpoint ON public.rate_limit_state USING btree (key, endpoint);

-- Name: idx_rate_limit_window_start; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_rate_limit_window_start ON public.rate_limit_state USING btree (window_start);

-- Name: idx_role_permissions_permission; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_role_permissions_permission ON public.role_permissions USING btree (permission_id);

-- Name: idx_role_permissions_role; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_role_permissions_role ON public.role_permissions USING btree (role_id);

-- Name: idx_scan_cursors_staleness; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_scan_cursors_staleness ON public.scan_cursors USING btree (library_id, last_scan_at DESC);

-- Name: idx_scan_state_active; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_scan_state_active ON public.scan_state USING btree (library_id, status) WHERE ((status)::text = ANY ((ARRAY['pending'::character varying, 'running'::character varying, 'paused'::character varying])::text[]));

-- Name: idx_scan_state_library_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_scan_state_library_id ON public.scan_state USING btree (library_id);

-- Name: idx_scan_state_scan_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_scan_state_scan_type ON public.scan_state USING btree (scan_type);

-- Name: idx_scan_state_started_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_scan_state_started_at ON public.scan_state USING btree (started_at DESC);

-- Name: idx_scan_state_status; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_scan_state_status ON public.scan_state USING btree (status);

-- Name: idx_season_references_library_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_season_references_library_id ON public.season_references USING btree (library_id);

-- Name: idx_season_references_season_number; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_season_references_season_number ON public.season_references USING btree (season_number);

-- Name: idx_season_references_series_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_season_references_series_id ON public.season_references USING btree (series_id);

-- Name: idx_season_refs_series_season; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_season_refs_series_season ON public.season_references USING btree (series_id, season_number);

-- Name: idx_season_refs_library_series_season; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_season_refs_library_series_season ON public.season_references USING btree (library_id, series_id, season_number);

-- Additional sort indices for seasons
CREATE INDEX idx_season_refs_library_discovered_at ON public.season_references USING btree (library_id, discovered_at DESC) INCLUDE (id, series_id, season_number, tmdb_series_id);

CREATE INDEX idx_season_refs_library_created_at ON public.season_references USING btree (library_id, created_at DESC) INCLUDE (id, series_id, season_number, tmdb_series_id);

-- Name: idx_security_audit_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_security_audit_created_at ON public.security_audit_log USING btree (created_at DESC);

-- Name: idx_security_audit_device_session; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_security_audit_device_session ON public.security_audit_log USING btree (device_session_id) WHERE (device_session_id IS NOT NULL);

-- Name: idx_security_audit_event_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_security_audit_event_type ON public.security_audit_log USING btree (event_type);

-- Name: idx_security_audit_ip_address; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_security_audit_ip_address ON public.security_audit_log USING btree (ip_address) WHERE (ip_address IS NOT NULL);

-- Name: idx_security_audit_severity; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_security_audit_severity ON public.security_audit_log USING btree (severity) WHERE (severity = ANY (ARRAY['warning'::text, 'error'::text, 'critical'::text]));

-- Name: idx_security_audit_user_event_time; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_security_audit_user_event_time ON public.security_audit_log USING btree (user_id, event_type, created_at DESC) WHERE (user_id IS NOT NULL);

-- Name: idx_security_audit_user_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_security_audit_user_id ON public.security_audit_log USING btree (user_id) WHERE (user_id IS NOT NULL);

-- Name: idx_series_metadata_first_air; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_metadata_first_air ON public.series_metadata USING btree (first_air_date);

-- Name: idx_series_metadata_title_search; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_metadata_title_search ON public.series_metadata USING gin (to_tsvector('english'::regconfig, name));

-- Name: idx_series_metadata_tmdb_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_metadata_tmdb_id ON public.series_metadata USING btree (tmdb_id);

-- Name: idx_series_library_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_library_id ON public.series USING btree (library_id);

-- Name: idx_series_library_title; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_library_title ON public.series USING btree (library_id, title);

-- Name: idx_series_title; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_title ON public.series USING gin (to_tsvector('english'::regconfig, (title)::text));

-- Name: idx_series_tmdb_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_tmdb_id ON public.series USING btree (tmdb_id);

-- Name: idx_series_refs_library_created_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_refs_library_created_at ON public.series USING btree (library_id, created_at DESC) INCLUDE (id, title, tmdb_id);

-- Name: idx_series_refs_library_discovered_at; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_refs_library_discovered_at ON public.series USING btree (library_id, discovered_at DESC) INCLUDE (id, title, tmdb_id);

-- Name: idx_series_refs_title_fts; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_refs_title_fts ON public.series USING gin (to_tsvector('english'::regconfig, (title)::text));

-- Name: idx_series_refs_title_lower; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_refs_title_lower ON public.series USING btree (lower((title)::text));

-- Name: idx_series_refs_title_trgm; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_series_refs_title_trgm ON public.series USING gin (title public.gin_trgm_ops);

-- Name: idx_sorted_indices_last_updated; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sorted_indices_last_updated ON public.library_sorted_indices USING btree (last_updated);

-- Name: idx_sorted_indices_library; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sorted_indices_library ON public.library_sorted_indices USING btree (library_id);

-- Name: idx_sorted_indices_metadata; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sorted_indices_metadata ON public.library_sorted_indices USING gin (metadata);

-- Name: idx_sorted_indices_sort_field; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sorted_indices_sort_field ON public.library_sorted_indices USING btree (sort_field);

-- Name: idx_sync_history_session; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sync_history_session ON public.sync_session_history USING btree (session_id, created_at DESC);

-- Name: idx_sync_participants_session; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sync_participants_session ON public.sync_participants USING btree (session_id);

-- Name: idx_sync_participants_user; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sync_participants_user ON public.sync_participants USING btree (user_id);

-- Name: idx_sync_sessions_expires; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sync_sessions_expires ON public.sync_sessions USING btree (expires_at) WHERE (is_active = true);

-- Name: idx_sync_sessions_host; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sync_sessions_host ON public.sync_sessions USING btree (host_id);

-- Name: idx_sync_sessions_media_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sync_sessions_media_type ON public.sync_sessions USING btree (media_type);

-- Name: idx_sync_sessions_media_uuid; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sync_sessions_media_uuid ON public.sync_sessions USING btree (media_uuid);

-- Name: idx_sync_sessions_room_code; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_sync_sessions_room_code ON public.sync_sessions USING btree (room_code) WHERE (is_active = true);

-- Name: idx_user_credentials_updated; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_user_credentials_updated ON public.user_credentials USING btree (updated_at DESC);

-- Name: idx_user_credentials_user_id; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_user_credentials_user_id ON public.user_credentials USING btree (user_id);

-- Name: idx_user_permissions_user; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_user_permissions_user ON public.user_permissions USING btree (user_id);

-- Name: idx_user_roles_role; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_user_roles_role ON public.user_roles USING btree (role_id);

-- Name: idx_user_roles_user; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_user_roles_user ON public.user_roles USING btree (user_id);

-- Name: idx_users_email_lower; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_users_email_lower ON public.users USING btree (lower((email)::text)) WHERE (email IS NOT NULL);

-- Name: idx_users_email_unique; Type: INDEX; Schema: public; Owner: postgres
CREATE UNIQUE INDEX idx_users_email_unique ON public.users USING btree (email) WHERE (email IS NOT NULL);

-- Name: idx_users_is_active; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_users_is_active ON public.users USING btree (is_active) WHERE (is_active = true);

-- Name: idx_users_last_login; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_users_last_login ON public.users USING btree (last_login) WHERE ((is_active = true) AND (last_login IS NOT NULL));

-- Name: idx_users_preferences_auto_login; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_users_preferences_auto_login ON public.users USING btree (((preferences ->> 'auto_login_enabled'::text))) WHERE (((preferences ->> 'auto_login_enabled'::text))::boolean = true);

-- Name: idx_view_history_media_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_view_history_media_type ON public.user_view_history USING btree (media_type);

-- Name: idx_view_history_media_uuid; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_view_history_media_uuid ON public.user_view_history USING btree (media_uuid);

-- Name: idx_view_history_user; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_view_history_user ON public.user_view_history USING btree (user_id, viewed_at DESC);

-- Name: idx_watch_progress_continue; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_watch_progress_continue ON public.user_watch_progress USING btree (user_id, last_watched DESC) WHERE (("position" > (0)::double precision) AND (("position" / duration) < (0.95)::double precision));

-- Name: idx_watch_progress_media_type; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_watch_progress_media_type ON public.user_watch_progress USING btree (media_type);

-- Name: idx_watch_progress_user_last; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_watch_progress_user_last ON public.user_watch_progress USING btree (user_id, last_watched DESC);

-- Name: idx_watch_progress_user_last_watched; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_watch_progress_user_last_watched ON public.user_watch_progress USING btree (user_id, last_watched DESC);

-- Name: idx_watch_progress_user_media; Type: INDEX; Schema: public; Owner: postgres
CREATE INDEX idx_watch_progress_user_media ON public.user_watch_progress USING btree (user_id, media_uuid);

-- Name: movie_alternative_titles_unique_idx; Type: INDEX; Schema: public; Owner: postgres
CREATE UNIQUE INDEX movie_alternative_titles_unique_idx ON public.movie_alternative_titles USING btree (movie_id, COALESCE(iso_3166_1, ''::text), COALESCE(title_type, ''::text), title);

-- Name: series_tmdb_id_library_id_key; Type: INDEX; Schema: public; Owner: postgres
CREATE UNIQUE INDEX series_tmdb_id_library_id_key ON public.series USING btree (tmdb_id, library_id) WHERE (tmdb_id IS NOT NULL);

-- Name: uq_jobs_dedupe_active; Type: INDEX; Schema: public; Owner: postgres
CREATE UNIQUE INDEX uq_jobs_dedupe_active ON public.orchestrator_jobs USING btree (dedupe_key) WHERE ((state)::text = ANY ((ARRAY['ready'::character varying, 'deferred'::character varying, 'leased'::character varying])::text[]));

-- Name: uq_jobs_lease_id_active; Type: INDEX; Schema: public; Owner: postgres
CREATE UNIQUE INDEX uq_jobs_lease_id_active ON public.orchestrator_jobs USING btree (lease_id) WHERE (((state)::text = 'leased'::text) AND (lease_id IS NOT NULL));

-- Name: user_watch_progress move_completed_items; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER move_completed_items BEFORE INSERT OR UPDATE ON public.user_watch_progress FOR EACH ROW EXECUTE FUNCTION public.check_and_move_completed();

-- Name: auth_device_sessions trg_auth_device_sessions_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER trg_auth_device_sessions_updated_at BEFORE UPDATE ON public.auth_device_sessions FOR EACH ROW EXECUTE FUNCTION public.update_auth_device_sessions_updated_at();

-- Name: episode_metadata update_episode_metadata_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_episode_metadata_updated_at BEFORE UPDATE ON public.episode_metadata FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: episode_references update_episode_references_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_episode_references_updated_at BEFORE UPDATE ON public.episode_references FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: folder_inventory update_folder_inventory_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_folder_inventory_updated_at BEFORE UPDATE ON public.folder_inventory FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: libraries update_libraries_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER libraries_prevent_movie_ref_batch_size_change
    BEFORE UPDATE OF movie_ref_batch_size ON public.libraries
    FOR EACH ROW
    EXECUTE FUNCTION public.prevent_movie_ref_batch_size_change_after_first_reference();

CREATE TRIGGER update_libraries_updated_at BEFORE UPDATE ON public.libraries FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: media_files update_media_files_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_media_files_updated_at BEFORE UPDATE ON public.media_files FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: media_processing_status update_media_processing_status_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_media_processing_status_updated_at BEFORE UPDATE ON public.media_processing_status FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: movie_metadata update_movie_metadata_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_movie_metadata_updated_at BEFORE UPDATE ON public.movie_metadata FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: movie_metadata propagate_movie_reference_theme_color; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER propagate_movie_reference_theme_color
    AFTER INSERT OR UPDATE OF primary_poster_image_id ON public.movie_metadata
    FOR EACH ROW
    EXECUTE FUNCTION public.propagate_movie_reference_theme_color();

-- Name: movie_references update_movie_references_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER movie_references_set_batch_id
    BEFORE INSERT ON public.movie_references
    FOR EACH ROW
    EXECUTE FUNCTION public.set_movie_reference_batch_id();

CREATE TRIGGER update_movie_references_updated_at BEFORE UPDATE ON public.movie_references FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: orchestrator_jobs update_orchestrator_jobs_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_orchestrator_jobs_updated_at BEFORE UPDATE ON public.orchestrator_jobs FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: persons update_persons_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_persons_updated_at BEFORE UPDATE ON public.persons FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: rate_limit_state update_rate_limit_state_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_rate_limit_state_updated_at BEFORE UPDATE ON public.rate_limit_state FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: scan_state update_scan_state_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_scan_state_updated_at BEFORE UPDATE ON public.scan_state FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: season_metadata update_season_metadata_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_season_metadata_updated_at BEFORE UPDATE ON public.season_metadata FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: season_references update_season_references_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_season_references_updated_at BEFORE UPDATE ON public.season_references FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: series_metadata update_series_metadata_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_series_metadata_updated_at BEFORE UPDATE ON public.series_metadata FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: series_metadata propagate_series_reference_theme_color; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER propagate_series_reference_theme_color
    AFTER INSERT OR UPDATE OF primary_poster_image_id ON public.series_metadata
    FOR EACH ROW
    EXECUTE FUNCTION public.propagate_series_reference_theme_color();

-- Name: series update_series_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_series_updated_at BEFORE UPDATE ON public.series FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Name: cached_images propagate_cached_image_theme_color; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER propagate_cached_image_theme_color
    AFTER INSERT OR UPDATE OF theme_color ON public.cached_images
    FOR EACH ROW
    EXECUTE FUNCTION public.propagate_cached_image_theme_color();

-- Name: user_credentials update_user_credentials_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_user_credentials_updated_at BEFORE UPDATE ON public.user_credentials FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_timestamp();

-- Name: users update_users_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON public.users FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_timestamp();

-- Name: admin_actions admin_actions_admin_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.admin_actions
    ADD CONSTRAINT admin_actions_admin_id_fkey FOREIGN KEY (admin_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: auth_security_settings auth_security_settings_updated_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_security_settings
    ADD CONSTRAINT auth_security_settings_updated_by_fkey FOREIGN KEY (updated_by) REFERENCES public.users(id) ON DELETE SET NULL;

-- Name: auth_device_sessions auth_device_sessions_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_device_sessions
    ADD CONSTRAINT auth_device_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: auth_device_sessions auth_device_sessions_first_authenticated_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_device_sessions
    ADD CONSTRAINT auth_device_sessions_first_authenticated_by_fkey FOREIGN KEY (first_authenticated_by) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: auth_device_sessions auth_device_sessions_revoked_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_device_sessions
    ADD CONSTRAINT auth_device_sessions_revoked_by_fkey FOREIGN KEY (revoked_by) REFERENCES public.users(id) ON DELETE SET NULL;

-- Name: auth_events auth_events_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_events
    ADD CONSTRAINT auth_events_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL;

-- Name: auth_events auth_events_device_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_events
    ADD CONSTRAINT auth_events_device_session_id_fkey FOREIGN KEY (device_session_id) REFERENCES public.auth_device_sessions(id) ON DELETE SET NULL;

-- Name: auth_events auth_events_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_events
    ADD CONSTRAINT auth_events_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.auth_sessions(id) ON DELETE SET NULL;

-- Name: auth_sessions auth_sessions_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_sessions
    ADD CONSTRAINT auth_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: auth_sessions auth_sessions_device_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_sessions
    ADD CONSTRAINT auth_sessions_device_session_id_fkey FOREIGN KEY (device_session_id) REFERENCES public.auth_device_sessions(id) ON DELETE CASCADE;

-- Name: auth_refresh_tokens auth_refresh_tokens_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_refresh_tokens
    ADD CONSTRAINT auth_refresh_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: auth_refresh_tokens auth_refresh_tokens_device_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_refresh_tokens
    ADD CONSTRAINT auth_refresh_tokens_device_session_id_fkey FOREIGN KEY (device_session_id) REFERENCES public.auth_device_sessions(id) ON DELETE CASCADE;

-- Name: auth_refresh_tokens auth_refresh_tokens_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_refresh_tokens
    ADD CONSTRAINT auth_refresh_tokens_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.auth_sessions(id) ON DELETE SET NULL;

-- Name: auth_device_challenges auth_device_challenges_device_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.auth_device_challenges
    ADD CONSTRAINT auth_device_challenges_device_session_id_fkey FOREIGN KEY (device_session_id) REFERENCES public.auth_device_sessions(id) ON DELETE CASCADE;

-- Name: jwt_blacklist jwt_blacklist_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.jwt_blacklist
    ADD CONSTRAINT jwt_blacklist_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: episode_content_ratings episode_content_ratings_episode_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_content_ratings
    ADD CONSTRAINT episode_content_ratings_episode_id_fkey FOREIGN KEY (episode_id) REFERENCES public.episode_references(id) ON DELETE CASCADE;

-- Name: episode_keywords episode_keywords_episode_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_keywords
    ADD CONSTRAINT episode_keywords_episode_id_fkey FOREIGN KEY (episode_id) REFERENCES public.episode_references(id) ON DELETE CASCADE;

-- Name: episode_metadata episode_metadata_episode_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_metadata
    ADD CONSTRAINT episode_metadata_episode_id_fkey FOREIGN KEY (episode_id) REFERENCES public.episode_references(id) ON DELETE CASCADE;

-- Name: episode_metadata episode_metadata_primary_thumbnail_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_metadata
    ADD CONSTRAINT episode_metadata_primary_thumbnail_image_id_fkey FOREIGN KEY (primary_thumbnail_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: episode_references episode_references_file_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_references
    ADD CONSTRAINT episode_references_file_id_fkey FOREIGN KEY (file_id) REFERENCES public.media_files(id) ON DELETE CASCADE;

-- Name: episode_references episode_references_season_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_references
    ADD CONSTRAINT episode_references_season_id_fkey FOREIGN KEY (season_id) REFERENCES public.season_references(id) ON DELETE CASCADE;

-- Name: episode_references episode_references_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_references
    ADD CONSTRAINT episode_references_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: episode_translations episode_translations_episode_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_translations
    ADD CONSTRAINT episode_translations_episode_id_fkey FOREIGN KEY (episode_id) REFERENCES public.episode_references(id) ON DELETE CASCADE;

-- Name: episode_videos episode_videos_episode_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.episode_videos
    ADD CONSTRAINT episode_videos_episode_id_fkey FOREIGN KEY (episode_id) REFERENCES public.episode_references(id) ON DELETE CASCADE;

-- Name: file_watch_events file_watch_events_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.file_watch_events
    ADD CONSTRAINT file_watch_events_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: season_references fk_season_library; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.season_references
    ADD CONSTRAINT fk_season_library FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: folder_inventory folder_inventory_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.folder_inventory
    ADD CONSTRAINT folder_inventory_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: folder_inventory folder_inventory_parent_folder_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.folder_inventory
    ADD CONSTRAINT folder_inventory_parent_folder_id_fkey FOREIGN KEY (parent_folder_id) REFERENCES public.folder_inventory(id) ON DELETE CASCADE;

-- Name: library_sorted_indices library_sorted_indices_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.library_sorted_indices
    ADD CONSTRAINT library_sorted_indices_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: media_files media_files_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.media_files
    ADD CONSTRAINT media_files_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: media_processing_status media_processing_status_media_file_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.media_processing_status
    ADD CONSTRAINT media_processing_status_media_file_id_fkey FOREIGN KEY (media_file_id) REFERENCES public.media_files(id) ON DELETE CASCADE;

-- Name: movie_alternative_titles movie_alternative_titles_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_alternative_titles
    ADD CONSTRAINT movie_alternative_titles_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_collection_membership movie_collection_membership_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_collection_membership
    ADD CONSTRAINT movie_collection_membership_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_genres movie_genres_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_genres
    ADD CONSTRAINT movie_genres_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_keywords movie_keywords_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_keywords
    ADD CONSTRAINT movie_keywords_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_metadata movie_metadata_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_metadata
    ADD CONSTRAINT movie_metadata_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_metadata movie_metadata_primary_poster_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_metadata
    ADD CONSTRAINT movie_metadata_primary_poster_image_id_fkey FOREIGN KEY (primary_poster_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: movie_metadata movie_metadata_primary_backdrop_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_metadata
    ADD CONSTRAINT movie_metadata_primary_backdrop_image_id_fkey FOREIGN KEY (primary_backdrop_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: movie_production_companies movie_production_companies_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_production_companies
    ADD CONSTRAINT movie_production_companies_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_production_countries movie_production_countries_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_production_countries
    ADD CONSTRAINT movie_production_countries_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_recommendations movie_recommendations_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_recommendations
    ADD CONSTRAINT movie_recommendations_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_references movie_references_file_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_references
    ADD CONSTRAINT movie_references_file_id_fkey FOREIGN KEY (file_id) REFERENCES public.media_files(id) ON DELETE CASCADE;

-- Name: movie_references movie_references_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_references
    ADD CONSTRAINT movie_references_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: movie_reference_batches movie_reference_batches_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_reference_batches
    ADD CONSTRAINT movie_reference_batches_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: movie_reference_batch_cursor movie_reference_batch_cursor_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_reference_batch_cursor
    ADD CONSTRAINT movie_reference_batch_cursor_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: movie_release_dates movie_release_dates_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_release_dates
    ADD CONSTRAINT movie_release_dates_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_similar movie_similar_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_similar
    ADD CONSTRAINT movie_similar_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_sort_positions movie_sort_positions_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_sort_positions
    ADD CONSTRAINT movie_sort_positions_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: movie_sort_positions movie_sort_positions_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_sort_positions
    ADD CONSTRAINT movie_sort_positions_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_spoken_languages movie_spoken_languages_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_spoken_languages
    ADD CONSTRAINT movie_spoken_languages_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_translations movie_translations_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_translations
    ADD CONSTRAINT movie_translations_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: movie_videos movie_videos_movie_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.movie_videos
    ADD CONSTRAINT movie_videos_movie_id_fkey FOREIGN KEY (movie_id, library_id, batch_id) REFERENCES public.movie_references(id, library_id, batch_id) ON DELETE CASCADE;

-- Name: orchestrator_jobs orchestrator_jobs_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.orchestrator_jobs
    ADD CONSTRAINT orchestrator_jobs_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: series_scan_state series_scan_state_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_scan_state
    ADD CONSTRAINT series_scan_state_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: password_reset_tokens password_reset_tokens_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.password_reset_tokens
    ADD CONSTRAINT password_reset_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: person_aliases person_aliases_tmdb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.person_aliases
    ADD CONSTRAINT person_aliases_tmdb_id_fkey FOREIGN KEY (tmdb_id) REFERENCES public.persons(tmdb_id) ON DELETE CASCADE;

-- Name: role_permissions role_permissions_permission_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.role_permissions
    ADD CONSTRAINT role_permissions_permission_id_fkey FOREIGN KEY (permission_id) REFERENCES public.permissions(id) ON DELETE CASCADE;

-- Name: role_permissions role_permissions_role_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.role_permissions
    ADD CONSTRAINT role_permissions_role_id_fkey FOREIGN KEY (role_id) REFERENCES public.roles(id) ON DELETE CASCADE;

-- Name: scan_cursors scan_cursors_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.scan_cursors
    ADD CONSTRAINT scan_cursors_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: scan_state scan_state_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.scan_state
    ADD CONSTRAINT scan_state_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: season_keywords season_keywords_season_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.season_keywords
    ADD CONSTRAINT season_keywords_season_id_fkey FOREIGN KEY (season_id) REFERENCES public.season_references(id) ON DELETE CASCADE;

-- Name: season_metadata season_metadata_season_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.season_metadata
    ADD CONSTRAINT season_metadata_season_id_fkey FOREIGN KEY (season_id) REFERENCES public.season_references(id) ON DELETE CASCADE;

-- Name: season_metadata season_metadata_primary_poster_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.season_metadata
    ADD CONSTRAINT season_metadata_primary_poster_image_id_fkey FOREIGN KEY (primary_poster_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: season_references season_references_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.season_references
    ADD CONSTRAINT season_references_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: season_translations season_translations_season_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.season_translations
    ADD CONSTRAINT season_translations_season_id_fkey FOREIGN KEY (season_id) REFERENCES public.season_references(id) ON DELETE CASCADE;

-- Name: season_videos season_videos_season_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.season_videos
    ADD CONSTRAINT season_videos_season_id_fkey FOREIGN KEY (season_id) REFERENCES public.season_references(id) ON DELETE CASCADE;

-- Name: security_audit_log security_audit_log_device_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.security_audit_log
    ADD CONSTRAINT security_audit_log_device_session_id_fkey FOREIGN KEY (device_session_id) REFERENCES public.auth_device_sessions(id) ON DELETE SET NULL;

-- Name: security_audit_log security_audit_log_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.security_audit_log
    ADD CONSTRAINT security_audit_log_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL;

-- Name: series_content_ratings series_content_ratings_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_content_ratings
    ADD CONSTRAINT series_content_ratings_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_episode_groups series_episode_groups_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_episode_groups
    ADD CONSTRAINT series_episode_groups_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_genres series_genres_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_genres
    ADD CONSTRAINT series_genres_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_keywords series_keywords_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_keywords
    ADD CONSTRAINT series_keywords_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_metadata series_metadata_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_metadata
    ADD CONSTRAINT series_metadata_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_metadata series_metadata_primary_poster_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_metadata
    ADD CONSTRAINT series_metadata_primary_poster_image_id_fkey FOREIGN KEY (primary_poster_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: series_metadata series_metadata_primary_backdrop_image_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_metadata
    ADD CONSTRAINT series_metadata_primary_backdrop_image_id_fkey FOREIGN KEY (primary_backdrop_image_id) REFERENCES public.tmdb_image_variants(id);

-- Name: series_networks series_networks_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_networks
    ADD CONSTRAINT series_networks_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_origin_countries series_origin_countries_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_origin_countries
    ADD CONSTRAINT series_origin_countries_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_production_companies series_production_companies_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_production_companies
    ADD CONSTRAINT series_production_companies_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_production_countries series_production_countries_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_production_countries
    ADD CONSTRAINT series_production_countries_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_recommendations series_recommendations_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_recommendations
    ADD CONSTRAINT series_recommendations_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series series_library_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series
    ADD CONSTRAINT series_library_id_fkey FOREIGN KEY (library_id) REFERENCES public.libraries(id) ON DELETE CASCADE;

-- Name: series_similar series_similar_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_similar
    ADD CONSTRAINT series_similar_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_spoken_languages series_spoken_languages_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_spoken_languages
    ADD CONSTRAINT series_spoken_languages_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_translations series_translations_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_translations
    ADD CONSTRAINT series_translations_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: series_videos series_videos_series_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.series_videos
    ADD CONSTRAINT series_videos_series_id_fkey FOREIGN KEY (series_id) REFERENCES public.series(id) ON DELETE CASCADE;

-- Name: sync_participants sync_participants_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.sync_participants
    ADD CONSTRAINT sync_participants_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sync_sessions(id) ON DELETE CASCADE;

-- Name: sync_participants sync_participants_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.sync_participants
    ADD CONSTRAINT sync_participants_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id);

-- Name: sync_session_history sync_session_history_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.sync_session_history
    ADD CONSTRAINT sync_session_history_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id);

-- Name: sync_sessions sync_sessions_host_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.sync_sessions
    ADD CONSTRAINT sync_sessions_host_id_fkey FOREIGN KEY (host_id) REFERENCES public.users(id);

-- Name: user_completed_media user_completed_media_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_completed_media
    ADD CONSTRAINT user_completed_media_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: user_credentials user_credentials_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_credentials
    ADD CONSTRAINT user_credentials_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: user_permissions user_permissions_granted_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_permissions
    ADD CONSTRAINT user_permissions_granted_by_fkey FOREIGN KEY (granted_by) REFERENCES public.users(id);

-- Name: user_permissions user_permissions_permission_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_permissions
    ADD CONSTRAINT user_permissions_permission_id_fkey FOREIGN KEY (permission_id) REFERENCES public.permissions(id) ON DELETE CASCADE;

-- Name: user_permissions user_permissions_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_permissions
    ADD CONSTRAINT user_permissions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: user_roles user_roles_granted_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_granted_by_fkey FOREIGN KEY (granted_by) REFERENCES public.users(id);

-- Name: user_roles user_roles_role_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_role_id_fkey FOREIGN KEY (role_id) REFERENCES public.roles(id) ON DELETE CASCADE;

-- Name: user_roles user_roles_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: user_view_history user_view_history_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_view_history
    ADD CONSTRAINT user_view_history_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- Name: user_watch_progress user_watch_progress_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
ALTER TABLE ONLY public.user_watch_progress
    ADD CONSTRAINT user_watch_progress_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

-- PostgreSQL database dump complete

-- Name: COLUMN auth_device_sessions.device_public_key; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.device_public_key IS 'Device-bound public key used to validate possession';

-- Name: COLUMN auth_device_sessions.device_key_alg; Type: COMMENT; Schema: public; Owner: postgres
COMMENT ON COLUMN public.auth_device_sessions.device_key_alg IS 'Algorithm for device public key (e.g., ed25519)';

-- Episode identity-based watch state Tracks per-user watch state for episodes independent of files
CREATE TABLE IF NOT EXISTS public.user_episode_state (
    user_id uuid NOT NULL,
    tmdb_series_id bigint NOT NULL,
    season_number smallint NOT NULL,
    episode_number smallint NOT NULL,
    position real NOT NULL,
    duration real NOT NULL,
    last_watched bigint NOT NULL,
    is_completed boolean NOT NULL DEFAULT false,
    last_media_uuid uuid,
    CONSTRAINT user_episode_state_pkey PRIMARY KEY (
        user_id, tmdb_series_id, season_number, episode_number
    )
);

COMMENT ON TABLE public.user_episode_state IS 'Identity-based episode watch state keyed by (user, series TMDB id, season, episode)';

-- Helpful indexes
CREATE INDEX IF NOT EXISTS idx_user_episode_state_user_series ON public.user_episode_state (user_id, tmdb_series_id);
CREATE INDEX IF NOT EXISTS idx_user_episode_state_lastwatched ON public.user_episode_state (user_id, last_watched DESC);
CREATE INDEX IF NOT EXISTS idx_user_episode_state_completed ON public.user_episode_state (user_id, tmdb_series_id, is_completed);

-- Move application objects to `ferrex` schema and apply privileges Notes: - This baseline ships app objects initially in public (per dump), then moves them into `ferrex` to achieve the desired end-state without touching every statement. - Extensions remain in public. Types (enums/domains) -> ferrex (idempotent)
DO $$
DECLARE r RECORD;
BEGIN
  FOR r IN
    SELECT t.oid, n.nspname, t.typname
    FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE n.nspname = 'public'
      AND t.typtype IN ('e','d')
      AND NOT EXISTS (
        SELECT 1
        FROM pg_depend d
        JOIN pg_extension e ON e.oid = d.refobjid
        WHERE d.objid = t.oid AND d.deptype = 'e'
      )
  LOOP
    -- If a type with the same name already exists in ferrex, drop the public one; otherwise move it.
    IF EXISTS (
      SELECT 1 FROM pg_type t2 JOIN pg_namespace n2 ON n2.oid = t2.typnamespace
      WHERE n2.nspname = 'ferrex' AND t2.typname = r.typname
    ) THEN
      EXECUTE format('DROP TYPE IF EXISTS %I.%I CASCADE', r.nspname, r.typname);
    ELSE
      EXECUTE format('ALTER TYPE %I.%I SET SCHEMA ferrex', r.nspname, r.typname);
    END IF;
  END LOOP;
END $$;

-- Functions -> ferrex (exclude extension-owned) (idempotent)
DO $$
DECLARE r RECORD;
BEGIN
  FOR r IN
    SELECT p.oid, n.nspname, p.proname, pg_get_function_identity_arguments(p.oid) AS args
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'public'
      AND NOT EXISTS (
        SELECT 1
        FROM pg_depend d
        JOIN pg_extension e ON e.oid = d.refobjid
        WHERE d.objid = p.oid AND d.deptype = 'e'
      )
  LOOP
    IF EXISTS (
      SELECT 1 FROM pg_proc p2 JOIN pg_namespace n2 ON n2.oid = p2.pronamespace
      WHERE n2.nspname = 'ferrex'
        AND p2.proname = r.proname
        AND pg_get_function_identity_arguments(p2.oid) = r.args
    ) THEN
      -- Drop duplicate in public to avoid name conflicts
      EXECUTE format('DROP FUNCTION IF EXISTS %I.%I(%s) CASCADE', r.nspname, r.proname, r.args);
    ELSE
      EXECUTE format('ALTER FUNCTION %I.%I(%s) SET SCHEMA ferrex', r.nspname, r.proname, r.args);
    END IF;
  END LOOP;
END $$;

-- Tables -> ferrex (idempotent)
DO $$
DECLARE r RECORD;
BEGIN
  FOR r IN
    SELECT c.oid, n.nspname, c.relname
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = 'public'
      AND c.relkind IN ('r','p')
      AND NOT EXISTS (
        SELECT 1
        FROM pg_depend d
        JOIN pg_extension e ON e.oid = d.refobjid
        WHERE d.objid = c.oid AND d.deptype = 'e'
      )
      AND c.relname <> '_sqlx_migrations'
  LOOP
    IF EXISTS (
      SELECT 1 FROM pg_class c2 JOIN pg_namespace n2 ON n2.oid = c2.relnamespace
      WHERE n2.nspname = 'ferrex' AND c2.relname = r.relname AND c2.relkind IN ('r','p')
    ) THEN
      -- Prefer ferrex; drop stray public copy (includes dependent indexes)
      EXECUTE format('DROP TABLE IF EXISTS %I.%I CASCADE', r.nspname, r.relname);
    ELSE
      EXECUTE format('ALTER TABLE %I.%I SET SCHEMA ferrex', r.nspname, r.relname);
    END IF;
  END LOOP;
END $$;

-- Sequences -> ferrex (none expected but included for completeness, idempotent)
DO $$
DECLARE r RECORD;
BEGIN
  FOR r IN
    SELECT c.oid, n.nspname, c.relname
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = 'public'
      AND c.relkind = 'S'
      AND NOT EXISTS (
        SELECT 1
        FROM pg_depend d
        JOIN pg_extension e ON e.oid = d.refobjid
        WHERE d.objid = c.oid AND d.deptype = 'e'
      )
  LOOP
    IF EXISTS (
      SELECT 1 FROM pg_class c2 JOIN pg_namespace n2 ON n2.oid = c2.relnamespace
      WHERE n2.nspname = 'ferrex' AND c2.relname = r.relname AND c2.relkind = 'S'
    ) THEN
      EXECUTE format('DROP SEQUENCE IF EXISTS %I.%I CASCADE', r.nspname, r.relname);
    ELSE
      EXECUTE format('ALTER SEQUENCE %I.%I SET SCHEMA ferrex', r.nspname, r.relname);
    END IF;
  END LOOP;
END $$;

-- Views / Materialized Views -> ferrex (idempotent)
DO $$
DECLARE r RECORD;
BEGIN
  FOR r IN
    SELECT c.oid, n.nspname, c.relname, c.relkind
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = 'public'
      AND c.relkind IN ('v','m')
      AND NOT EXISTS (
        SELECT 1
        FROM pg_depend d
        JOIN pg_extension e ON e.oid = d.refobjid
        WHERE d.objid = c.oid AND d.deptype = 'e'
      )
  LOOP
    IF EXISTS (
      SELECT 1 FROM pg_class c2 JOIN pg_namespace n2 ON n2.oid = c2.relnamespace
      WHERE n2.nspname = 'ferrex' AND c2.relname = r.relname AND c2.relkind = r.relkind
    ) THEN
      IF r.relkind = 'v' THEN
        EXECUTE format('DROP VIEW IF EXISTS %I.%I CASCADE', r.nspname, r.relname);
      ELSE
        EXECUTE format('DROP MATERIALIZED VIEW IF EXISTS %I.%I CASCADE', r.nspname, r.relname);
      END IF;
    ELSE
      IF r.relkind = 'v' THEN
        EXECUTE format('ALTER VIEW %I.%I SET SCHEMA ferrex', r.nspname, r.relname);
      ELSE
        EXECUTE format('ALTER MATERIALIZED VIEW %I.%I SET SCHEMA ferrex', r.nspname, r.relname);
      END IF;
    END IF;
  END LOOP;
END $$;

-- Apply privileges and defaults to the current role running the migration
DO $$
DECLARE
  role_name text := current_user;
BEGIN
  EXECUTE format('GRANT USAGE, CREATE ON SCHEMA ferrex TO %I', role_name);
  EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA ferrex TO %I', role_name);
  EXECUTE format('GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA ferrex TO %I', role_name);
  EXECUTE format('GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ferrex TO %I', role_name);

  EXECUTE format('ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA ferrex GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO %I', role_name, role_name);
  EXECUTE format('ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA ferrex GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO %I', role_name, role_name);
  EXECUTE format('ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA ferrex GRANT EXECUTE ON FUNCTIONS TO %I', role_name, role_name);
END $$;

-- Set a database-level default search_path so unqualified names resolve to `ferrex` first. Rationale: tests and tools that don't set search_path still see the app schema.
DO $$
DECLARE
  db_name text := current_database();
BEGIN
  BEGIN
    EXECUTE format('ALTER DATABASE %I SET search_path = ferrex, public', db_name);
  EXCEPTION WHEN insufficient_privilege THEN
    -- Not a database owner or superuser: skip. The application sets search_path per-connection.
    NULL;
  END;
END $$;
