//! SDF Font Atlas generation using fontdue.
//!
//! Generates a Signed Distance Field texture atlas from a monospace font
//! for high-quality text rendering in the poster backface menu shader.

use std::collections::HashMap;

/// Embedded monospace font (JetBrains Mono Regular)
/// Using a subset for menu labels: A-Z, 0-9, and common punctuation
const EMBEDDED_FONT: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf");

/// Characters to include in the atlas
const ATLAS_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .-:!?";

/// SDF generation parameters
const ATLAS_SIZE: u32 = 512;
const GLYPH_SIZE: f32 = 48.0; // Render size for SDF generation
const SDF_PADDING: u32 = 8;   // Padding around each glyph for SDF spread

/// Glyph metrics for shader-side text rendering
#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    /// UV coordinates in atlas (min_u, min_v, max_u, max_v)
    pub uv: [f32; 4],
    /// Horizontal advance (normalized to glyph height)
    pub advance: f32,
    /// Bearing offset (normalized)
    pub bearing_x: f32,
    pub bearing_y: f32,
    /// Glyph size (normalized to glyph height)
    pub width: f32,
    pub height: f32,
}

/// Font atlas containing SDF texture data and glyph metrics
pub struct FontAtlas {
    /// RGBA texture data (SDF in R channel, others set to same value)
    pub texture_data: Vec<u8>,
    /// Atlas dimensions
    pub width: u32,
    pub height: u32,
    /// Glyph metrics indexed by character
    pub glyphs: HashMap<char, GlyphMetrics>,
    /// Line height for text layout (normalized)
    pub line_height: f32,
}

impl FontAtlas {
    /// Generate a new font atlas from the embedded font
    pub fn generate() -> Result<Self, FontAtlasError> {
        Self::generate_from_bytes(EMBEDDED_FONT)
    }

    /// Generate atlas from font bytes
    pub fn generate_from_bytes(font_bytes: &[u8]) -> Result<Self, FontAtlasError> {
        use fontdue::{Font, FontSettings};

        // Parse font
        let font = Font::from_bytes(font_bytes, FontSettings::default())
            .map_err(|e| FontAtlasError::FontParse(e.to_string()))?;

        // Calculate atlas layout
        let chars: Vec<char> = ATLAS_CHARS.chars().collect();
        let glyphs_per_row = (ATLAS_SIZE / (GLYPH_SIZE as u32 + SDF_PADDING * 2)) as usize;
        let rows_needed = (chars.len() + glyphs_per_row - 1) / glyphs_per_row;

        let atlas_width = ATLAS_SIZE;
        let atlas_height = ((rows_needed as u32) * (GLYPH_SIZE as u32 + SDF_PADDING * 2))
            .next_power_of_two()
            .max(256);

        // Create atlas buffer (RGBA)
        let mut texture_data = vec![0u8; (atlas_width * atlas_height * 4) as usize];
        let mut glyphs = HashMap::new();

        let cell_size = GLYPH_SIZE as u32 + SDF_PADDING * 2;

        for (i, &ch) in chars.iter().enumerate() {
            let col = i % glyphs_per_row;
            let row = i / glyphs_per_row;

            let cell_x = col as u32 * cell_size;
            let cell_y = row as u32 * cell_size;

            // Rasterize glyph
            let (metrics, bitmap) = font.rasterize(ch, GLYPH_SIZE);

            if metrics.width == 0 || metrics.height == 0 {
                // Space or empty glyph
                glyphs.insert(ch, GlyphMetrics {
                    uv: [0.0, 0.0, 0.0, 0.0],
                    advance: metrics.advance_width / GLYPH_SIZE,
                    bearing_x: 0.0,
                    bearing_y: 0.0,
                    width: 0.0,
                    height: 0.0,
                });
                continue;
            }

            // Generate SDF from bitmap
            let sdf = generate_sdf(&bitmap, metrics.width, metrics.height, SDF_PADDING as usize);
            let sdf_width = metrics.width + SDF_PADDING as usize * 2;
            let sdf_height = metrics.height + SDF_PADDING as usize * 2;

            // Copy SDF to atlas
            for sy in 0..sdf_height {
                for sx in 0..sdf_width {
                    let atlas_x = cell_x as usize + sx;
                    let atlas_y = cell_y as usize + sy;

                    if atlas_x < atlas_width as usize && atlas_y < atlas_height as usize {
                        let sdf_val = sdf[sy * sdf_width + sx];
                        let pixel_idx = (atlas_y * atlas_width as usize + atlas_x) * 4;

                        // Store SDF in all channels (grayscale)
                        texture_data[pixel_idx] = sdf_val;
                        texture_data[pixel_idx + 1] = sdf_val;
                        texture_data[pixel_idx + 2] = sdf_val;
                        texture_data[pixel_idx + 3] = 255;
                    }
                }
            }

            // Calculate UV coordinates
            let u_min = cell_x as f32 / atlas_width as f32;
            let v_min = cell_y as f32 / atlas_height as f32;
            let u_max = (cell_x + sdf_width as u32) as f32 / atlas_width as f32;
            let v_max = (cell_y + sdf_height as u32) as f32 / atlas_height as f32;

            glyphs.insert(ch, GlyphMetrics {
                uv: [u_min, v_min, u_max, v_max],
                advance: metrics.advance_width / GLYPH_SIZE,
                bearing_x: (metrics.xmin as f32 - SDF_PADDING as f32) / GLYPH_SIZE,
                bearing_y: (metrics.ymin as f32 - SDF_PADDING as f32) / GLYPH_SIZE,
                width: sdf_width as f32 / GLYPH_SIZE,
                height: sdf_height as f32 / GLYPH_SIZE,
            });
        }

        // Get line height from font metrics
        let line_height = font.horizontal_line_metrics(GLYPH_SIZE)
            .map(|m| m.new_line_size / GLYPH_SIZE)
            .unwrap_or(1.2);

        Ok(Self {
            texture_data,
            width: atlas_width,
            height: atlas_height,
            glyphs,
            line_height,
        })
    }

