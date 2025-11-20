-- Expand media image categories for new filesystem layout
ALTER TABLE media_images DROP CONSTRAINT media_images_image_type_check;

-- Normalize existing rows to the new taxonomy
UPDATE media_images SET image_type = 'thumbnail' WHERE image_type = 'still';
UPDATE media_images SET image_type = 'cast' WHERE image_type = 'profile';

ALTER TABLE media_images
    ADD CONSTRAINT media_images_image_type_check
    CHECK (image_type IN ('poster', 'backdrop', 'logo', 'thumbnail', 'cast'));
