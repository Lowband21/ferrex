use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SessionScope {
    /// Full access session, typically created after password authentication
    #[default]
    Full,
    /// Playback-only scope created from reduced trust flows (e.g. PIN unlock)
    Playback,
}

impl SessionScope {
    pub const FULL: &'static str = "full";
    pub const PLAYBACK: &'static str = "playback";

    pub fn as_str(self) -> &'static str {
        match self {
            SessionScope::Full => Self::FULL,
            SessionScope::Playback => Self::PLAYBACK,
        }
    }
}

impl fmt::Display for SessionScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SessionScope {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            Self::FULL => Ok(SessionScope::Full),
            Self::PLAYBACK => Ok(SessionScope::Playback),
            _ => Err("invalid session scope"),
        }
    }
}

impl TryFrom<&str> for SessionScope {
    type Error = &'static str;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        SessionScope::from_str(value)
    }
}

impl TryFrom<String> for SessionScope {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        SessionScope::from_str(value.as_str())
    }
}
