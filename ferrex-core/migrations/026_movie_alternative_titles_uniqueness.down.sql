BEGIN;

ALTER TABLE movie_alternative_titles
    DROP CONSTRAINT IF EXISTS movie_alternative_titles_pkey;

ALTER TABLE movie_alternative_titles
    DROP COLUMN IF EXISTS iso_3166_1_key;

ALTER TABLE movie_alternative_titles
    DROP COLUMN IF EXISTS title_type_key;

ALTER TABLE movie_alternative_titles
    ADD CONSTRAINT movie_alternative_titles_pkey
        PRIMARY KEY (movie_id, title);

COMMIT;