    /// Get glyph metrics for a character (returns space metrics for unknown chars)
    pub fn get_glyph(&self, ch: char) -> GlyphMetrics {
        // Try uppercase first for menu labels
        self.glyphs
            .get(&ch.to_ascii_uppercase())
            .or_else(|| self.glyphs.get(&ch))
            .or_else(|| self.glyphs.get(&' '))
            .copied()
            .unwrap_or(GlyphMetrics {
                uv: [0.0, 0.0, 0.0, 0.0],
                advance: 0.5,
                bearing_x: 0.0,
                bearing_y: 0.0,
                width: 0.0,
                height: 0.0,
            })
    }

    /// Generate glyph data buffer for shader uniform
    /// Returns a packed array of glyph metrics for GPU upload
    pub fn generate_glyph_data(&self) -> Vec<f32> {
        // Pack glyph data for characters A-Z, 0-9, space, and punctuation
        // Each glyph: 8 floats (uv[4], advance, bearing_x, bearing_y, width)
        let mut data = Vec::new();

        for ch in ATLAS_CHARS.chars() {
            let glyph = self.get_glyph(ch);
            data.extend_from_slice(&glyph.uv);
            data.push(glyph.advance);
            data.push(glyph.bearing_x);
            data.push(glyph.bearing_y);
            data.push(glyph.width);
        }

        data
    }
}

/// Generate SDF from a grayscale bitmap
fn generate_sdf(bitmap: &[u8], width: usize, height: usize, padding: usize) -> Vec<u8> {
    let sdf_width = width + padding * 2;
    let sdf_height = height + padding * 2;
    let mut sdf = vec![0u8; sdf_width * sdf_height];

    let max_dist = padding as f32;

    for sy in 0..sdf_height {
        for sx in 0..sdf_width {
            // Find distance to nearest edge
            let mut min_dist = max_dist;

            // Check if this pixel is inside the glyph
            let bx = sx as i32 - padding as i32;
            let by = sy as i32 - padding as i32;
            let inside = if bx >= 0 && bx < width as i32 && by >= 0 && by < height as i32 {
                bitmap[by as usize * width + bx as usize] > 127
            } else {
                false
            };

            // Search for nearest edge pixel
            let search_range = padding as i32;
            for dy in -search_range..=search_range {
                for dx in -search_range..=search_range {
                    let check_bx = bx + dx;
                    let check_by = by + dy;

                    let check_inside = if check_bx >= 0 && check_bx < width as i32
                        && check_by >= 0 && check_by < height as i32
                    {
                        bitmap[check_by as usize * width + check_bx as usize] > 127
                    } else {
                        false
                    };

                    // Found an edge transition
                    if check_inside != inside {
                        let dist = ((dx * dx + dy * dy) as f32).sqrt();
                        min_dist = min_dist.min(dist);
                    }
                }
            }

            // Convert to 0-255 range (128 = edge, >128 = inside, <128 = outside)
            let normalized = if inside {
                0.5 + (min_dist / max_dist) * 0.5
            } else {
                0.5 - (min_dist / max_dist) * 0.5
            };

            sdf[sy * sdf_width + sx] = (normalized.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }

    sdf
}

#[derive(Debug)]
pub enum FontAtlasError {
    FontParse(String),
    AtlasGeneration(String),
}

impl std::fmt::Display for FontAtlasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontAtlasError::FontParse(e) => write!(f, "Failed to parse font: {}", e),
            FontAtlasError::AtlasGeneration(e) => write!(f, "Failed to generate atlas: {}", e),
        }
    }
}

impl std::error::Error for FontAtlasError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atlas_generation() {
        // This test will fail until we have the font file
        // For now, just test the SDF generation
        let bitmap = vec![
            0, 0, 0, 0, 0,
            0, 255, 255, 255, 0,
            0, 255, 255, 255, 0,
            0, 255, 255, 255, 0,
            0, 0, 0, 0, 0,
        ];
        let sdf = generate_sdf(&bitmap, 5, 5, 2);
        assert_eq!(sdf.len(), 9 * 9);
        // Center should be high (inside)
        assert!(sdf[4 * 9 + 4] > 127);
        // Corners should be low (outside)
        assert!(sdf[0] < 127);
    }
}
