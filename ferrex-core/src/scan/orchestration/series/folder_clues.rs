use std::path::Path;

use crate::tv_parser::TvParser;

use super::naming::clean_series_title;

/// Extracted hints about a series based on its folder structure.
#[derive(Debug, Clone)]
pub struct SeriesFolderClues {
    pub raw_title: String,
    pub normalized_title: String,
    pub year: Option<u16>,
    pub region: Option<String>,
}

impl SeriesFolderClues {
    fn new(raw_title: String, year: Option<u16>, region: Option<String>) -> Self {
        let normalized_title = clean_series_title(&raw_title);
        Self {
            raw_title,
            normalized_title,
            year,
            region,
        }
    }

    /// Build a default `SeriesFolderClues` when no reliable folder name exists.
    pub fn unknown() -> Self {
        Self::new("Unknown Series".to_string(), None, None)
    }

    /// Extract clues from the filesystem path.
    pub fn from_path(path: &Path) -> Self {
        let Some(folder_name) = TvParser::extract_series_name(path) else {
            return Self::unknown();
        };

        Self::from_folder_name(&folder_name)
    }

    /// Extract clues directly from a folder name when a full path is not available.
    pub fn from_folder_name(name: &str) -> Self {
        parse_folder_name(name)
    }

    /// Merge an alternate title/year (typically from metadata) without
    /// overriding folder-derived hints when they look authoritative.
    pub fn merge_metadata(
        mut self,
        metadata_title: Option<&str>,
        metadata_year: Option<u16>,
    ) -> Self {
        if self.raw_title == "Unknown Series"
            && let Some(title) = metadata_title.map(str::trim).filter(|t| !t.is_empty())
        {
            self.raw_title = title.to_string();
            self.normalized_title = clean_series_title(title);
        }

        if self.year.is_none() {
            self.year = metadata_year.filter(|year| *year >= 1900);
        }

        self
    }
}

fn parse_folder_name(name: &str) -> SeriesFolderClues {
    let mut base = name.trim().to_string();
    let mut year: Option<u16> = None;
    let mut region: Option<String> = None;

    loop {
        let trimmed = base.trim_end();
        if !trimmed.ends_with(')') {
            break;
        }

        let Some(start_idx) = trimmed.rfind('(') else {
            break;
        };
        if start_idx > trimmed.len().saturating_sub(2) {
            break;
        }

        let candidate = &trimmed[start_idx + 1..trimmed.len() - 1];
        let candidate = candidate.trim();

        if candidate.is_empty() {
            break;
        }

        let before = trimmed[..start_idx].trim_end();

        if year.is_none() && is_year_hint(candidate) {
            year = candidate.parse::<u16>().ok();
            base = before.to_string();
            continue;
        }

        if region.is_none()
            && let Some(region_hint) = normalize_region(candidate)
        {
            region = Some(region_hint);
            base = before.to_string();
            continue;
        }

        break;
    }

    if base.is_empty() {
        return SeriesFolderClues::unknown();
    }

    let lowered = base.to_ascii_lowercase();
    if TvParser::parse_season_folder(&base).is_some()
        || matches!(lowered.as_str(), "extras" | "specials" | "special")
    {
        return SeriesFolderClues::unknown();
    }

    SeriesFolderClues::new(base, year, region)
}

fn is_year_hint(candidate: &str) -> bool {
    candidate.len() == 4 && candidate.chars().all(|ch| ch.is_ascii_digit())
}

fn normalize_region(candidate: &str) -> Option<String> {
    let trimmed = candidate.trim_matches(|ch: char| ch == '[' || ch == ']');
    let upper = trimmed.to_ascii_uppercase();

    let valid = !upper.is_empty()
        && upper.len() <= 6
        && upper
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || ch == '-' || ch == '_');

    if valid {
        Some(upper.replace('_', "-"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_title_year_and_region() {
        let path = PathBuf::from("/media/The Office (2005) (US)/Season 01/E01.mkv");
        let clues = SeriesFolderClues::from_path(&path);
        assert_eq!(clues.raw_title, "The Office");
        assert_eq!(clues.year, Some(2005));
        assert_eq!(clues.region.as_deref(), Some("US"));
        assert_eq!(clues.normalized_title, "The Office");
    }

    #[test]
    fn keeps_region_when_year_missing() {
        let path = PathBuf::from("/library/Taskmaster (UK)/Season 02/E02.mkv");
        let clues = SeriesFolderClues::from_path(&path);
        assert_eq!(clues.raw_title, "Taskmaster");
        assert_eq!(clues.year, None);
        assert_eq!(clues.region.as_deref(), Some("UK"));
    }

    #[test]
    fn ignores_season_folders_as_series_names() {
        let path = PathBuf::from("/media/Season 01/Episode.mkv");
        let clues = SeriesFolderClues::from_path(&path);
        assert_eq!(clues.raw_title, "Unknown Series");
        assert!(clues.year.is_none());
        assert!(clues.region.is_none());
    }

    #[test]
    fn parses_series_when_season_folder_repeats_name() {
        let path = PathBuf::from("/media/The Office/The Office S02/Episode.mkv");
        let clues = SeriesFolderClues::from_path(&path);
        assert_eq!(clues.raw_title, "The Office");
        assert_eq!(clues.year, None);
        assert!(clues.region.is_none());
    }
}
