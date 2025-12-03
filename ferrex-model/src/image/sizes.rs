use std::fmt::Formatter;

use std::fmt::Display;

/// Image size variants
#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum ImageSize {
    Thumbnail(EpisodeSize), // Small size for grids
    Poster(PosterSize),     // Standard poster size
    Backdrop(BackdropSize), // Wide backdrop/banner
    Profile(ProfileSize),   // Person profile image (2:3 aspect ratio)
}

impl Display for ImageSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageSize::Thumbnail(s) => {
                write!(f, "Thumbnail (size: {:#?})", s)
            }
            ImageSize::Poster(s) => write!(f, "Poster (size: {:#?})", s),
            ImageSize::Backdrop(s) => {
                write!(f, "Backdrop (size: {:#?})", s)
            }
            ImageSize::Profile(s) => write!(f, "Profile (size: {:#?})", s),
        }
    }
}

impl ImageSize {
    // Default size constructors for convenience
    /// Default thumbnail size (512px episode still)
    pub const fn thumbnail() -> Self {
        Self::Thumbnail(EpisodeSize::W512)
    }

    /// Default poster size (300px)
    pub const fn poster() -> Self {
        Self::Poster(PosterSize::W300)
    }

    /// Large poster size (950px) - replacement for old "Full" variant
    pub const fn poster_large() -> Self {
        Self::Poster(PosterSize::W950)
    }

    /// Default backdrop size (1920px)
    pub const fn backdrop() -> Self {
        Self::Backdrop(BackdropSize::W1920)
    }

    /// Default profile size (180px)
    pub const fn profile() -> Self {
        Self::Profile(ProfileSize::W180)
    }

    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match self {
            ImageSize::Thumbnail(s) => s.dimensions(),
            ImageSize::Poster(s) => s.dimensions(),
            ImageSize::Backdrop(s) => s.dimensions(),
            ImageSize::Profile(s) => s.dimensions(),
        }
    }

    pub fn suffix(&self) -> &str {
        match self {
            ImageSize::Thumbnail(_) => "_thumb",
            ImageSize::Poster(_) => "_poster",
            ImageSize::Backdrop(_) => "_backdrop",
            ImageSize::Profile(_) => "_profile",
        }
    }

    /// Convert to URL-safe string representation (e.g., "poster_w300", "thumb_w512")
    pub fn as_str(&self) -> String {
        match self {
            ImageSize::Thumbnail(s) => format!("thumb_{}", s.as_str()),
            ImageSize::Poster(s) => format!("poster_{}", s.as_str()),
            ImageSize::Backdrop(s) => format!("backdrop_{}", s.as_str()),
            ImageSize::Profile(s) => format!("profile_{}", s.as_str()),
        }
    }

    /// Get the width hint for this size
    pub fn width(&self) -> Option<u16> {
        match self {
            ImageSize::Thumbnail(s) => s.width(),
            ImageSize::Poster(s) => s.width(),
            ImageSize::Backdrop(s) => s.width(),
            ImageSize::Profile(s) => s.width(),
        }
    }
}

/// Episode still/thumbnail sizes (16:9 aspect ratio)
#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum EpisodeSize {
    W256,
    #[default]
    W512,
    W768,
    Original(Option<u32>),
}

impl EpisodeSize {
    pub const ALL: [EpisodeSize; 3] = [Self::W256, Self::W512, Self::W768];

    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            EpisodeSize::W256 => Some((256, 144)),
            EpisodeSize::W512 => Some((512, 288)),
            EpisodeSize::W768 => Some((768, 432)),
            EpisodeSize::Original(w) => w.map(|w| (w, (w / 16) * 9)),
        }
    }

    pub const fn width(&self) -> Option<u16> {
        match self {
            Self::W256 => Some(256),
            Self::W512 => Some(512),
            Self::W768 => Some(768),
            Self::Original(_) => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::W256 => "w256",
            Self::W512 => "w512",
            Self::W768 => "w768",
            Self::Original(_) => "original",
        }
    }
}

impl Display for EpisodeSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::W256 => write!(f, "256px"),
            Self::W512 => write!(f, "512px"),
            Self::W768 => write!(f, "768px"),
            Self::Original(_) => write!(f, "Original"),
        }
    }
}

/// Poster image sizes (2:3 aspect ratio)
///
/// These are the target output sizes the player can request. The server will
/// resize TMDB source images to these dimensions during storage/caching.
#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum PosterSize {
    /// 92px width - tiny thumbnail
    W92,
    /// 180px width - small poster
    W180,
    /// 300px width - medium poster (default)
    #[default]
    W300,
    /// 600px width - large poster
    W600,
    /// 950px width - high quality poster
    W950,
    /// 1320px width - very high quality poster
    W1320,
    /// 2000px width - maximum quality poster
    W2000,
    /// Original resolution (optional known width)
    Original(Option<u32>),
}

