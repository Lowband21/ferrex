-- Intelligence repository test fixtures.
--
-- Seeds two users, available + unavailable movies with metadata and genres,
-- a series/season/episode hierarchy with available and unavailable episodes,
-- and user watch progress. Used by tests/intelligence_repository.rs.

SET search_path TO ferrex, public;

-- Users -------------------------------------------------------------------
INSERT INTO users (id, username, display_name) VALUES
    ('11111111-0000-0000-0000-000000000001', 'alice', 'Alice'),
    ('11111111-0000-0000-0000-000000000002', 'bob', 'Bob')
ON CONFLICT (id) DO NOTHING;

-- Image variants referenced by metadata poster FKs ------------------------
-- All poster image ids used below must exist in tmdb_image_variants.
INSERT INTO tmdb_image_variants
    (id, image_variant, tmdb_path, media_id, media_type, width, height, vote_avg, vote_cnt, is_primary)
VALUES
    ('eee00000-0000-0000-0000-000000000001', 'poster',   '/m1.jpg', '22222222-0000-0000-0000-000000000001', 'movie',  500, 750, 7.0, 10, true),
    ('eee00000-0000-0000-0000-000000000002', 'poster',   '/m2.jpg', '22222222-0000-0000-0000-000000000002', 'movie',  500, 750, 7.0, 10, true),
    ('eee00000-0000-0000-0000-000000000003', 'poster',   '/m3.jpg', '22222222-0000-0000-0000-000000000003', 'movie',  500, 750, 7.0, 10, true),
    ('eee00000-0000-0000-0000-000000000010', 'poster',   '/s1.jpg', '44444444-0000-0000-0000-000000000001', 'series', 500, 750, 8.0, 20, true),
    ('eee00000-0000-0000-0000-000000000011', 'poster',   '/sn.jpg', '55555555-0000-0000-0000-000000000001', 'season', 500, 750, 8.0, 20, true)
ON CONFLICT (id) DO NOTHING;

