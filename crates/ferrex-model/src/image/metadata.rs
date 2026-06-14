use std::fmt;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ImageMetadata {
    pub file_path: String,
    pub width: u64,
    pub height: u64,
    pub aspect_ratio: f64,
    pub iso_639_1: Option<String>,
    pub vote_average: f64,
    pub vote_count: u64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ImageWithMetadata {
    pub endpoint: String,
    pub metadata: ImageMetadata,
}

#[derive(Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct MediaImages {
    pub posters: Vec<ImageWithMetadata>,
    pub backdrops: Vec<ImageWithMetadata>,
    pub logos: Vec<ImageWithMetadata>,
    pub stills: Vec<ImageWithMetadata>,
}

impl fmt::Debug for MediaImages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaImages")
            .field("poster_count", &self.posters.len())
            .field("backdrop_count", &self.backdrops.len())
            .field("logo_count", &self.logos.len())
            .field("still_count", &self.stills.len())
            .finish()
    }
}
