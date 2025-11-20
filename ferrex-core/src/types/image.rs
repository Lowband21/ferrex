use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ImageMetadata {
    pub file_path: String,
    pub width: u64,
    pub height: u64,
    pub aspect_ratio: f64,
    pub iso_639_1: Option<String>,
    pub vote_average: f64,
    pub vote_count: u64,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ImageWithMetadata {
    pub endpoint: String,
    pub metadata: ImageMetadata,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    Default,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct MediaImages {
    pub posters: Vec<ImageWithMetadata>,
    pub backdrops: Vec<ImageWithMetadata>,
    pub logos: Vec<ImageWithMetadata>,
    pub stills: Vec<ImageWithMetadata>,
}
