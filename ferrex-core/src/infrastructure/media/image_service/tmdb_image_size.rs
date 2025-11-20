use crate::domain::media::image::MediaImageKind;

/// TMDB image size variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmdbImageSize {
    // Poster sizes
    PosterW92,
    PosterW154,
    PosterW185,
    PosterW300,
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
            TmdbImageSize::PosterW300 => "w300",
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

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "w92" => Some(TmdbImageSize::PosterW92),
            "w154" => Some(TmdbImageSize::PosterW154),
            "w185" => Some(TmdbImageSize::PosterW185),
            "w300" => Some(TmdbImageSize::PosterW300),
            "w342" => Some(TmdbImageSize::PosterW342),
            "w500" => Some(TmdbImageSize::PosterW500),
            "w780" => Some(TmdbImageSize::PosterW780),
            "w1280" => Some(TmdbImageSize::BackdropW1280),
            "h632" => Some(TmdbImageSize::ProfileH632),
            "w45" => Some(TmdbImageSize::ProfileW45),
            "original" => Some(TmdbImageSize::Original),
            _ => None,
        }
    }

    /// Get recommended sizes for native client usage
    pub fn recommended_for_kind(kind: &MediaImageKind) -> Vec<Self> {
        match kind {
            // Prioritize ~300w poster (w342) first for fast above-the-fold loads,
            // then a larger fallback for high-DPI/detail, followed by a small thumb.
            MediaImageKind::Poster => vec![
                TmdbImageSize::PosterW300,
                TmdbImageSize::PosterW500,
                TmdbImageSize::PosterW185,
            ],
            // Backdrops: prefer only original for now to avoid artifacts
            MediaImageKind::Backdrop => vec![TmdbImageSize::Original],
            MediaImageKind::Logo => vec![TmdbImageSize::Original], // SVG logos should use original
            MediaImageKind::Thumbnail => {
                vec![TmdbImageSize::StillW300, TmdbImageSize::StillW500]
            }
            MediaImageKind::Cast => vec![TmdbImageSize::ProfileW185],
            MediaImageKind::Other(_) => vec![TmdbImageSize::Original],
        }
    }
}
