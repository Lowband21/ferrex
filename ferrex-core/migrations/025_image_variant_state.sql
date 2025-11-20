BEGIN;

CREATE TABLE media_image_variants (
    media_type VARCHAR(20) NOT NULL CHECK (media_type IN ('movie','series','season','episode','person')),
    media_id UUID NOT NULL,
    image_type VARCHAR(20) NOT NULL CHECK (image_type IN ('poster','backdrop','logo','still','profile')),
    order_index INTEGER NOT NULL DEFAULT 0,
    variant VARCHAR(20) NOT NULL,
    cached BOOLEAN NOT NULL DEFAULT false,
    width INTEGER,
    height INTEGER,
    content_hash VARCHAR(64),
    theme_color VARCHAR(7),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    cached_at TIMESTAMPTZ,
    PRIMARY KEY (media_type, media_id, image_type, order_index, variant),
    FOREIGN KEY (media_type, media_id, image_type, order_index)
        REFERENCES media_images(media_type, media_id, image_type, order_index)
        ON DELETE CASCADE
);

CREATE INDEX idx_media_image_variants_cached
    ON media_image_variants(media_type, media_id, image_type, variant)
    WHERE cached = true;

ALTER TABLE image_variants
    ADD COLUMN downloaded_at TIMESTAMPTZ;

UPDATE image_variants
SET downloaded_at = created_at
WHERE downloaded_at IS NULL;

ALTER TABLE image_variants
    ALTER COLUMN downloaded_at SET DEFAULT NOW();

ALTER TABLE movie_cast
    ADD COLUMN profile_image_id UUID REFERENCES images(id);

ALTER TABLE series_cast
    ADD COLUMN profile_image_id UUID REFERENCES images(id);

ALTER TABLE episode_cast
    ADD COLUMN profile_image_id UUID REFERENCES images(id);

ALTER TABLE episode_guest_stars
    ADD COLUMN profile_image_id UUID REFERENCES images(id);

ALTER TABLE orchestrator_jobs
    DROP CONSTRAINT IF EXISTS orchestrator_jobs_kind_check;

ALTER TABLE orchestrator_jobs
    ADD CONSTRAINT orchestrator_jobs_kind_check
    CHECK (kind IN ('scan','analyze','metadata','index','image'));

COMMIT;
