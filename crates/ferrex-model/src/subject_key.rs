use std::fmt;

/// Errors produced when constructing strongly-typed subject keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectKeyError {
    Empty,
}

impl fmt::Display for SubjectKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubjectKeyError::Empty => write!(f, "subject key cannot be empty"),
        }
    }
}

/// Normalized filesystem path key used for scan orchestration and progress
/// tracking (e.g. `folder_path_norm`, `path_norm`).
///
/// This is intentionally a thin wrapper around `String` so:
/// - call sites can't accidentally pass an arbitrary string without opting in
/// - serialization remains compact and ergonomic
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub struct NormalizedPathKey(String);

impl NormalizedPathKey {
    pub fn new(value: impl Into<String>) -> Result<Self, SubjectKeyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SubjectKeyError::Empty);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for NormalizedPathKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque stable identifier that isn't necessarily a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub struct OpaqueSubjectKey(String);

impl OpaqueSubjectKey {
    pub fn new(value: impl Into<String>) -> Result<Self, SubjectKeyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SubjectKeyError::Empty);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for OpaqueSubjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Typed identifier for "what this event/job is about".
///
/// This replaces stringly-typed `path_key` usage while keeping the payload
/// lightweight and serializable across process boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(tag = "type", content = "value", rename_all = "snake_case")
)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub enum SubjectKey {
    /// A normalized filesystem path key.
    Path(NormalizedPathKey),
    /// A stable identifier in a different keyspace (e.g. fingerprint hash,
    /// remote provider path, synthetic image key).
    Opaque(OpaqueSubjectKey),
}

impl SubjectKey {
    pub fn path(value: impl Into<String>) -> Result<Self, SubjectKeyError> {
        Ok(Self::Path(NormalizedPathKey::new(value)?))
    }

    pub fn opaque(value: impl Into<String>) -> Result<Self, SubjectKeyError> {
        Ok(Self::Opaque(OpaqueSubjectKey::new(value)?))
    }

    pub fn as_str(&self) -> &str {
        match self {
            SubjectKey::Path(value) => value.as_str(),
            SubjectKey::Opaque(value) => value.as_str(),
        }
    }
}

impl fmt::Display for SubjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubjectKey::Path(value) => write!(f, "path:{value}"),
            SubjectKey::Opaque(value) => write!(f, "opaque:{value}"),
        }
    }
}
