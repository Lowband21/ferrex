BEGIN;

ALTER TABLE movie_alternative_titles
    DROP CONSTRAINT IF EXISTS movie_alternative_titles_pkey;

ALTER TABLE movie_alternative_titles
    ADD COLUMN iso_3166_1_key TEXT GENERATED ALWAYS AS (COALESCE(iso_3166_1, '')) STORED;

ALTER TABLE movie_alternative_titles
    ADD COLUMN title_type_key TEXT GENERATED ALWAYS AS (COALESCE(title_type, '')) STORED;

ALTER TABLE movie_alternative_titles
    ADD CONSTRAINT movie_alternative_titles_pkey
        PRIMARY KEY (movie_id, iso_3166_1_key, title_type_key, title);

COMMIT;
