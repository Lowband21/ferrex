BEGIN;

-- Drop new normalized tables
DROP TABLE IF EXISTS episode_content_ratings;
DROP TABLE IF EXISTS episode_crew;
DROP TABLE IF EXISTS episode_guest_stars;
DROP TABLE IF EXISTS episode_cast;
DROP TABLE IF EXISTS episode_translations;
DROP TABLE IF EXISTS episode_videos;
DROP TABLE IF EXISTS episode_keywords;
DROP TABLE IF EXISTS episode_metadata;

DROP TABLE IF EXISTS season_translations;
DROP TABLE IF EXISTS season_videos;
DROP TABLE IF EXISTS season_keywords;
DROP TABLE IF EXISTS season_metadata;

DROP TABLE IF EXISTS series_crew;
DROP TABLE IF EXISTS series_cast;
DROP TABLE IF EXISTS series_similar;
DROP TABLE IF EXISTS series_recommendations;
DROP TABLE IF EXISTS series_translations;
DROP TABLE IF EXISTS series_videos;
DROP TABLE IF EXISTS series_keywords;
DROP TABLE IF EXISTS series_episode_groups;
DROP TABLE IF EXISTS series_content_ratings;
DROP TABLE IF EXISTS series_networks;
DROP TABLE IF EXISTS series_production_countries;
DROP TABLE IF EXISTS series_production_companies;
DROP TABLE IF EXISTS series_spoken_languages;
DROP TABLE IF EXISTS series_origin_countries;
DROP TABLE IF EXISTS series_genres;
DROP TABLE IF EXISTS series_metadata;

DROP TABLE IF EXISTS movie_crew;
DROP TABLE IF EXISTS movie_cast;
DROP TABLE IF EXISTS movie_collection_membership;
DROP TABLE IF EXISTS movie_similar;
DROP TABLE IF EXISTS movie_recommendations;
DROP TABLE IF EXISTS movie_keywords;
DROP TABLE IF EXISTS movie_videos;
DROP TABLE IF EXISTS movie_translations;
DROP TABLE IF EXISTS movie_alternative_titles;
DROP TABLE IF EXISTS movie_release_dates;
DROP TABLE IF EXISTS movie_production_countries;
DROP TABLE IF EXISTS movie_production_companies;
DROP TABLE IF EXISTS movie_spoken_languages;
DROP TABLE IF EXISTS movie_genres;
DROP TABLE IF EXISTS movie_metadata;

DROP TABLE IF EXISTS person_aliases;
DROP TABLE IF EXISTS persons;

-- Recreate legacy JSON-based tables
CREATE TABLE movie_metadata (
    movie_id UUID PRIMARY KEY REFERENCES movie_references(id) ON DELETE CASCADE,
    tmdb_details JSONB NOT NULL,
    images JSONB NOT NULL DEFAULT '{"posters":[],"backdrops":[],"logos":[]}',
    cast_crew JSONB NOT NULL DEFAULT '{"cast":[],"crew":[]}',
    videos JSONB NOT NULL DEFAULT '[]',
    keywords TEXT[] DEFAULT '{}',
    external_ids JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE series_metadata (
    series_id UUID PRIMARY KEY REFERENCES series_references(id) ON DELETE CASCADE,
    tmdb_details JSONB NOT NULL,
    images JSONB NOT NULL DEFAULT '{"posters":[],"backdrops":[],"logos":[]}',
    cast_crew JSONB NOT NULL DEFAULT '{"cast":[],"crew":[]}',
    videos JSONB NOT NULL DEFAULT '[]',
    keywords TEXT[] DEFAULT '{}',
    external_ids JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE season_metadata (
    season_id UUID PRIMARY KEY REFERENCES season_references(id) ON DELETE CASCADE,
    tmdb_details JSONB NOT NULL,
    images JSONB NOT NULL DEFAULT '{"posters":[]}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE episode_metadata (
    episode_id UUID PRIMARY KEY REFERENCES episode_references(id) ON DELETE CASCADE,
    tmdb_details JSONB NOT NULL,
    still_images JSONB NOT NULL DEFAULT '{"stills":[]}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_movie_metadata_tmdb_details ON movie_metadata USING GIN (tmdb_details);
CREATE INDEX idx_movie_metadata_keywords ON movie_metadata USING GIN (keywords);
CREATE INDEX idx_series_metadata_tmdb_details ON series_metadata USING GIN (tmdb_details);
CREATE INDEX idx_series_metadata_keywords ON series_metadata USING GIN (keywords);
CREATE INDEX idx_season_metadata_tmdb_details ON season_metadata USING GIN (tmdb_details);
CREATE INDEX idx_episode_metadata_tmdb_details ON episode_metadata USING GIN (tmdb_details);

CREATE TRIGGER update_movie_metadata_updated_at
BEFORE UPDATE ON movie_metadata
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_series_metadata_updated_at
BEFORE UPDATE ON series_metadata
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_season_metadata_updated_at
BEFORE UPDATE ON season_metadata
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_episode_metadata_updated_at
BEFORE UPDATE ON episode_metadata
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

COMMIT;