-- Media files -------------------------------------------------------------
-- Library A (movies): two available files and one tombstoned file.
INSERT INTO media_files (id, library_id, media_id, media_type, file_path, filename, file_size, is_available, tombstoned_at)
VALUES
    ('33333333-0000-0000-0000-000000000001', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', '22222222-0000-0000-0000-000000000001', 'movie', '/lib/a/arrival.mkv',    'arrival.mkv',    1, true,  NULL),
    ('33333333-0000-0000-0000-000000000002', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', '22222222-0000-0000-0000-000000000002', 'movie', '/lib/a/tenet.mkv',      'tenet.mkv',      2, true,  NULL),
    ('33333333-0000-0000-0000-000000000003', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', '22222222-0000-0000-0000-000000000003', 'movie', '/lib/a/gone.mkv',       'gone.mkv',       3, false, now())
ON CONFLICT (file_path) DO NOTHING;

-- Library C (tvshows): one available episode file and one unavailable.
INSERT INTO media_files (id, library_id, media_id, media_type, file_path, filename, file_size, is_available, tombstoned_at)
VALUES
    ('77777777-0000-0000-0000-000000000001', 'cccccccc-cccc-cccc-cccc-cccccccccccc', '66666666-0000-0000-0000-000000000001', 'episode', '/lib/c/show/s1e1.mkv', 's1e1.mkv', 4, true,  NULL),
    ('77777777-0000-0000-0000-000000000002', 'cccccccc-cccc-cccc-cccc-cccccccccccc', '66666666-0000-0000-0000-000000000002', 'episode', '/lib/c/show/s1e2.mkv', 's1e2.mkv', 5, false, now())
ON CONFLICT (file_path) DO NOTHING;

-- Movie references (batch_id=1 required by movie_metadata composite FK) ---
INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, batch_id)
VALUES
    ('22222222-0000-0000-0000-000000000001', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', '33333333-0000-0000-0000-000000000001', 1041, 'Arrival', 1),
    ('22222222-0000-0000-0000-000000000002', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', '33333333-0000-0000-0000-000000000002', 1042, 'Tenet',   1),
    ('22222222-0000-0000-0000-000000000003', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', '33333333-0000-0000-0000-000000000003', 1043, 'Gone',    1)
ON CONFLICT (id) DO NOTHING;

-- Movie metadata ----------------------------------------------------------
INSERT INTO movie_metadata
    (movie_id, library_id, batch_id, tmdb_id, title, overview, release_date, runtime,
     vote_average, primary_certification, primary_poster_image_id)
VALUES
    ('22222222-0000-0000-0000-000000000001', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 1041, 'Arrival',
     'A linguist works to communicate with alien visitors.', DATE '2016-09-02', 116, 7.9, 'PG-13',
     'eee00000-0000-0000-0000-000000000001'),
    ('22222222-0000-0000-0000-000000000002', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 1042, 'Tenet',
     'A secret agent uses time inversion to stop a global war.', DATE '2020-08-26', 150, 7.3, 'PG-13',
     'eee00000-0000-0000-0000-000000000002'),
    ('22222222-0000-0000-0000-000000000003', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 1043, 'Gone',
     'A missing person mystery that is not available to stream.', DATE '2010-01-01', 90, 6.0, 'R',
     'eee00000-0000-0000-0000-000000000003')
ON CONFLICT (movie_id) DO NOTHING;

-- Movie genres ------------------------------------------------------------
INSERT INTO movie_genres (movie_id, library_id, batch_id, genre_id, name) VALUES
    ('22222222-0000-0000-0000-000000000001', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 878, 'Science Fiction'),
    ('22222222-0000-0000-0000-000000000001', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 18,  'Drama'),
    ('22222222-0000-0000-0000-000000000002', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 878, 'Science Fiction'),
    ('22222222-0000-0000-0000-000000000002', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 53,  'Thriller'),
    ('22222222-0000-0000-0000-000000000003', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 9648, 'Mystery')
ON CONFLICT (movie_id, genre_id) DO NOTHING;

-- People / credits --------------------------------------------------------
INSERT INTO persons (id, tmdb_id, name) VALUES
    ('99999999-0000-0000-0000-000000000001', 5001, 'Amy Adams'),
    ('99999999-0000-0000-0000-000000000002', 5002, 'John David Washington')
ON CONFLICT (tmdb_id) DO NOTHING;

INSERT INTO movie_cast
    (movie_id, library_id, batch_id, person_tmdb_id, person_id, credit_id, cast_id, "character", order_index, profile_image_id)
VALUES
    ('22222222-0000-0000-0000-000000000001', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 5001, '99999999-0000-0000-0000-000000000001', 'arrival-cast-1', 1, 'Louise Banks', 0, NULL),
    ('22222222-0000-0000-0000-000000000002', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 5002, '99999999-0000-0000-0000-000000000002', 'tenet-cast-1',   1, 'The Protagonist', 0, NULL)
ON CONFLICT (movie_id, person_tmdb_id, "character") DO NOTHING;

-- Series / season / episode hierarchy (library C) ------------------------
INSERT INTO series (id, library_id, tmdb_id, title)
VALUES ('44444444-0000-0000-0000-000000000001', 'cccccccc-cccc-cccc-cccc-cccccccccccc', 2001, 'Foundation')
ON CONFLICT (id) DO NOTHING;

INSERT INTO season_references (id, series_id, season_number, tmdb_series_id, library_id)
VALUES ('55555555-0000-0000-0000-000000000001', '44444444-0000-0000-0000-000000000001', 1, 2001, 'cccccccc-cccc-cccc-cccc-cccccccccccc')
ON CONFLICT (id) DO NOTHING;

INSERT INTO episode_references (id, series_id, season_id, file_id, season_number, episode_number, tmdb_series_id)
VALUES
    ('66666666-0000-0000-0000-000000000001', '44444444-0000-0000-0000-000000000001', '55555555-0000-0000-0000-000000000001', '77777777-0000-0000-0000-000000000001', 1, 1, 2001),
    ('66666666-0000-0000-0000-000000000002', '44444444-0000-0000-0000-000000000001', '55555555-0000-0000-0000-000000000001', '77777777-0000-0000-0000-000000000002', 1, 2, 2001)
ON CONFLICT (id) DO NOTHING;

INSERT INTO series_metadata
    (series_id, tmdb_id, name, overview, first_air_date, number_of_seasons, number_of_episodes,
     vote_average, primary_content_rating, primary_poster_image_id)
VALUES
    ('44444444-0000-0000-0000-000000000001', 2001, 'Foundation',
     'A complex saga of humans scattered on planets trying to save civilization.',
     DATE '2021-09-24', 2, 20, 8.0, 'TV-14', 'eee00000-0000-0000-0000-000000000010')
ON CONFLICT (series_id) DO NOTHING;

INSERT INTO season_metadata
    (season_id, tmdb_id, series_tmdb_id, name, overview, air_date, episode_count, primary_poster_image_id)
VALUES
    ('55555555-0000-0000-0000-000000000001', 20011, 2001, 'Season 1',
     'The opening season establishing the fall and flight of civilization.',
     DATE '2021-09-24', 10, 'eee00000-0000-0000-0000-000000000011')
ON CONFLICT (season_id) DO NOTHING;

INSERT INTO episode_metadata
    (episode_id, tmdb_id, series_tmdb_id, season_tmdb_id, season_number, episode_number, name, overview, air_date, runtime)
VALUES
    ('66666666-0000-0000-0000-000000000001', 2001101, 2001, 20011, 1, 1, 'The Emperor''s Peace',
     'A young mathematician foresees the end of the galactic empire.', DATE '2021-09-24', 75),
    ('66666666-0000-0000-0000-000000000002', 2001102, 2001, 20011, 1, 2, 'Preparing to Live',
     'The second episode is not available to stream.', DATE '2021-10-01', 60)
ON CONFLICT (episode_id) DO NOTHING;

INSERT INTO series_genres (series_id, genre_id, name) VALUES
    ('44444444-0000-0000-0000-000000000001', 878, 'Science Fiction'),
    ('44444444-0000-0000-0000-000000000001', 18,  'Drama')
ON CONFLICT (series_id, genre_id) DO NOTHING;

-- User watch progress (Alice) ---------------------------------------------
-- media_type smallint: 0=movie, 1=series, 2=season, 3=episode.
INSERT INTO user_watch_progress (user_id, position, duration, last_watched, updated_at, media_uuid, media_type)
VALUES
    ('11111111-0000-0000-0000-000000000001', 3600.0, 6960.0, 1700000001, 1700000001, '22222222-0000-0000-0000-000000000001', 0),
    ('11111111-0000-0000-0000-000000000001', 6000.0, 6960.0, 1700000002, 1700000002, '22222222-0000-0000-0000-000000000002', 0)
ON CONFLICT (user_id, media_uuid) DO NOTHING;
