use std::fmt::Formatter;

use std::fmt::Display;

/// Image size variants
#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub enum ImageSize {
    Poster(PosterSize),     // Standard poster size
    Backdrop(BackdropSize), // Wide backdrop/banner
    Thumbnail(EpisodeSize), // Small size for grids
    Profile(ProfileSize),   // Person profile image (2:3 aspect ratio)
}

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "image_variant", rename_all = "lowercase")
)]
pub enum ImageVariant {
    Poster,
    Backdrop,
    Thumbnail,
    Profile,
}

impl Display for ImageVariant {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageVariant::Poster => write!(f, "poster"),
            ImageVariant::Backdrop => write!(f, "backdrop"),
            ImageVariant::Thumbnail => write!(f, "thumbnail"),
            ImageVariant::Profile => write!(f, "profile"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "size_variant", rename_all = "lowercase")
)]
pub enum SqlxImageSizeVariant {
    Original,
    Resized,
    Tmdb,
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

    /// Default poster size (342px)
    pub const fn poster() -> Self {
        Self::Poster(PosterSize::W342)
    }

    /// Large poster size (780px)
    pub const fn poster_large() -> Self {
        Self::Poster(PosterSize::W780)
    }

    /// Default backdrop size (original)
    pub const fn backdrop() -> Self {
        Self::Backdrop(BackdropSize::Original(None))
    }

    /// Default profile size (180px)
    pub const fn profile() -> Self {
        Self::Profile(ProfileSize::W185)
    }

    /// Rounds up to the nearest tmdb api valid size variant, always returns with a specific width
    pub fn to_nearest_tmdb_size(self, original_width: ImageSize) -> Self {
        match self {
            ImageSize::Poster(poster_size) => match poster_size {
                PosterSize::CustomResized(w) => {
                    for size in PosterSize::ALL.iter() {
                        if let PosterSize::Original(None) = size {
                            return original_width;
                        }
                        if w <= size.width_unchecked() {
                            return ImageSize::Poster(*size);
                        }
                    }
                    ImageSize::Poster(PosterSize::default())
                }
                PosterSize::Original(Some(_)) => self,
                PosterSize::Original(None) => {
                    ImageSize::Poster(PosterSize::default())
                }
                _ => self,
            },
            ImageSize::Backdrop(backdrop_size) => match backdrop_size {
                BackdropSize::CustomResized(w) => {
                    for size in BackdropSize::ALL.iter() {
                        if let BackdropSize::Original(None) = size {
                            return original_width;
                        }
                        if w <= size.width_unchecked() {
                            return ImageSize::Backdrop(*size);
                        }
                    }
                    ImageSize::Backdrop(BackdropSize::default())
                }
                BackdropSize::Original(Some(_)) => self,
                BackdropSize::Original(None) => {
                    ImageSize::Backdrop(BackdropSize::default())
                }
                _ => self,
            },
            ImageSize::Thumbnail(episode_size) => match episode_size {
                EpisodeSize::CustomResized(w) => {
                    for size in EpisodeSize::ALL.iter() {
                        if let EpisodeSize::Original(None) = size {
                            return original_width;
                        }
                        if w <= size.width_unchecked() {
                            return ImageSize::Thumbnail(*size);
                        }
                    }
                    ImageSize::Thumbnail(EpisodeSize::default())
                }
                EpisodeSize::Original(Some(_)) => self,
                EpisodeSize::Original(None) => {
                    ImageSize::Thumbnail(EpisodeSize::default())
                }
                _ => self,
            },
            ImageSize::Profile(profile_size) => match profile_size {
                ProfileSize::CustomResized(w) => {
                    for size in ProfileSize::ALL.iter() {
                        if let ProfileSize::Original(None) = size {
                            return original_width;
                        }
                        if w <= size.width_unchecked() {
                            return ImageSize::Profile(*size);
                        }
                    }
                    ImageSize::Profile(ProfileSize::default())
                }
                ProfileSize::Original(Some(_)) => self,
                ProfileSize::Original(None) => {
                    ImageSize::Profile(ProfileSize::default())
                }
                _ => self,
            },
        }
    }