impl PosterSize {
    /// All available poster sizes for UI enumeration (excluding Original)
    pub const ALL: [PosterSize; 7] = [
        Self::W92,
        Self::W180,
        Self::W300,
        Self::W600,
        Self::W950,
        Self::W1320,
        Self::W2000,
    ];

    /// Get the pixel dimensions for this size (width, height at 2:3 ratio)
    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            PosterSize::W92 => Some((92, 138)),
            PosterSize::W180 => Some((180, 270)),
            PosterSize::W300 => Some((300, 450)),
            PosterSize::W600 => Some((600, 900)),
            PosterSize::W950 => Some((950, 1425)),
            PosterSize::W1320 => Some((1320, 1980)),
            PosterSize::W2000 => Some((2000, 3000)),
            PosterSize::Original(w) => w.map(|w| (w, (w / 2) * 3)),
        }
    }

    /// Get the width for this size
    pub const fn width(&self) -> Option<u16> {
        match self {
            Self::W92 => Some(92),
            Self::W180 => Some(180),
            Self::W300 => Some(300),
            Self::W600 => Some(600),
            Self::W950 => Some(950),
            Self::W1320 => Some(1320),
            Self::W2000 => Some(2000),
            Self::Original(_) => None,
        }
    }

    /// Convert to URL-safe string representation
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::W92 => "w92",
            Self::W180 => "w180",
            Self::W300 => "w300",
            Self::W600 => "w600",
            Self::W950 => "w950",
            Self::W1320 => "w1320",
            Self::W2000 => "w2000",
            Self::Original(_) => "original",
        }
    }

    /// Parse from URL string representation
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "w92" => Some(Self::W92),
            "w180" => Some(Self::W180),
            "w300" => Some(Self::W300),
            "w600" => Some(Self::W600),
            "w950" => Some(Self::W950),
            "w1320" => Some(Self::W1320),
            "w2000" => Some(Self::W2000),
            "original" => Some(Self::Original(None)),
            _ => None,
        }
    }
}

impl Display for PosterSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::W92 => write!(f, "92px"),
            Self::W180 => write!(f, "180px"),
            Self::W300 => write!(f, "300px"),
            Self::W600 => write!(f, "600px"),
            Self::W950 => write!(f, "950px"),
            Self::W1320 => write!(f, "1320px"),
            Self::W2000 => write!(f, "2000px"),
            Self::Original(_) => write!(f, "Original"),
        }
    }
}

/// 16:9 Widescreen Media Backdrop Sizes
#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum BackdropSize {
    W1280,
    W1920,
    #[default]
    W3840,
    Original(Option<u32>),
}

impl BackdropSize {
    pub const ALL: [BackdropSize; 3] = [Self::W1280, Self::W1920, Self::W3840];

    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            BackdropSize::W1280 => Some((1280, 720)),
            BackdropSize::W1920 => Some((1920, 1080)),
            BackdropSize::W3840 => Some((3840, 2160)),
            BackdropSize::Original(w) => w.map(|w| (w, (w / 16) * 9)),
        }
    }

    pub const fn width(&self) -> Option<u16> {
        match self {
            Self::W1280 => Some(1280),
            Self::W1920 => Some(1920),
            Self::W3840 => Some(3840),
            Self::Original(_) => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::W1280 => "w1280",
            Self::W1920 => "w1920",
            Self::W3840 => "w3840",
            Self::Original(_) => "original",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "w1280" => Some(Self::W1280),
            "w1920" => Some(Self::W1920),
            "w3840" => Some(Self::W3840),
            "original" => Some(Self::Original(None)),
            _ => None,
        }
    }
}

impl Display for BackdropSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::W1280 => write!(f, "1280px"),
            Self::W1920 => write!(f, "1920px"),
            Self::W3840 => write!(f, "3840px"),
            Self::Original(_) => write!(f, "Original"),
        }
    }
}

/// Profile/cast image sizes (2:3 aspect ratio)
#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum ProfileSize {
    W92,
    #[default]
    W180,
    Original(Option<u32>),
}

impl ProfileSize {
    pub const ALL: [ProfileSize; 2] = [Self::W92, Self::W180];

    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            ProfileSize::W92 => Some((92, 138)),
            ProfileSize::W180 => Some((180, 270)),
            ProfileSize::Original(w) => w.map(|w| (w, (w / 2) * 3)),
        }
    }

    pub const fn width(&self) -> Option<u16> {
        match self {
            Self::W92 => Some(92),
            Self::W180 => Some(180),
            Self::Original(_) => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::W92 => "w92",
            Self::W180 => "w180",
            Self::Original(_) => "original",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "w92" => Some(Self::W92),
            "w180" => Some(Self::W180),
            "original" => Some(Self::Original(None)),
            _ => None,
        }
    }
}

impl Display for ProfileSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::W92 => write!(f, "92px"),
            Self::W180 => write!(f, "180px"),
            Self::Original(_) => write!(f, "Original"),
        }
    }
}
