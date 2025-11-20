use crate::database::traits::{ImageRecord, ImageVariant, ImageLookupParams};
use crate::database::MediaDatabase;
use crate::{Result, MediaError};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use sha2::{Sha256, Digest};
use uuid::Uuid;

/// TMDB image size variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmdbImageSize {
    // Poster sizes
    PosterW92,
    PosterW154,
    PosterW185,
    PosterW342,
    PosterW500,
    PosterW780,
    // Backdrop sizes
    BackdropW300,
    BackdropW780,
    BackdropW1280,
    // Still sizes
    StillW92,
    StillW185,
    StillW300,
    StillW500,
    // Profile sizes
    ProfileW45,
    ProfileW185,
    ProfileH632,
    // Original
    Original,
}

impl TmdbImageSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            TmdbImageSize::PosterW92 => "w92",
            TmdbImageSize::PosterW154 => "w154",
            TmdbImageSize::PosterW185 => "w185",
            TmdbImageSize::PosterW342 => "w342",
            TmdbImageSize::PosterW500 => "w500",
            TmdbImageSize::PosterW780 => "w780",
            TmdbImageSize::BackdropW300 => "w300",
            TmdbImageSize::BackdropW780 => "w780",
            TmdbImageSize::BackdropW1280 => "w1280",
            TmdbImageSize::StillW92 => "w92",
            TmdbImageSize::StillW185 => "w185",
            TmdbImageSize::StillW300 => "w300",
            TmdbImageSize::StillW500 => "w500",
            TmdbImageSize::ProfileW45 => "w45",
            TmdbImageSize::ProfileW185 => "w185",
            TmdbImageSize::ProfileH632 => "h632",
            TmdbImageSize::Original => "original",
        }
    }
    
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "w92" => Some(TmdbImageSize::PosterW92),
            "w154" => Some(TmdbImageSize::PosterW154),
            "w185" => Some(TmdbImageSize::PosterW185),
            "w342" => Some(TmdbImageSize::PosterW342),
            "w500" => Some(TmdbImageSize::PosterW500),
            "w780" => Some(TmdbImageSize::PosterW780),
            "w300" => Some(TmdbImageSize::BackdropW300),
            "w1280" => Some(TmdbImageSize::BackdropW1280),
            "h632" => Some(TmdbImageSize::ProfileH632),
            "w45" => Some(TmdbImageSize::ProfileW45),
            "original" => Some(TmdbImageSize::Original),
            _ => None,
        }
    }
    
    /// Get recommended sizes for native client usage
    pub fn recommended_for_type(image_type: &str) -> Vec<Self> {
        match image_type {
            "poster" => vec![TmdbImageSize::PosterW185, TmdbImageSize::PosterW500],
            "backdrop" => vec![TmdbImageSize::BackdropW780, TmdbImageSize::BackdropW1280],
            "logo" => vec![TmdbImageSize::Original], // SVG logos should use original
            "still" => vec![TmdbImageSize::StillW300, TmdbImageSize::StillW500],
            "profile" => vec![TmdbImageSize::ProfileW185],
            _ => vec![TmdbImageSize::Original],
        }
    }
}

#[derive(Clone)]
pub struct ImageService {
    db: Arc<MediaDatabase>,
    cache_dir: PathBuf,
    http_client: reqwest::Client,
}

impl ImageService {
    pub fn new(db: Arc<MediaDatabase>, cache_dir: PathBuf) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
            