    pub const fn original(width: u32, image_variant: ImageVariant) -> Self {
        match image_variant {
            ImageVariant::Poster => {
                ImageSize::Poster(PosterSize::Original(Some(width)))
            }
            ImageVariant::Backdrop => {
                ImageSize::Backdrop(BackdropSize::Original(Some(width)))
            }
            ImageVariant::Thumbnail => {
                ImageSize::Thumbnail(EpisodeSize::Original(Some(width)))
            }
            ImageVariant::Profile => {
                ImageSize::Profile(ProfileSize::Original(Some(width)))
            }
        }
    }

    pub const fn original_unknown(image_variant: ImageVariant) -> Self {
        match image_variant {
            ImageVariant::Poster => {
                ImageSize::Poster(PosterSize::Original(None))
            }
            ImageVariant::Backdrop => {
                ImageSize::Backdrop(BackdropSize::Original(None))
            }
            ImageVariant::Thumbnail => {
                ImageSize::Thumbnail(EpisodeSize::Original(None))
            }
            ImageVariant::Profile => {
                ImageSize::Profile(ProfileSize::Original(None))
            }
        }
    }

    pub const fn is_original(self) -> bool {
        match self {
            ImageSize::Poster(poster_size) => {
                matches!(poster_size, PosterSize::Original(_))
            }
            ImageSize::Backdrop(backdrop_size) => {
                matches!(backdrop_size, BackdropSize::Original(_))
            }
            ImageSize::Thumbnail(episode_size) => {
                matches!(episode_size, EpisodeSize::Original(_))
            }
            ImageSize::Profile(profile_size) => {
                matches!(profile_size, ProfileSize::Original(_))
            }
        }
    }

    pub const fn custom(width: u32, image_variant: ImageVariant) -> Self {
        match image_variant {
            ImageVariant::Poster => {
                ImageSize::Poster(PosterSize::CustomResized(width))
            }
            ImageVariant::Backdrop => {
                ImageSize::Backdrop(BackdropSize::CustomResized(width))
            }
            ImageVariant::Thumbnail => {
                ImageSize::Thumbnail(EpisodeSize::CustomResized(width))
            }
            ImageVariant::Profile => {
                ImageSize::Profile(ProfileSize::CustomResized(width))
            }
        }
    }

    pub const fn is_resized(self) -> bool {
        match self {
            ImageSize::Poster(poster_size) => {
                matches!(poster_size, PosterSize::CustomResized(_))
            }
            ImageSize::Backdrop(backdrop_size) => {
                matches!(backdrop_size, BackdropSize::CustomResized(_))
            }
            ImageSize::Thumbnail(episode_size) => {
                matches!(episode_size, EpisodeSize::CustomResized(_))
            }
            ImageSize::Profile(profile_size) => {
                matches!(profile_size, ProfileSize::CustomResized(_))
            }
        }
    }

    pub fn from_size_and_variant(width: u32, variant: ImageVariant) -> Self {
        match variant {
            ImageVariant::Poster => {
                ImageSize::Poster(PosterSize::from_width(width))
            }
            ImageVariant::Backdrop => {
                ImageSize::Backdrop(BackdropSize::from_width(width))
            }
            ImageVariant::Thumbnail => {
                ImageSize::Thumbnail(EpisodeSize::from_width(width))
            }
            ImageVariant::Profile => {
                ImageSize::Profile(ProfileSize::from_width(width))
            }
        }
    }

