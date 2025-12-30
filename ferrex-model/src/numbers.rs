/// Season number with u8 bounds
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, Clone, PartialEq, Eq, Hash)))]
pub struct SeasonNumber(u16);

impl SeasonNumber {
    pub fn new(num: u16) -> Self {
        SeasonNumber(num)
    }

    pub fn value(&self) -> u16 {
        self.0
    }
}

impl std::fmt::Display for SeasonNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for SeasonNumber {
    fn from(num: u16) -> Self {
        SeasonNumber(num)
    }
}

impl Default for SeasonNumber {
    fn default() -> Self {
        SeasonNumber(1) // Season 1 is a reasonable default
    }
}

/// Episode number with u8 bounds
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, Clone, PartialEq, Eq, Hash)))]
pub struct EpisodeNumber(u16);

impl EpisodeNumber {
    pub fn new(num: u16) -> Self {
        EpisodeNumber(num)
    }

    pub fn value(&self) -> u16 {
        self.0
    }
}

impl std::fmt::Display for EpisodeNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for EpisodeNumber {
    fn from(num: u16) -> Self {
        EpisodeNumber(num)
    }
}

impl Default for EpisodeNumber {
    fn default() -> Self {
        EpisodeNumber(1) // Episode 1 is a reasonable default
    }
}
