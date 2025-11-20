use rkyv::{
    Archive, Archived, Deserialize, Place, Serialize,
    rancor::Fallible,
    with::{ArchiveWith, DeserializeWith, SerializeWith},
};
use std::path::PathBuf;

// Wrapper for PathBuf
#[derive(Debug, Clone, Copy, Default)]
pub struct PathBufWrapper;

impl ArchiveWith<PathBuf> for PathBufWrapper {
    type Archived = Archived<String>;
    type Resolver = <String as Archive>::Resolver;

    fn resolve_with(field: &PathBuf, resolver: Self::Resolver, out: Place<Self::Archived>) {
        let path_str = field.to_string_lossy().to_string();
        path_str.resolve(resolver, out);
    }
}

impl<S: Fallible + ?Sized> SerializeWith<PathBuf, S> for PathBufWrapper
where
    String: Serialize<S>,
{
    fn serialize_with(field: &PathBuf, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        let path_str = field.to_string_lossy().to_string();
        path_str.serialize(serializer)
    }
}

impl<D: Fallible + ?Sized> DeserializeWith<Archived<String>, PathBuf, D> for PathBufWrapper
where
    Archived<String>: Deserialize<String, D>,
{
    fn deserialize_with(
        field: &Archived<String>,
        deserializer: &mut D,
    ) -> Result<PathBuf, D::Error> {
        let path_str: String = field.deserialize(deserializer)?;
        Ok(PathBuf::from(path_str))
    }
}

// Wrapper for chrono::DateTime<chrono::Utc>
#[derive(Debug, Clone, Copy, Default)]
pub struct DateTimeWrapper;

impl ArchiveWith<chrono::DateTime<chrono::Utc>> for DateTimeWrapper {
    type Archived = Archived<i64>; // Use Archived<i64> instead of raw i64
    type Resolver = <i64 as Archive>::Resolver;

    fn resolve_with(
        field: &chrono::DateTime<chrono::Utc>,
        resolver: Self::Resolver,
        out: Place<Self::Archived>,
    ) {
        let timestamp = field.timestamp();
        timestamp.resolve(resolver, out);
    }
}

impl<S: Fallible + ?Sized> SerializeWith<chrono::DateTime<chrono::Utc>, S> for DateTimeWrapper
where
    i64: Serialize<S>,
{
    fn serialize_with(
        field: &chrono::DateTime<chrono::Utc>,
        serializer: &mut S,
    ) -> Result<Self::Resolver, <S as Fallible>::Error> {
        let timestamp = field.timestamp();
        timestamp.serialize(serializer)
    }
}

impl<D: Fallible + ?Sized> DeserializeWith<Archived<i64>, chrono::DateTime<chrono::Utc>, D>
    for DateTimeWrapper
where
    Archived<i64>: Deserialize<i64, D>,
{
    fn deserialize_with(
        field: &Archived<i64>,
        deserializer: &mut D,
    ) -> Result<chrono::DateTime<chrono::Utc>, <D as Fallible>::Error> {
        let timestamp: i64 = field.deserialize(deserializer)?;
        Ok(chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(chrono::Utc::now))
    }
}

// Wrapper for Vec<PathBuf>
#[derive(Debug, Clone, Copy, Default)]
pub struct VecPathBuf;

impl ArchiveWith<Vec<PathBuf>> for VecPathBuf {
    type Archived = Archived<Vec<String>>;
    type Resolver = <Vec<String> as Archive>::Resolver;

    fn resolve_with(field: &Vec<PathBuf>, resolver: Self::Resolver, out: Place<Self::Archived>) {
        let strings: Vec<String> = field
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        strings.resolve(resolver, out);
    }
}

