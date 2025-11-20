use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// Season number with u8 bounds
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, Clone, PartialEq, Eq, Hash))]
pub struct SeasonNumber(u8);

impl SeasonNumber {
    pub fn new(num: u8) -> Self {
        SeasonNumber(num)
    }

    pub fn value(&self) -> u8 {
        self.0
    }
}

impl std::fmt::Display for SeasonNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u8> for SeasonNumber {
    fn from(num: u8) -> Self {
        SeasonNumber(num)
    }
}

impl Default for SeasonNumber {
    fn default() -> Self {
        SeasonNumber(1) // Season 1 is a reasonable default
    }
}

/// Episode number with u8 bounds
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, Clone, PartialEq, Eq, Hash))]
pub struct EpisodeNumber(u8);

impl EpisodeNumber {
    pub fn new(num: u8) -> Self {
        EpisodeNumber(num)
    }

    pub fn value(&self) -> u8 {
        self.0
    }
}

impl std::fmt::Display for EpisodeNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u8> for EpisodeNumber {
    fn from(num: u8) -> Self {
        EpisodeNumber(num)
    }
}

impl Default for EpisodeNumber {
    fn default() -> Self {
        EpisodeNumber(1) // Episode 1 is a reasonable default
    }
}