        Self {
            db,
            cache_dir,
            http_client,
        }
    }
    
    /// Register a TMDB image in the database
    pub async fn register_tmdb_image(&self, tmdb_path: &str) -> Result<ImageRecord> {
        // Check if already registered
        if let Some(existing) = self.db.backend().get_image_by_tmdb_path(tmdb_path).await? {
            return Ok(existing);
        }
        
        // Create new image record
        self.db.backend().create_image(tmdb_path).await
    }
    
    /// Download and cache an image variant
    /// Returns (path, variant, optional_theme_color)
    pub async fn download_variant(
        &self, 
        tmdb_path: &str, 
        size: TmdbImageSize
    ) -> Result<(PathBuf, ImageVariant, Option<String>)> {
        // First ensure image is registered
        let image_record = self.register_tmdb_image(tmdb_path).await?;
        
        // Check if variant already exists
        let variant_name = size.as_str();
        if let Some(existing) = self.db.backend().get_image_variant(image_record.id, variant_name).await? {
            // For poster variants, check if we need to extract theme color
            let theme_color = if variant_name == "w185" || variant_name == "w342" {
                // Check if the file exists and extract theme color if not in database
                let path = PathBuf::from(&existing.file_path);
                if path.exists() {
                    // Extract theme color from existing image
                    match tokio::fs::read(&path).await {
                        Ok(data) => {
                            info!("Re-extracting theme color from cached poster: {}", tmdb_path);
                            self.extract_theme_color(&data)
                        }
                        Err(e) => {
                            warn!("Failed to read cached image for theme color extraction: {}", e);
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            };
            
            return Ok((PathBuf::from(&existing.file_path), existing, theme_color));
        }
        
        // Download from TMDB
        let url = format!(
            "https://image.tmdb.org/t/p/{}/{}",
            variant_name,
            tmdb_path
        );
        
        info!("Downloading image variant: {}", url);
        
        let response = self.http_client.get(&url)
            .send()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to download image: {}", e)))?;
            
        if !response.status().is_success() {
            return Err(MediaError::Internal(format!(
                "Failed to download image: HTTP {}",
                response.status()
            )));
        }
        
        let bytes = response.bytes()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to read image data: {}", e)))?;
        
        // Create cache directory structure
        let variant_dir = self.cache_dir.join("images").join(variant_name);
        tokio::fs::create_dir_all(&variant_dir)
            .await
            .map_err(|e| MediaError::Io(e))?;
        
        // Generate filename based on tmdb_path
        let filename = tmdb_path
            .trim_start_matches('/')
            .replace('/', "_");
        let file_path = variant_dir.join(&filename);
        
        // Write to disk
        tokio::fs::write(&file_path, &bytes)
            .await
            .map_err(|e| MediaError::Io(e))?;
        
        // Get image dimensions if this is the first variant
        let (width, height) = if image_record.width.is_none() {
            match self.get_image_dimensions(&bytes) {
                Ok((w, h)) => (Some(w as i32), Some(h as i32)),
                Err(e) => {
                    warn!("Failed to get image dimensions: {}", e);
                    (None, None)
                }
            }
        } else {
            (None, None)
        };
        
        // Calculate hash for deduplication
        let hash = self.calculate_hash(&bytes);
        let format = self.detect_format(&bytes);
        
        // Extract theme color from the first poster variant
        let theme_color = if variant_name == "w185" || variant_name == "w342" {
            self.extract_theme_color(&bytes)
        } else {
            None
        };
        
        if let Some(color) = &theme_color {
            info!("Extracted theme color: {} for {}", color, tmdb_path);
            // TODO: Store theme_color in database with the image or media record
        }
        
        // Check for duplicate by hash
        if let Some(existing_image) = self.db.backend().get_image_by_hash(&hash).await? {
            info!("Found duplicate image by hash: {}", hash);
            
            // Check if this variant already exists for the duplicate
            if let Some(existing_variant) = self.db.backend().get_image_variant(existing_image.id, variant_name).await? {
                // TODO: Load theme color from database
                return Ok((PathBuf::from(&existing_variant.file_path), existing_variant, theme_color));
            }
            
            // Create variant record for existing image
            let variant = self.db.backend().create_image_variant(
                existing_image.id,
                variant_name,
                file_path.to_string_lossy().as_ref(),
                bytes.len() as i32,
                width,
                height,
            ).await?;
            
            return Ok((file_path, variant, theme_color));
        }
        
        // Update image metadata with hash
        if image_record.file_hash.is_none() {
            self.db.backend().update_image_metadata(
                image_record.id,
                &hash,
                bytes.len() as i32,
                width.unwrap_or(0),
                height.unwrap_or(0),
                &format,
            ).await?;
        }
        
        // Store variant in database
        let variant = self.db.backend().create_image_variant(
            image_record.id,
            variant_name,
            file_path.to_string_lossy().as_ref(),
            bytes.len() as i32,
            width,
            height,
        ).await?;
        
        Ok((file_path, variant, theme_color))
    }
    
    /// Link an image to a media item
    pub async fn link_to_media(
        &self,
        media_type: &str,
        media_id: Uuid,
        tmdb_path: &str,
        image_type: &str,
        order_index: i32,
        is_primary: bool,
    ) -> Result<()> {
        info!("link_to_media: type={}, id={}, tmdb_path={}, image_type={}, index={}",
              media_type, media_id, tmdb_path, image_type, order_index);
              
        // Ensure image is registered
        let image_record = self.register_tmdb_image(tmdb_path).await?;
        
        // Create the link
        self.db.backend().link_media_image(
            media_type,
            media_id,
            image_record.id,
            image_type,
            order_index,
            is_primary,
        ).await
    }
    
    /// Get or download an image variant
    pub async fn get_or_download_variant(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<PathBuf>> {
        info!("get_or_download_variant called with params: type={}, id={}, image_type={}, index={}, variant={:?}",
              params.media_type, params.media_id, params.image_type, params.index, params.variant);
              
        // Look up the image in database
        if let Some((image_record, existing_variant)) = self.db.backend().lookup_image_variant(params).await? {
            // If we have the requested variant, return it
            if let Some(variant) = existing_variant {
                return Ok(Some(PathBuf::from(variant.file_path)));
            }
            
            // Otherwise download the requested size
            if let Some(size_str) = &params.variant {
                if let Some(size) = TmdbImageSize::from_str(size_str) {
                    let (path, _, _) = self.download_variant(&image_record.tmdb_path, size).await?;
                    return Ok(Some(path));
                }
            }
            
            // Fall back to any available variant
            let variants = self.db.backend().get_image_variants(image_record.id).await?;
            if let Some(variant) = variants.first() {
                return Ok(Some(PathBuf::from(&variant.file_path)));
            }
        }
        
        Ok(None)
    }
    
    /// Calculate SHA256 hash of image data
    fn calculate_hash(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }
    
    /// Detect image format from bytes
    fn detect_format(&self, data: &[u8]) -> String {
        if data.len() < 4 {
            return "unknown".to_string();
        }
        
        match &data[0..4] {
            [0xFF, 0xD8, 0xFF, _] => "jpg",
            [0x89, 0x50, 0x4E, 0x47] => "png",
            [0x52, 0x49, 0x46, 0x46] => "webp",
            _ => "unknown",
        }.to_string()
    }
    
    /// Get image dimensions using the image crate
    fn get_image_dimensions(&self, data: &[u8]) -> Result<(u32, u32)> {
        use image::GenericImageView;
        
        let img = image::load_from_memory(data)
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to decode image: {}", e)))?;
            
        Ok(img.dimensions())
    }
    
    /// Extract dominant color from image data
    pub fn extract_theme_color(&self, data: &[u8]) -> Option<String> {
        use image::{GenericImageView, Rgba};
        use std::collections::HashMap;
        
        // Try to decode the image
        let img = match image::load_from_memory(data) {
            Ok(img) => img,
            Err(e) => {
                warn!("Failed to decode image for color extraction: {}", e);
                return None;
            }
        };
        
        let (width, height) = img.dimensions();
        
        // Skip very small images
        if width < 50 || height < 50 {
            return None;
        }
        
        // Sample pixels from a 5x5 grid, excluding 10% border
        let border_x = width / 10;
        let border_y = height / 10;
        let sample_width = width - (2 * border_x);
        let sample_height = height - (2 * border_y);
        
        let mut color_counts: HashMap<[u8; 3], u32> = HashMap::new();
        
        // Sample 25 points in a grid pattern
        for i in 0..5 {
            for j in 0..5 {
                let x = border_x + (i * sample_width / 4);
                let y = border_y + (j * sample_height / 4);
                
                let pixel = img.get_pixel(x, y);
                let Rgba([r, g, b, a]) = pixel;
                
                // Skip transparent or near-transparent pixels
                if a < 128 {
                    continue;
                }
                
                // Quantize colors to reduce noise (round to nearest 16)
                let quantized = [
                    (r / 16) * 16,
                    (g / 16) * 16,
                    (b / 16) * 16,
                ];
                
                *color_counts.entry(quantized).or_insert(0) += 1;
            }
        }
        
        // Find the most common non-grayscale color
        let mut best_color = None;
        let mut best_count = 0;
        let mut best_saturation = 0.0;
        
        for (color, count) in &color_counts {
            let [r, g, b] = *color;
            
            // Skip near-black and near-white colors
            let brightness = (r as u32 + g as u32 + b as u32) / 3;
            if brightness < 30 || brightness > 225 {
                continue;
            }
            
            // Calculate saturation (how colorful it is)
            let max = r.max(g).max(b) as f32;
            let min = r.min(g).min(b) as f32;
            let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
            
            // Prefer more saturated colors
            if *count > best_count || (*count == best_count && saturation > best_saturation) {
                best_color = Some(*color);
                best_count = *count;
                best_saturation = saturation;
            }
        }
        
        // Convert to hex color
        let hex_color = best_color.map(|[r, g, b]| format!("#{:02x}{:02x}{:02x}", r, g, b));
        if let Some(ref color) = hex_color {
            info!("Extracted theme color: {} (saturation: {:.2})", color, best_saturation);
        } else {
            info!("No suitable theme color found (sampled {} colors)", color_counts.len());
        }
        hex_color
    }
    
    /// Clean up orphaned images
    pub async fn cleanup_orphaned(&self) -> Result<u32> {
        self.db.backend().cleanup_orphaned_images().await
    }
    
    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<std::collections::HashMap<String, u64>> {
        self.db.backend().get_image_cache_stats().await
    }
}