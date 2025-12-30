use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

fn parse_date(value: &str) -> Option<NaiveDate> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").ok()
}

fn deserialize_optional_date<'de, D>(
    deserializer: D,
) -> Result<Option<NaiveDate>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.as_deref().and_then(parse_date))
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscoverPage<T> {
    pub page: u32,
    pub results: Vec<T>,
    pub total_pages: u32,
    pub total_results: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscoverMovieItem {
    pub id: u64,
    pub title: String,
    #[serde(default, deserialize_with = "deserialize_optional_date")]
    pub release_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscoverTvItem {
    pub id: u64,
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_optional_date")]
    pub first_air_date: Option<NaiveDate>,
    #[serde(default)]
    pub origin_country: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverMovieQuery<'a> {
    pub api_key: &'a str,
    pub sort_by: &'a str,
    pub include_adult: bool,
    pub include_video: bool,
    pub page: u32,
    pub primary_release_year: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverTvQuery<'a> {
    pub api_key: &'a str,
    pub sort_by: &'a str,
    pub include_adult: bool,
    pub page: u32,
    pub first_air_date_year: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<&'a str>,
}
