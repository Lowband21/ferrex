-- Add HDR metadata columns to media_metadata table
ALTER TABLE media_metadata 
ADD COLUMN IF NOT EXISTS bit_depth INTEGER,
ADD COLUMN IF NOT EXISTS color_transfer TEXT,
ADD COLUMN IF NOT EXISTS color_space TEXT,
ADD COLUMN IF NOT EXISTS color_primaries TEXT;

-- Create indexes for HDR content filtering
CREATE INDEX IF NOT EXISTS idx_media_metadata_bit_depth 
ON media_metadata(bit_depth) 
WHERE bit_depth IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_media_metadata_color_transfer 
ON media_metadata(color_transfer) 
WHERE color_transfer IS NOT NULL;