    pub fn sqlx_image_size_variant(self) -> SqlxImageSizeVariant {
        match self {
            ImageSize::Poster(s) => s.sqlx_image_size_variant(),
            ImageSize::Backdrop(s) => s.sqlx_image_size_variant(),
            ImageSize::Thumbnail(s) => s.sqlx_image_size_variant(),
            ImageSize::Profile(s) => s.sqlx_image_size_variant(),
        }
    }

    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match self {
            ImageSize::Poster(s) => s.dimensions(),
            ImageSize::Backdrop(s) => s.dimensions(),
            ImageSize::Thumbnail(s) => s.dimensions(),
            ImageSize::Profile(s) => s.dimensions(),
        }
    }

    /// Panics if given an Original variant with no width included
    pub fn dimensions_unchecked(&self) -> (u32, u32) {
        self.dimensions().unwrap_or_else(|| {
            panic!(
                "dimensions_unchecked called for ImageSize with no dimensions available: {self:?}"
            )
        })
    }

    pub fn has_width(&self) -> bool {
        !match self {
            ImageSize::Poster(s) => {
                matches!(s, PosterSize::Original(None))
            }
            ImageSize::Backdrop(s) => {
                matches!(s, BackdropSize::Original(None))
            }
            ImageSize::Thumbnail(s) => {
                matches!(s, EpisodeSize::Original(None))
            }
            ImageSize::Profile(s) => {
                matches!(s, ProfileSize::Original(None))
            }
        }
    }

    pub fn image_variant(&self) -> ImageVariant {
        match self {
            ImageSize::Poster(_) => ImageVariant::Poster,
            ImageSize::Backdrop(_) => ImageVariant::Backdrop,
            ImageSize::Thumbnail(_) => ImageVariant::Thumbnail,
            ImageSize::Profile(_) => ImageVariant::Profile,
        }
    }

    /// Convert to URL string size
    pub fn to_tmdb_param(&self) -> &'static str {
        match self {
            ImageSize::Thumbnail(s) => s.to_tmdb_param(),
            ImageSize::Poster(s) => s.to_tmdb_param(),
            ImageSize::Backdrop(s) => s.to_tmdb_param(),
            ImageSize::Profile(s) => s.to_tmdb_param(),
        }
    }

    /// Get the width hint for this size
    pub fn width(&self) -> Option<u32> {
        match self {
            ImageSize::Thumbnail(s) => s.width(),
            ImageSize::Poster(s) => s.width(),
            ImageSize::Backdrop(s) => s.width(),
            ImageSize::Profile(s) => s.width(),
        }
    }

    /// Get the width for this size
    /// Panics if given an Original variant with no width included
    pub const fn width_unchecked(&self) -> u32 {
        match self {
            ImageSize::Thumbnail(s) => s.width_unchecked(),
            ImageSize::Poster(s) => s.width_unchecked(),
            ImageSize::Backdrop(s) => s.width_unchecked(),
            ImageSize::Profile(s) => s.width_unchecked(),
        }
    }

    pub fn width_name(&self) -> String {
        match self {
            ImageSize::Thumbnail(s) => s.width_name(),
            ImageSize::Poster(s) => s.width_name(),
            ImageSize::Backdrop(s) => s.width_name(),
            ImageSize::Profile(s) => s.width_name(),
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
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub enum EpisodeSize {
    W256,
    #[default]
    W512,
    W768,
    /// Custom resized poster width
    CustomResized(u32),
    Original(Option<u32>),
}

impl EpisodeSize {
    pub const ALL: [EpisodeSize; 3] = [Self::W256, Self::W512, Self::W768];

    pub fn from_width(w: u32) -> Self {
        match w {
            256 => Self::W256,
            512 => Self::W512,
            768 => Self::W768,
            w => Self::Original(Some(w)),
        }
    }

    pub fn height_from_width(w: u32) -> u32 {
        // Round to nearest integer height for a 16:9 aspect ratio.
        // This keeps known TMDB sizes exact (multiples of 16) and behaves well
        // for custom widths.
        (w.saturating_mul(9).saturating_add(8)) / 16
    }

    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            EpisodeSize::W256 => Some((256, 144)),
            EpisodeSize::W512 => Some((512, 288)),
            EpisodeSize::W768 => Some((768, 432)),
            EpisodeSize::CustomResized(w) => {
                Some((w, Self::height_from_width(w)))
            }
            EpisodeSize::Original(w) => {
                w.map(|w| (w, Self::height_from_width(w)))
            }
        }
    }

    pub const fn width(&self) -> Option<u32> {
        match self {
            Self::W256 => Some(256),
            Self::W512 => Some(512),
            Self::W768 => Some(768),
            Self::CustomResized(w) => Some(*w),
            Self::Original(Some(w)) => Some(*w),
            Self::Original(None) => None,
        }
    }
    /// Get the width for this size
    /// Panics if given an Original variant with no width included
    pub const fn width_unchecked(&self) -> u32 {
        match self {
            Self::W256 => 256,
            Self::W512 => 512,
            Self::W768 => 768,
            Self::CustomResized(w) => *w,
            Self::Original(Some(w)) => *w,
            Self::Original(None) => panic!(
                "Width not available for Original variant with no width included"
            ),
        }
    }
    pub fn width_name(&self) -> String {
        match self {
            Self::CustomResized(_) => "custom".to_string(),
            Self::Original(Some(_)) => "original".to_string(),
            Self::Original(None) => "original".to_string(),
            _ => self.to_tmdb_param().to_string(),
        }
    }

    pub fn sqlx_image_size_variant(&self) -> SqlxImageSizeVariant {
        match self {
            Self::CustomResized(_) => SqlxImageSizeVariant::Resized,
            Self::Original(_) => SqlxImageSizeVariant::Original,
            _ => SqlxImageSizeVariant::Tmdb,
        }
    }

    pub const fn to_tmdb_param(&self) -> &'static str {
        match self {
            Self::W256 => "w256",
            Self::W512 => "w512",
            Self::W768 => "w768",
            Self::CustomResized(_) => "invalid",
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
            Self::CustomResized(w) => write!(f, "{}px", w),
            Self::Original(_) => write!(f, "Original"),
        }
    }
}

