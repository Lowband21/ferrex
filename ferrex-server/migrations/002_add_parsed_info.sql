-- Add parsed_info column to store filename parsing results
ALTER TABLE media_metadata 
ADD COLUMN IF NOT EXISTS parsed_info JSONB;

-- Create index for searching by show name
CREATE INDEX IF NOT EXISTS idx_media_metadata_show_name 
ON media_metadata ((parsed_info->>'show_name')) 
WHERE parsed_info IS NOT NULL;

-- Create index for searching by media type
CREATE INDEX IF NOT EXISTS idx_media_metadata_media_type 
ON media_metadata ((parsed_info->>'media_type')) 
WHERE parsed_info IS NOT NULL;