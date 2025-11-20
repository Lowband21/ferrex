BEGIN;

ALTER TABLE image_variants
    DROP COLUMN IF EXISTS downloaded_at;

DROP INDEX IF EXISTS idx_media_image_variants_cached;
DROP TABLE IF EXISTS media_image_variants;

ALTER TABLE movie_cast
    DROP COLUMN IF EXISTS profile_image_id;

ALTER TABLE series_cast
    DROP COLUMN IF EXISTS profile_image_id;

ALTER TABLE episode_cast
    DROP COLUMN IF EXISTS profile_image_id;

ALTER TABLE episode_guest_stars
    DROP COLUMN IF EXISTS profile_image_id;

ALTER TABLE orchestrator_jobs
    DROP CONSTRAINT IF EXISTS orchestrator_jobs_kind_check;

ALTER TABLE orchestrator_jobs
    ADD CONSTRAINT orchestrator_jobs_kind_check
    CHECK (kind IN ('scan','analyze','metadata','index'));

COMMIT;
