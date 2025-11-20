-- Restore original media image categories
ALTER TABLE media_images DROP CONSTRAINT media_images_image_type_check;

UPDATE media_images SET image_type = 'still' WHERE image_type = 'thumbnail';
UPDATE media_images SET image_type = 'profile' WHERE image_type = 'cast';

ALTER TABLE media_images
    ADD CONSTRAINT media_images_image_type_check
    CHECK (image_type IN ('poster', 'backdrop', 'logo', 'still', 'profile'));
