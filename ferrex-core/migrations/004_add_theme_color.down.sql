-- Remove theme_color columns

ALTER TABLE movie_references DROP COLUMN IF EXISTS theme_color;
ALTER TABLE series_references DROP COLUMN IF EXISTS theme_color;
ALTER TABLE season_references DROP COLUMN IF EXISTS theme_color;