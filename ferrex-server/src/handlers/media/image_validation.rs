//! Image validation utilities for atomic read-then-serve approach.
//!
//! This module provides magic byte validation to detect image format
//! and catch corrupted or mismatched files before serving.

use tracing::warn;

/// Result of magic byte validation
#[derive(Debug)]
pub enum InvalidReason {
    /// File is too small to contain valid image header
    TooSmall,
    /// File does not match any recognized image format
    UnrecognizedFormat,
}

/// Validate image bytes by checking magic bytes.
/// Returns the detected content type if valid.
pub fn validate_magic_bytes(data: &[u8]) -> Result<&'static str, InvalidReason> {
    if data.len() < 4 {
        return Err(InvalidReason::TooSmall);
    }

    // JPEG: FF D8 FF
    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return Ok("image/jpeg");
    }

    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if data.len() >= 8
        && data[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    {
        return Ok("image/png");
    }

    // WebP: RIFF....WEBP
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Ok("image/webp");
    }

    // AVIF: ftyp box with avif/avis brand
    if data.len() >= 12
        && &data[4..8] == b"ftyp"
        && (&data[8..12] == b"avif" || &data[8..12] == b"avis")
    {
        return Ok("image/avif");
    }

    // GIF: GIF87a or GIF89a
    if data.len() >= 6 && &data[0..3] == b"GIF" {
        return Ok("image/gif");
    }

    // BMP: BM
    if data.len() >= 2 && &data[0..2] == b"BM" {
        return Ok("image/bmp");
    }

    warn!(
        "Unrecognized image format, first 8 bytes: {:02X?}",
        &data[..8.min(data.len())]
    );
    Err(InvalidReason::UnrecognizedFormat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_jpeg_magic() {
        let jpeg_header = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert!(matches!(
            validate_magic_bytes(&jpeg_header),
            Ok("image/jpeg")
        ));
    }

    #[test]
    fn test_validate_png_magic() {
        let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(matches!(
            validate_magic_bytes(&png_header),
            Ok("image/png")
        ));
    }

    #[test]
    fn test_validate_webp_magic() {
        let mut webp = [0u8; 12];
        webp[0..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");
        assert!(matches!(
            validate_magic_bytes(&webp),
            Ok("image/webp")
        ));
    }

    #[test]
    fn test_validate_avif_magic() {
        let mut avif = [0u8; 12];
        avif[4..8].copy_from_slice(b"ftyp");
        avif[8..12].copy_from_slice(b"avif");
        assert!(matches!(
            validate_magic_bytes(&avif),
            Ok("image/avif")
        ));
    }

    #[test]
    fn test_validate_gif_magic() {
        let gif_header = b"GIF89a";
        assert!(matches!(
            validate_magic_bytes(gif_header),
            Ok("image/gif")
        ));
    }

    #[test]
    fn test_validate_too_small() {
        assert!(matches!(
            validate_magic_bytes(&[0xFF, 0xD8]),
            Err(InvalidReason::TooSmall)
        ));
    }

    #[test]
    fn test_validate_unrecognized() {
        let unknown = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
        assert!(matches!(
            validate_magic_bytes(&unknown),
            Err(InvalidReason::UnrecognizedFormat)
        ));
    }
}
