use ferrex_core::player_prelude::ImageRequest;
use ferrex_model::ImageSize;
use iced::widget::image::Handle;

/// Convert encoded image bytes into an `iced::widget::image::Handle` that is cheap
/// to (re)upload into the atlas.
///
/// Why:
/// - `Handle::from_bytes` keeps compressed bytes in RAM and defers decoding until
///   Iced needs to rasterize for the atlas.
/// - When the atlas trims/evicts (common during fast scrolling / big grids),
///   Iced may have to decode again on the next upload, causing stutter.
/// - `Handle::from_rgba` stores already-decoded RGBA pixels, so future atlas
///   uploads become a straight memcpy (no repeated decode work).
///
/// The returned `usize` is an *estimated resident byte size* for RAM budgeting.
pub fn handle_from_encoded_bytes(
    request: &ImageRequest,
    bytes: Vec<u8>,
) -> (Handle, u64) {
    let encoded_len = bytes.len();

    // Keep backdrops as encoded bytes by default: they can be large enough that
    // decoding eagerly can blow up RAM (and they are far less churn-heavy than
    // poster grids). Posters/thumbnails/profiles are small and heavily reused.
    let should_decode = match request.size {
        ImageSize::Poster(_)
        | ImageSize::Thumbnail(_)
        | ImageSize::Profile(_) => true,
        ImageSize::Backdrop(_) => false,
    };

    if !should_decode {
        return (Handle::from_bytes(bytes), encoded_len as u64);
    }

    match image::load_from_memory(&bytes) {
        Ok(decoded) => {
            let rgba = decoded.to_rgba8();
            let (width, height) = rgba.dimensions();
            let raw = rgba.into_raw();
            let estimated_bytes = raw.len();

            let handle = Handle::from_rgba(width, height, raw);
            (handle, estimated_bytes as u64)
        }
        Err(e) => {
            // Fall back to lazy decode if we cannot decode here (e.g. uncommon
            // formats). This preserves existing behavior instead of failing the
            // image load path.
            log::debug!(
                "image decode failed; using encoded bytes handle (iid={}, size={:?}, bytes={}, err={})",
                request.iid,
                request.size,
                encoded_len,
                e
            );
            (Handle::from_bytes(bytes), encoded_len as u64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::handle_from_encoded_bytes;
    use ferrex_core::player_prelude::ImageRequest;
    use ferrex_model::image::{ImageSize, PosterSize};
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;
    use uuid::Uuid;

    #[test]
    fn posters_decode_to_rgba_for_ram_residency() {
        // 2x1 RGBA image: two pixels (red, green)
        let width = 2u32;
        let height = 1u32;
        let rgba: Vec<u8> = vec![
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
        ];

        let mut encoded = Vec::new();
        let encoder = PngEncoder::new(&mut encoded);
        encoder
            .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
            .expect("encode png");

        let request =
            ImageRequest::new(Uuid::nil(), ImageSize::Poster(PosterSize::W185));

        let (_handle, estimated_bytes) =
            handle_from_encoded_bytes(&request, encoded);

        assert_eq!(estimated_bytes, (width * height * 4) as u64);
    }
}