/// Poster image sizes (2:3 aspect ratio)
///
/// These are the target output sizes the player can request. The server will
/// resize TMDB source images to these dimensions during storage/caching.
#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub enum PosterSize {
    /// 92px width - tiny thumbnail
    W92,
    /// 154px width - small poster
    W154,
    /// 185px width - medium poster (default)
    W185,
    /// 342px width - large poster
    W342,
    /// 500px width - high quality poster
    W500,
    /// 780px width - very high quality poster
    W780,
    /// Custom resized poster width
    CustomResized(u32),
    /// Original resolution (optional known width)
    Original(Option<u32>),
}

impl Default for PosterSize {
    fn default() -> Self {
        PosterSize::Original(None)
    }
}

impl PosterSize {
    /// All available poster sizes for UI enumeration (excluding Original)
    pub const ALL: [PosterSize; 7] = [
        Self::W92,
        Self::W154,
        Self::W185,
        Self::W342,
        Self::W500,
        Self::W780,
        Self::Original(None),
    ];

    pub fn from_width(w: u32) -> Self {
        match w {
            92 => Self::W92,
            154 => Self::W154,
            185 => Self::W185,
            342 => Self::W342,
            500 => Self::W500,
            780 => Self::W780,
            w => Self::CustomResized(w),
        }
    }

    pub fn original(w: u32) -> Self {
        Self::Original(Some(w))
    }

    pub fn height_from_width(w: u32) -> u32 {
        (w / 2) * 3
    }

    /// Get the pixel dimensions for this size (width, height at 2:3 ratio)
    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            PosterSize::W92 => Some((92, 138)),
            PosterSize::W154 => Some((154, 231)),
            PosterSize::W185 => Some((185, 277)),
            PosterSize::W342 => Some((342, 513)),
            PosterSize::W500 => Some((500, 750)),
            PosterSize::W780 => Some((780, 1170)),
            PosterSize::CustomResized(w) => Some((w, (w / 2) * 3)),
            PosterSize::Original(w) => w.map(|w| (w, (w / 2) * 3)),
        }
    }

    /// Get the width for this size
    pub const fn width(&self) -> Option<u32> {
        match self {
            Self::W92 => Some(92),
            Self::W154 => Some(154),
            Self::W185 => Some(185),
            Self::W342 => Some(342),
            Self::W500 => Some(500),
            Self::W780 => Some(780),
            Self::CustomResized(w) => Some(*w),
            Self::Original(Some(w)) => Some(*w),
            Self::Original(None) => None,
        }
    }

    /// Get the width for this size
    /// Panics if given an Original variant with no width included
    pub const fn width_unchecked(&self) -> u32 {
        match self {
            Self::W92 => 92,
            Self::W154 => 154,
            Self::W185 => 185,
            Self::W342 => 342,
            Self::W500 => 500,
            Self::W780 => 780,
            Self::CustomResized(w) => *w,
            Self::Original(Some(w)) => *w,
            Self::Original(None) => panic!(
                "Width not available for Original variant with no width included"
            ),
        }
    }

    pub fn width_name(&self) -> String {
        match self {
            Self::CustomResized(_) => "custom".to_string(),
            Self::Original(Some(_)) => "original".to_string(),
            Self::Original(None) => "original".to_string(),
            _ => self.to_tmdb_param().to_string(),
        }
    }

    pub fn sqlx_image_size_variant(&self) -> SqlxImageSizeVariant {
        match self {
            Self::CustomResized(_) => SqlxImageSizeVariant::Resized,
            Self::Original(_) => SqlxImageSizeVariant::Original,
            _ => SqlxImageSizeVariant::Tmdb,
        }
    }

    /// Convert to URL string size
    pub const fn to_tmdb_param(&self) -> &'static str {
        match self {
            Self::W92 => "w92",
            Self::W154 => "w154",
            Self::W185 => "w185",
            Self::W342 => "w342",
            Self::W500 => "w500",
            Self::W780 => "w780",
            Self::CustomResized(_width) => "invalid",
            Self::Original(_) => "original",
        }
    }
}

