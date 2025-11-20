-- Episode identity-based watch state
-- Tracks per-user watch state for episodes independent of files

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
