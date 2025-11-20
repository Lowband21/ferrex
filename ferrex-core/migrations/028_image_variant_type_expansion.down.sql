BEGIN;

ALTER TABLE media_image_variants DROP CONSTRAINT media_image_variants_image_type_check;

UPDATE media_image_variants SET image_type = 'still' WHERE image_type = 'thumbnail';
UPDATE media_image_variants SET image_type = 'profile' WHERE image_type = 'cast';

ALTER TABLE media_image_variants
    ADD CONSTRAINT media_image_variants_image_type_check
    CHECK (image_type IN ('poster','backdrop','logo','still','profile'));

COMMIT;