impl Display for PosterSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::W92 => write!(f, "92px"),
            Self::W154 => write!(f, "154px"),
            Self::W185 => write!(f, "185px"),
            Self::W342 => write!(f, "342px"),
            Self::W500 => write!(f, "500px"),
            Self::W780 => write!(f, "780px"),
            Self::CustomResized(w) => write!(f, "{}px", w),
            Self::Original(_) => write!(f, "Original"),
        }
    }
}

/// 16:9 Widescreen Media Backdrop Sizes
#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub enum BackdropSize {
    W300,
    W780,
    W1280,
    /// Custom resized width
    CustomResized(u32),
    Original(Option<u32>),
}
impl Default for BackdropSize {
    fn default() -> Self {
        BackdropSize::Original(None)
    }
}

impl BackdropSize {
    pub const ALL: [BackdropSize; 3] = [Self::W300, Self::W780, Self::W1280];

    pub fn from_width(w: u32) -> Self {
        match w {
            300 => Self::W300,
            780 => Self::W780,
            1280 => Self::W1280,
            w => Self::Original(Some(w)),
        }
    }

    pub fn original(w: u32) -> Self {
        Self::Original(Some(w))
    }

    // TODO: Consider pre-cropping backdrop to wider aspect
    // to save memory/bandwidth vs current player-size crop
    pub fn height_from_width(w: u32) -> u32 {
        // Round to nearest integer height for a 16:9 aspect ratio.
        (w.saturating_mul(9).saturating_add(8)) / 16
    }

    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            Self::W300 => Some((300, 169)),
            Self::W780 => Some((780, 439)),
            Self::W1280 => Some((1280, 720)),
            Self::CustomResized(w) => Some((w, Self::height_from_width(w))),
            Self::Original(w) => w.map(|w| (w, Self::height_from_width(w))),
        }
    }

    pub const fn width(&self) -> Option<u32> {
        match self {
            Self::W300 => Some(300),
            Self::W780 => Some(780),
            Self::W1280 => Some(1280),
            Self::CustomResized(w) => Some(*w),
            Self::Original(Some(w)) => Some(*w),
            Self::Original(None) => None,
        }
    }
    /// Get the width for this size
    /// Panics if given an Original variant with no width included
    pub const fn width_unchecked(&self) -> u32 {
        match self {
            Self::W300 => 300,
            Self::W780 => 780,
            Self::W1280 => 1280,
            Self::CustomResized(w) => *w,
            Self::Original(Some(w)) => *w,
            Self::Original(None) => panic!(
                "Width not available for Original variant with no width included"
            ),
        }
    }

    pub fn width_name(&self) -> String {
        match self {
            Self::CustomResized(_) => "custom".to_string(),
            Self::Original(Some(_)) => "original".to_string(),
            Self::Original(None) => "original".to_string(),
            _ => self.to_tmdb_param().to_string(),
        }
    }

    pub fn sqlx_image_size_variant(&self) -> SqlxImageSizeVariant {
        match self {
            Self::CustomResized(_) => SqlxImageSizeVariant::Resized,
            Self::Original(_) => SqlxImageSizeVariant::Original,
            _ => SqlxImageSizeVariant::Tmdb,
        }
    }

    pub const fn to_tmdb_param(&self) -> &'static str {
        match self {
            Self::W300 => "w300",
            Self::W780 => "w780",
            Self::W1280 => "w1280",
            Self::CustomResized(_) => "invalid",
            Self::Original(_) => "original",
        }
    }
}

impl Display for BackdropSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::W300 => write!(f, "300px"),
            Self::W780 => write!(f, "780px"),
            Self::W1280 => write!(f, "1280px"),
            Self::CustomResized(w) => write!(f, "Custom({}px)", w),
            Self::Original(Some(w)) => write!(f, "Original({}px)", w),
            Self::Original(None) => write!(f, "Original"),
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
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub enum ProfileSize {
    W45,
    #[default]
    W185,
    W632,
    /// Custom resized width
    CustomResized(u32),
    Original(Option<u32>),
}

