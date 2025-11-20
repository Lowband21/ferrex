-- TMDB metadata normalization overhaul

BEGIN;

-- Drop legacy metadata tables and associated indexes
DROP INDEX IF EXISTS idx_movie_metadata_tmdb_details;
DROP INDEX IF EXISTS idx_movie_metadata_keywords;
DROP INDEX IF EXISTS idx_series_metadata_tmdb_details;
DROP INDEX IF EXISTS idx_series_metadata_keywords;
DROP INDEX IF EXISTS idx_season_metadata_tmdb_details;
DROP INDEX IF EXISTS idx_episode_metadata_tmdb_details;

DROP TABLE IF EXISTS episode_metadata CASCADE;
DROP TABLE IF EXISTS season_metadata CASCADE;
DROP TABLE IF EXISTS series_metadata CASCADE;
DROP TABLE IF EXISTS movie_metadata CASCADE;

-- Persons catalog (shared across media)
CREATE TABLE persons (
    tmdb_id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    original_name TEXT,
    gender SMALLINT,
    known_for_department TEXT,
    profile_path TEXT,
    adult BOOLEAN,
    popularity REAL,
    biography TEXT,
    birthday DATE,
    deathday DATE,
    place_of_birth TEXT,
    homepage TEXT,
    imdb_id TEXT,
    facebook_id TEXT,
    instagram_id TEXT,
    twitter_id TEXT,
    wikidata_id TEXT,
    tiktok_id TEXT,
    youtube_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE person_aliases (
    tmdb_id BIGINT REFERENCES persons(tmdb_id) ON DELETE CASCADE,
    alias TEXT NOT NULL,
    PRIMARY KEY (tmdb_id, alias)
);

CREATE TRIGGER update_persons_updated_at
BEFORE UPDATE ON persons
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

-- Movie metadata core table
CREATE TABLE movie_metadata (
    movie_id UUID PRIMARY KEY REFERENCES movie_references(id) ON DELETE CASCADE,
    tmdb_id BIGINT NOT NULL,
    title TEXT NOT NULL,
    original_title TEXT,
    overview TEXT,
    release_date DATE,
    runtime INTEGER,
    vote_average REAL,
    vote_count INTEGER,
    popularity REAL,
    primary_certification TEXT,
    homepage TEXT,
    status TEXT,
    tagline TEXT,
    budget BIGINT,
    revenue BIGINT,
    poster_path TEXT,
    backdrop_path TEXT,
    logo_path TEXT,
    collection_id BIGINT,
    collection_name TEXT,
    collection_poster_path TEXT,
    collection_backdrop_path TEXT,
    imdb_id TEXT,
    facebook_id TEXT,
    instagram_id TEXT,
    twitter_id TEXT,
    wikidata_id TEXT,
    tiktok_id TEXT,
    youtube_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_movie_metadata_tmdb_id ON movie_metadata(tmdb_id);
CREATE INDEX idx_movie_metadata_release_date ON movie_metadata(release_date);
CREATE INDEX idx_movie_metadata_title_search ON movie_metadata USING GIN (to_tsvector('english', title));

CREATE TRIGGER update_movie_metadata_updated_at
BEFORE UPDATE ON movie_metadata
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE movie_genres (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    genre_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (movie_id, genre_id)
);

CREATE TABLE movie_spoken_languages (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    iso_639_1 TEXT,
    name TEXT NOT NULL,
    PRIMARY KEY (movie_id, name)
);

CREATE TABLE movie_production_companies (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    company_id BIGINT,
    name TEXT NOT NULL,
    origin_country TEXT,
    PRIMARY KEY (movie_id, name)
);

CREATE TABLE movie_production_countries (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (movie_id, iso_3166_1)
);

CREATE TABLE movie_release_dates (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    iso_639_1 TEXT,
    certification TEXT,
    release_date TIMESTAMPTZ,
    release_type SMALLINT,
    note TEXT,
    descriptors TEXT[] DEFAULT ARRAY[]::TEXT[],
    PRIMARY KEY (movie_id, iso_3166_1, release_type, release_date)
);

CREATE TABLE movie_alternative_titles (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT,
    title TEXT NOT NULL,
    title_type TEXT,
    PRIMARY KEY (movie_id, title)
);

CREATE TABLE movie_translations (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    iso_639_1 TEXT NOT NULL,
    name TEXT,
    english_name TEXT,
    title TEXT,
    overview TEXT,
    homepage TEXT,
    tagline TEXT,
    PRIMARY KEY (movie_id, iso_3166_1, iso_639_1)
);

CREATE TABLE movie_videos (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    video_key TEXT NOT NULL,
    site TEXT NOT NULL,
    name TEXT,
    video_type TEXT,
    official BOOLEAN,
    iso_639_1 TEXT,
    iso_3166_1 TEXT,
    published_at TIMESTAMPTZ,
    size INTEGER,
    PRIMARY KEY (movie_id, video_key, site)
);

CREATE TABLE movie_keywords (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    keyword_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (movie_id, keyword_id)
);

CREATE TABLE movie_recommendations (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    recommended_tmdb_id BIGINT NOT NULL,
    title TEXT,
    PRIMARY KEY (movie_id, recommended_tmdb_id)
);

CREATE TABLE movie_similar (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    similar_tmdb_id BIGINT NOT NULL,
    title TEXT,
    PRIMARY KEY (movie_id, similar_tmdb_id)
);

CREATE TABLE movie_collection_membership (
    movie_id UUID PRIMARY KEY REFERENCES movie_references(id) ON DELETE CASCADE,
    collection_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    poster_path TEXT,
    backdrop_path TEXT
);

CREATE TABLE movie_cast (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    person_tmdb_id BIGINT REFERENCES persons(tmdb_id) ON DELETE CASCADE,
    credit_id TEXT,
    cast_id BIGINT,
    character TEXT,
    order_index INTEGER,
    PRIMARY KEY (movie_id, person_tmdb_id, character)
);

CREATE TABLE movie_crew (
    movie_id UUID REFERENCES movie_references(id) ON DELETE CASCADE,
    person_tmdb_id BIGINT REFERENCES persons(tmdb_id) ON DELETE CASCADE,
    credit_id TEXT,
    department TEXT NOT NULL,
    job TEXT NOT NULL,
    PRIMARY KEY (movie_id, person_tmdb_id, department, job)
);

-- Series metadata core table
CREATE TABLE series_metadata (
    series_id UUID PRIMARY KEY REFERENCES series_references(id) ON DELETE CASCADE,
    tmdb_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    original_name TEXT,
    overview TEXT,
    first_air_date DATE,
    last_air_date DATE,
    number_of_seasons INTEGER,
    number_of_episodes INTEGER,
    vote_average REAL,
    vote_count INTEGER,
    popularity REAL,
    primary_content_rating TEXT,
    homepage TEXT,
    status TEXT,
    tagline TEXT,
    in_production BOOLEAN,
    poster_path TEXT,
    backdrop_path TEXT,
    logo_path TEXT,
    imdb_id TEXT,
    tvdb_id BIGINT,
    facebook_id TEXT,
    instagram_id TEXT,
    twitter_id TEXT,
    wikidata_id TEXT,
    tiktok_id TEXT,
    youtube_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_series_metadata_tmdb_id ON series_metadata(tmdb_id);
CREATE INDEX idx_series_metadata_first_air ON series_metadata(first_air_date);
CREATE INDEX idx_series_metadata_title_search ON series_metadata USING GIN (to_tsvector('english', name));

CREATE TRIGGER update_series_metadata_updated_at
BEFORE UPDATE ON series_metadata
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE series_genres (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    genre_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (series_id, genre_id)
);

CREATE TABLE series_origin_countries (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    PRIMARY KEY (series_id, iso_3166_1)
);

CREATE TABLE series_spoken_languages (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    iso_639_1 TEXT,
    name TEXT NOT NULL,
    PRIMARY KEY (series_id, name)
);

CREATE TABLE series_production_companies (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    company_id BIGINT,
    name TEXT NOT NULL,
    origin_country TEXT,
    PRIMARY KEY (series_id, name)
);

CREATE TABLE series_production_countries (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (series_id, iso_3166_1)
);

CREATE TABLE series_networks (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    network_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    origin_country TEXT,
    PRIMARY KEY (series_id, network_id)
);

CREATE TABLE series_content_ratings (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    rating TEXT,
    rating_system TEXT,
    descriptors TEXT[] DEFAULT ARRAY[]::TEXT[],
    PRIMARY KEY (series_id, iso_3166_1)
);

CREATE TABLE series_episode_groups (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    group_type TEXT,
    PRIMARY KEY (series_id, group_id)
);

CREATE TABLE series_keywords (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    keyword_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (series_id, keyword_id)
);

CREATE TABLE series_videos (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    video_key TEXT NOT NULL,
    site TEXT NOT NULL,
    name TEXT,
    video_type TEXT,
    official BOOLEAN,
    iso_639_1 TEXT,
    iso_3166_1 TEXT,
    published_at TIMESTAMPTZ,
    size INTEGER,
    PRIMARY KEY (series_id, video_key, site)
);

CREATE TABLE series_translations (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    iso_639_1 TEXT NOT NULL,
    name TEXT,
    english_name TEXT,
    title TEXT,
    overview TEXT,
    homepage TEXT,
    tagline TEXT,
    PRIMARY KEY (series_id, iso_3166_1, iso_639_1)
);

CREATE TABLE series_recommendations (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    recommended_tmdb_id BIGINT NOT NULL,
    title TEXT,
    PRIMARY KEY (series_id, recommended_tmdb_id)
);

CREATE TABLE series_similar (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    similar_tmdb_id BIGINT NOT NULL,
    title TEXT,
    PRIMARY KEY (series_id, similar_tmdb_id)
);

CREATE TABLE series_cast (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    person_tmdb_id BIGINT REFERENCES persons(tmdb_id) ON DELETE CASCADE,
    credit_id TEXT,
    character TEXT,
    total_episode_count INTEGER,
    order_index INTEGER,
    PRIMARY KEY (series_id, person_tmdb_id, character)
);

CREATE TABLE series_crew (
    series_id UUID REFERENCES series_references(id) ON DELETE CASCADE,
    person_tmdb_id BIGINT REFERENCES persons(tmdb_id) ON DELETE CASCADE,
    credit_id TEXT,
    department TEXT NOT NULL,
    job TEXT NOT NULL,
    PRIMARY KEY (series_id, person_tmdb_id, department, job)
);

-- Season metadata core table
CREATE TABLE season_metadata (
    season_id UUID PRIMARY KEY REFERENCES season_references(id) ON DELETE CASCADE,
    tmdb_id BIGINT NOT NULL,
    series_tmdb_id BIGINT,
    name TEXT,
    overview TEXT,
    air_date DATE,
    episode_count INTEGER,
    poster_path TEXT,
    runtime INTEGER,
    vote_average REAL,
    vote_count INTEGER,
    imdb_id TEXT,
    facebook_id TEXT,
    instagram_id TEXT,
    twitter_id TEXT,
    wikidata_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER update_season_metadata_updated_at
BEFORE UPDATE ON season_metadata
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE season_keywords (
    season_id UUID REFERENCES season_references(id) ON DELETE CASCADE,
    keyword_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (season_id, keyword_id)
);

CREATE TABLE season_videos (
    season_id UUID REFERENCES season_references(id) ON DELETE CASCADE,
    video_key TEXT NOT NULL,
    site TEXT NOT NULL,
    name TEXT,
    video_type TEXT,
    official BOOLEAN,
    iso_639_1 TEXT,
    iso_3166_1 TEXT,
    published_at TIMESTAMPTZ,
    size INTEGER,
    PRIMARY KEY (season_id, video_key, site)
);

CREATE TABLE season_translations (
    season_id UUID REFERENCES season_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    iso_639_1 TEXT NOT NULL,
    name TEXT,
    english_name TEXT,
    title TEXT,
    overview TEXT,
    homepage TEXT,
    tagline TEXT,
    PRIMARY KEY (season_id, iso_3166_1, iso_639_1)
);

-- Episode metadata core table
CREATE TABLE episode_metadata (
    episode_id UUID PRIMARY KEY REFERENCES episode_references(id) ON DELETE CASCADE,
    tmdb_id BIGINT NOT NULL,
    series_tmdb_id BIGINT,
    season_tmdb_id BIGINT,
    season_number INTEGER,
    episode_number INTEGER,
    name TEXT,
    overview TEXT,
    air_date DATE,
    runtime INTEGER,
    still_path TEXT,
    vote_average REAL,
    vote_count INTEGER,
    production_code TEXT,
    imdb_id TEXT,
    tvdb_id BIGINT,
    facebook_id TEXT,
    instagram_id TEXT,
    twitter_id TEXT,
    wikidata_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER update_episode_metadata_updated_at
BEFORE UPDATE ON episode_metadata
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE episode_keywords (
    episode_id UUID REFERENCES episode_references(id) ON DELETE CASCADE,
    keyword_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (episode_id, keyword_id)
);

CREATE TABLE episode_videos (
    episode_id UUID REFERENCES episode_references(id) ON DELETE CASCADE,
    video_key TEXT NOT NULL,
    site TEXT NOT NULL,
    name TEXT,
    video_type TEXT,
    official BOOLEAN,
    iso_639_1 TEXT,
    iso_3166_1 TEXT,
    published_at TIMESTAMPTZ,
    size INTEGER,
    PRIMARY KEY (episode_id, video_key, site)
);

CREATE TABLE episode_translations (
    episode_id UUID REFERENCES episode_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    iso_639_1 TEXT NOT NULL,
    name TEXT,
    english_name TEXT,
    title TEXT,
    overview TEXT,
    homepage TEXT,
    tagline TEXT,
    PRIMARY KEY (episode_id, iso_3166_1, iso_639_1)
);

CREATE TABLE episode_cast (
    episode_id UUID REFERENCES episode_references(id) ON DELETE CASCADE,
    person_tmdb_id BIGINT REFERENCES persons(tmdb_id) ON DELETE CASCADE,
    credit_id TEXT,
    character TEXT,
    order_index INTEGER,
    PRIMARY KEY (episode_id, person_tmdb_id, character)
);

CREATE TABLE episode_guest_stars (
    episode_id UUID REFERENCES episode_references(id) ON DELETE CASCADE,
    person_tmdb_id BIGINT REFERENCES persons(tmdb_id) ON DELETE CASCADE,
    credit_id TEXT,
    character TEXT,
    order_index INTEGER,
    PRIMARY KEY (episode_id, person_tmdb_id, character)
);

CREATE TABLE episode_crew (
    episode_id UUID REFERENCES episode_references(id) ON DELETE CASCADE,
    person_tmdb_id BIGINT REFERENCES persons(tmdb_id) ON DELETE CASCADE,
    credit_id TEXT,
    department TEXT NOT NULL,
    job TEXT NOT NULL,
    PRIMARY KEY (episode_id, person_tmdb_id, department, job)
);

CREATE TABLE episode_content_ratings (
    episode_id UUID REFERENCES episode_references(id) ON DELETE CASCADE,
    iso_3166_1 TEXT NOT NULL,
    rating TEXT,
    rating_system TEXT,
    descriptors TEXT[] DEFAULT ARRAY[]::TEXT[],
    PRIMARY KEY (episode_id, iso_3166_1)
);

COMMIT;
