-- Add image registry tables for efficient image caching and management

-- Image registry with deduplication support
CREATE TABLE images (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tmdb_path TEXT NOT NULL UNIQUE,     -- e.g., "/8uO0gUM8aNqYLs1OsTBQiXu0fEv.jpg"
    file_hash VARCHAR(64) UNIQUE,        -- SHA256 for deduplication
    file_size INTEGER,
    width INTEGER,
    height INTEGER,
    format VARCHAR(10),                  -- jpg, png, webp
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Image variants (different sizes we've cached)
CREATE TABLE image_variants (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    image_id UUID NOT NULL REFERENCES images(id) ON DELETE CASCADE,
    variant VARCHAR(20) NOT NULL,        -- "w92", "w200", "w500", "original", etc.
    file_path TEXT NOT NULL,             -- Local filesystem path
    file_size INTEGER NOT NULL,
    width INTEGER,
    height INTEGER,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    UNIQUE(image_id, variant)
);

-- Link images to media items
CREATE TABLE media_images (
    media_type VARCHAR(20) NOT NULL CHECK (media_type IN ('movie', 'series', 'season', 'episode', 'person')),
    media_id UUID NOT NULL,              -- References the appropriate table based on media_type
    image_id UUID NOT NULL REFERENCES images(id) ON DELETE CASCADE,
    image_type VARCHAR(20) NOT NULL CHECK (image_type IN ('poster', 'backdrop', 'logo', 'still', 'profile')),
    order_index INTEGER NOT NULL DEFAULT 0,
    is_primary BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    PRIMARY KEY (media_type, media_id, image_type, order_index)
);

-- Performance indices
CREATE INDEX idx_images_tmdb_path ON images(tmdb_path);
CREATE INDEX idx_images_hash ON images(file_hash);
CREATE INDEX idx_images_created_at ON images(created_at);

CREATE INDEX idx_image_variants_image_id ON image_variants(image_id);
CREATE INDEX idx_image_variants_variant ON image_variants(variant);
CREATE INDEX idx_image_variants_file_path ON image_variants(file_path);

CREATE INDEX idx_media_images_lookup ON media_images(media_type, media_id);
CREATE INDEX idx_media_images_image_id ON media_images(image_id);
CREATE INDEX idx_media_images_primary ON media_images(media_type, media_id, is_primary) WHERE is_primary = true;

-- Update triggers for images table
CREATE TRIGGER update_images_updated_at BEFORE UPDATE ON images 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Add comments for documentation
COMMENT ON TABLE images IS 'Registry of all images with deduplication support';
COMMENT ON TABLE image_variants IS 'Different size variants of images cached locally';
COMMENT ON TABLE media_images IS 'Links images to media items (movies, series, etc)';
COMMENT ON COLUMN images.tmdb_path IS 'Original TMDB path like /abc123.jpg';
COMMENT ON COLUMN images.file_hash IS 'SHA256 hash for deduplication';
COMMENT ON COLUMN image_variants.variant IS 'TMDB size variant: w92, w154, w185, w342, w500, w780, original';
COMMENT ON COLUMN media_images.is_primary IS 'Marks the primary image for quick lookups';