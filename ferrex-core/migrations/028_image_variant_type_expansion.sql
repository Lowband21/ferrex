BEGIN;

ALTER TABLE media_image_variants DROP CONSTRAINT media_image_variants_image_type_check;

UPDATE media_image_variants SET image_type = 'thumbnail' WHERE image_type = 'still';
UPDATE media_image_variants SET image_type = 'cast' WHERE image_type = 'profile';

ALTER TABLE media_image_variants
    ADD CONSTRAINT media_image_variants_image_type_check
    CHECK (image_type IN ('poster','backdrop','logo','thumbnail','cast'));

COMMIT;
