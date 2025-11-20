-- Add TV show specific fields to external_metadata
ALTER TABLE external_metadata 
ADD COLUMN IF NOT EXISTS show_description TEXT,
ADD COLUMN IF NOT EXISTS show_poster_path TEXT,
ADD COLUMN IF NOT EXISTS season_poster_path TEXT,
ADD COLUMN IF NOT EXISTS episode_still_path TEXT;

-- Add indexes for better performance
CREATE INDEX IF NOT EXISTS idx_external_metadata_media_file_id 
ON external_metadata(media_file_id);

CREATE INDEX IF NOT EXISTS idx_external_metadata_tmdb_id 
ON external_metadata((metadata_json->>'tmdb_id')) 
WHERE metadata_json IS NOT NULL;