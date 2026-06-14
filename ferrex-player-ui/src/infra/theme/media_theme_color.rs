use iced::Color;
use uuid::Uuid;

/// Deterministic, per-media fallback theme color.
///
/// This is used when the backend hasn't provided a poster-derived theme color yet
/// (e.g. during scanning before images are downloaded).
///
/// Properties:
/// - Stable across runs for a given `media_id`
/// - Low-cost (no image decode / sampling)
/// - Curated palette to avoid unreadable extremes
#[must_use]
pub fn fallback_theme_color_for(media_id: Uuid) -> Color {
    // Tailwind-ish "700" colors, biased darker so overlays remain readable.
    // Stored as 0xRRGGBB.
    const PALETTE: [u32; 12] = [
        0x1D4ED8, // blue
        0x4338CA, // indigo
        0x6D28D9, // purple
        0xA21CAF, // fuchsia
        0xBE123C, // rose
        0xC2410C, // orange
        0xB45309, // amber
        0x15803D, // green
        0x0F766E, // teal
        0x0E7490, // cyan
        0x155E75, // sky-ish
        0x334155, // slate
    ];

    let seed = media_id.as_u128();
    let folded = (seed as u64) ^ ((seed >> 64) as u64);

    // SplitMix64 finalizer
    let mut x = folded.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;

    let rgb = PALETTE[(x as usize) % PALETTE.len()];
    Color::from_rgb8(
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
    )
}
