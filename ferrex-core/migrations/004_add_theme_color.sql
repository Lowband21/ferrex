-- Add theme_color column to media tables for storing dominant poster colors

-- Add to movie_references table
ALTER TABLE movie_references ADD COLUMN IF NOT EXISTS theme_color VARCHAR(7);

-- Add to series_references table  
ALTER TABLE series_references ADD COLUMN IF NOT EXISTS theme_color VARCHAR(7);

-- Add to season_references table
ALTER TABLE season_references ADD COLUMN IF NOT EXISTS theme_color VARCHAR(7);

-- Episodes don't need theme_color as they use thumbnails, not posters