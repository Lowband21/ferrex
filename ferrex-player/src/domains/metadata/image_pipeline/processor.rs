//! Image processor implementation with multi-resolution support

use super::{ImagePipelineError, ImageProcessor, ProcessOptions, ProcessedImage, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use iced::widget::image::Handle;
use image::{DynamicImage, ImageOutputFormat};
use std::io::Cursor;
use std::sync::Arc;

/// Image processor with parallel processing support
pub struct DefaultImageProcessor {
    /// Thread pool for CPU-intensive operations
    pool: rayon::ThreadPool,
}

impl DefaultImageProcessor {
    /// Create a new image processor
    pub fn new() -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get().min(16))
            .thread_name(|idx| format!("image-processor-{}", idx))
            .build()
            .expect("Failed to create thread pool");

        Self { pool }
    }
}

#[async_trait::async_trait]
impl ImageProcessor for DefaultImageProcessor {
    async fn process(&self, data: &[u8], options: ProcessOptions) -> Result<ProcessedImage> {
        let data = data.to_vec();
        let options = options.clone();

        // Process in thread pool to avoid blocking
        let result = tokio::task::spawn_blocking(move || process_image_sync(&data, options))
            .await
            .map_err(|e| ImagePipelineError::Processing(e.to_string()))?;

        result
    }

    async fn generate_lqip(&self, image: &DynamicImage) -> Result<String> {
        let image = image.clone();

        tokio::task::spawn_blocking(move || {
            // Generate tiny 20x20 thumbnail
            let tiny = image.thumbnail(20, 20);

            // Encode as JPEG with low quality
            let mut buffer = Vec::new();
            let mut cursor = Cursor::new(&mut buffer);
            tiny.write_to(&mut cursor, ImageOutputFormat::Jpeg(40))
                .map_err(|e| ImagePipelineError::Processing(e.to_string()))?;

            // Base64 encode
            Ok(BASE64.encode(&buffer))
        })
        .await
        .map_err(|e| ImagePipelineError::Processing(e.to_string()))?
    }

    async fn calculate_dominant_color(&self, image: &DynamicImage) -> Result<[u8; 3]> {
        let image = image.clone();

        tokio::task::spawn_blocking(move || calculate_dominant_color_sync(&image))
            .await
            .map_err(|e| ImagePipelineError::Processing(e.to_string()))?
    }
}

fn process_image_sync(data: &[u8], options: ProcessOptions) -> Result<ProcessedImage> {
    // Decode image
    let img =
        image::load_from_memory(data).map_err(|e| ImagePipelineError::Decode(e.to_string()))?;

    let original_size = (img.width(), img.height());

    // Calculate target dimensions based on image size
    let (target_width, target_height) = match options.size {
        crate::domains::metadata::image_types::ImageSize::Thumbnail => (150, 225),
        crate::domains::metadata::image_types::ImageSize::Poster => (300, 450),
        crate::domains::metadata::image_types::ImageSize::Backdrop => (800, 450),
        crate::domains::metadata::image_types::ImageSize::Full => (600, 900),
        crate::domains::metadata::image_types::ImageSize::Profile => (120, 180), // 2:3 aspect ratio for cast
    };

    // Resize if needed
    let processed = if img.width() > target_width || img.height() > target_height {
        img.thumbnail(target_width, target_height)
    } else {
        img.clone()
    };

    let processed_size = (processed.width(), processed.height());

    // Generate thumbnail if requested
    let thumbnail = if options.generate_thumbnail {
        Some(Arc::new(processed.thumbnail(100, 150)))
    } else {
        None
    };

    // Generate LQIP if requested
    let lqip = if options.calculate_dominant_color {
        let tiny = processed.thumbnail(20, 20);
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        tiny.write_to(&mut cursor, ImageOutputFormat::Jpeg(40))
            .map_err(|e| ImagePipelineError::Processing(e.to_string()))?;
        Some(BASE64.encode(&buffer))
    } else {
        None
    };

    // Calculate dominant color if requested
    let dominant_color = if options.calculate_dominant_color {
        Some(calculate_dominant_color_sync(&processed)?)
    } else {
        None
    };

    // Create Iced handle
    let handle = create_iced_handle(&processed)?;

    Ok(ProcessedImage {
        image: Arc::new(processed),
        handle,
        thumbnail,
        lqip,
        dominant_color,
        original_size,
        processed_size,
    })
}

fn calculate_dominant_color_sync(image: &DynamicImage) -> Result<[u8; 3]> {
    let rgba = image.to_rgba8();
    let pixels: Vec<_> = rgba.pixels().collect();

    // Simple average for now - could be improved with k-means clustering
    let (mut r_sum, mut g_sum, mut b_sum, mut count) = (0u64, 0u64, 0u64, 0u64);

    for pixel in pixels.iter().step_by(10) {
        // Sample every 10th pixel for speed
        let [r, g, b, a] = pixel.0;
        if a > 128 {
            // Only count non-transparent pixels
            r_sum += r as u64;
            g_sum += g as u64;
            b_sum += b as u64;
            count += 1;
        }
    }

    if count == 0 {
        return Ok([128, 128, 128]); // Gray fallback
    }

    Ok([
        (r_sum / count) as u8,
        (g_sum / count) as u8,
        (b_sum / count) as u8,
    ])
}

fn create_iced_handle(image: &DynamicImage) -> Result<Handle> {
    // Convert to PNG bytes for Iced handle
    let mut buffer = Vec::new();
    let mut cursor = Cursor::new(&mut buffer);
    image
        .write_to(&mut cursor, ImageOutputFormat::Png)
        .map_err(|e| ImagePipelineError::Processing(e.to_string()))?;

    Ok(Handle::from_bytes(buffer))
}