impl ProfileSize {
    pub const ALL: [ProfileSize; 3] = [Self::W45, Self::W185, Self::W632];

    pub fn from_width(w: u32) -> Self {
        match w {
            45 => Self::W45,
            185 => Self::W185,
            632 => Self::W632,
            w => Self::Original(Some(w)),
        }
    }

    // TODO: Consider pre-cropping backdrop to wider aspect
    // to save memory/bandwidth vs current player-size crop
    pub fn height_from_width(w: u32) -> u32 {
        (w / 2) * 3
    }

    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            ProfileSize::W45 => Some((45, 138)),
            ProfileSize::W185 => Some((185, 270)),
            ProfileSize::W632 => Some((632, 450)),
            ProfileSize::CustomResized(w) => Some((w, (w / 2) * 3)),
            ProfileSize::Original(w) => w.map(|w| (w, (w / 2) * 3)),
        }
    }

    pub const fn width(&self) -> Option<u32> {
        match self {
            Self::W45 => Some(45),
            Self::W185 => Some(185),
            Self::W632 => Some(632),
            Self::CustomResized(w) => Some(*w),
            Self::Original(Some(w)) => Some(*w),
            Self::Original(None) => None,
        }
    }
    /// Get the width for this size
    /// Panics if given an Original variant with no width included
    pub const fn width_unchecked(&self) -> u32 {
        match self {
            Self::W45 => 45,
            Self::W185 => 185,
            Self::W632 => 632,
            Self::CustomResized(w) => *w,
            Self::Original(Some(w)) => *w,
            Self::Original(None) => panic!(
                "Width not available for Original variant with no width included"
            ),
        }
    }

    pub fn width_name(&self) -> String {
        match self {
            ProfileSize::W45 | ProfileSize::W185 | ProfileSize::W632 => {
                self.to_tmdb_param().to_string()
            }
            ProfileSize::CustomResized(_) => "custom".to_string(),
            ProfileSize::Original(Some(_)) => "original".to_string(),
            ProfileSize::Original(None) => "original".to_string(),
        }
    }

    pub fn sqlx_image_size_variant(&self) -> SqlxImageSizeVariant {
        match self {
            Self::CustomResized(_) => SqlxImageSizeVariant::Resized,
            Self::Original(_) => SqlxImageSizeVariant::Original,
            _ => SqlxImageSizeVariant::Tmdb,
        }
    }

    pub const fn to_tmdb_param(&self) -> &'static str {
        match self {
            Self::W45 => "w45",
            Self::W185 => "w185",
            Self::W632 => "w632",
            Self::CustomResized(_) => "invalid",
            Self::Original(_) => "original",
        }
    }
}

impl Display for ProfileSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::W45 => write!(f, "45px"),
            Self::W185 => write!(f, "185px"),
            Self::W632 => write!(f, "632px"),
            Self::CustomResized(w) => write!(f, "{}px", w),
            Self::Original(w) => {
                if let Some(w) = w {
                    write!(f, "{}px", w)
                } else {
                    write!(f, "Original")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_height_from_width_matches_known_tmdb_sizes() {
        assert_eq!(BackdropSize::height_from_width(300), 169);
        assert_eq!(BackdropSize::height_from_width(780), 439);
        assert_eq!(BackdropSize::height_from_width(1280), 720);
    }

    #[test]
    fn episode_height_from_width_matches_known_tmdb_sizes() {
        assert_eq!(EpisodeSize::height_from_width(256), 144);
        assert_eq!(EpisodeSize::height_from_width(512), 288);
        assert_eq!(EpisodeSize::height_from_width(768), 432);
    }

    #[test]
    fn image_size_dimensions_unchecked_is_available_for_fixed_sizes() {
        let samples = [
            ImageSize::poster(),
            ImageSize::poster_large(),
            ImageSize::Backdrop(BackdropSize::W780),
            ImageSize::Thumbnail(EpisodeSize::W512),
            ImageSize::Profile(ProfileSize::W185),
        ];

        for imz in samples {
            let dims = imz.dimensions_unchecked();
            assert!(dims.0 > 0);
            assert!(dims.1 > 0);
        }
    }
}