impl<S: Fallible + ?Sized> SerializeWith<Vec<PathBuf>, S> for VecPathBuf
where
    Vec<String>: Serialize<S>,
{
    fn serialize_with(
        field: &Vec<PathBuf>,
        serializer: &mut S,
    ) -> Result<Self::Resolver, <S as Fallible>::Error> {
        let strings: Vec<String> = field
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        strings.serialize(serializer)
    }
}

impl<D: Fallible + ?Sized> DeserializeWith<Archived<Vec<String>>, Vec<PathBuf>, D> for VecPathBuf
where
    Archived<Vec<String>>: Deserialize<Vec<String>, D>,
{
    fn deserialize_with(
        field: &Archived<Vec<String>>,
        deserializer: &mut D,
    ) -> Result<Vec<PathBuf>, <D as Fallible>::Error> {
        let strings: Vec<String> = field.deserialize(deserializer)?;
        Ok(strings.into_iter().map(PathBuf::from).collect())
    }
}

// Wrapper for Option<DateTime>
#[derive(Debug, Clone, Copy, Default)]
pub struct OptionDateTime;

impl ArchiveWith<Option<chrono::DateTime<chrono::Utc>>> for OptionDateTime {
    type Archived = Archived<Option<i64>>;
    type Resolver = <Option<i64> as Archive>::Resolver;

    fn resolve_with(
        field: &Option<chrono::DateTime<chrono::Utc>>,
        resolver: Self::Resolver,
        out: Place<Self::Archived>,
    ) {
        let timestamp = field.map(|dt| dt.timestamp());
        timestamp.resolve(resolver, out);
    }
}

impl<S: Fallible + ?Sized> SerializeWith<Option<chrono::DateTime<chrono::Utc>>, S>
    for OptionDateTime
where
    Option<i64>: Serialize<S>,
{
    fn serialize_with(
        field: &Option<chrono::DateTime<chrono::Utc>>,
        serializer: &mut S,
    ) -> Result<Self::Resolver, <S as Fallible>::Error> {
        let timestamp = field.map(|dt| dt.timestamp());
        timestamp.serialize(serializer)
    }
}

impl<D: Fallible + ?Sized>
    DeserializeWith<Archived<Option<i64>>, Option<chrono::DateTime<chrono::Utc>>, D>
    for OptionDateTime
where
    Archived<Option<i64>>: Deserialize<Option<i64>, D>,
{
    fn deserialize_with(
        field: &Archived<Option<i64>>,
        deserializer: &mut D,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, <D as Fallible>::Error> {
        let timestamp: Option<i64> = field.deserialize(deserializer)?;
        Ok(timestamp.and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)))
    }
}

// Wrapper for std::time::Duration
#[derive(Debug, Clone, Copy, Default)]
pub struct DurationWrapper;

impl ArchiveWith<std::time::Duration> for DurationWrapper {
    type Archived = Archived<u64>; // Store as total seconds
    type Resolver = <u64 as Archive>::Resolver;

    fn resolve_with(
        field: &std::time::Duration,
        resolver: Self::Resolver,
        out: Place<Self::Archived>,
    ) {
        let secs = field.as_secs();
        secs.resolve(resolver, out);
    }
}

impl<S: Fallible + ?Sized> SerializeWith<std::time::Duration, S> for DurationWrapper
where
    u64: Serialize<S>,
{
    fn serialize_with(
        field: &std::time::Duration,
        serializer: &mut S,
    ) -> Result<Self::Resolver, <S as Fallible>::Error> {
        let secs = field.as_secs();
        secs.serialize(serializer)
    }
}

impl<D: Fallible + ?Sized> DeserializeWith<Archived<u64>, std::time::Duration, D>
    for DurationWrapper
where
    Archived<u64>: Deserialize<u64, D>,
{
    fn deserialize_with(
        field: &Archived<u64>,
        deserializer: &mut D,
    ) -> Result<std::time::Duration, <D as Fallible>::Error> {
        let secs: u64 = field.deserialize(deserializer)?;
        Ok(std::time::Duration::from_secs(secs))
    }
}
