use crate::{
    ExtrasParser, LibraryType, ParsedEpisodeInfo, ParsedMediaInfo, ParsedMovieInfo, TvParser,
};
use regex::Regex;
use std::path::Path;
use tracing::info;

#[derive(Debug, Clone, Default)]
pub struct FilenameParser {
    library_type: Option<LibraryType>,
}

impl FilenameParser {
    pub fn new() -> Self {
        Self { library_type: None }
    }

    pub fn with_library_type(library_type: LibraryType) -> Self {
        Self {
            library_type: Some(library_type),
        }
    }

    pub fn set_library_type(&mut self, library_type: Option<LibraryType>) {
        self.library_type = library_type;
    }

    /// Parse filename with library type context
    pub fn parse_filename_with_type<P: AsRef<Path>>(
        &self,
        file_path: P,
    ) -> Option<ParsedMediaInfo> {
        let file_path = file_path.as_ref();

        if ExtrasParser::is_extra(file_path) {
            return None;
        }

        let episode = self.try_parse_episode(file_path);
        let movie = self.try_parse_movie(file_path);

        match self.library_type {
            Some(LibraryType::Series) => episode.or(movie),
            Some(LibraryType::Movies) => movie.or(episode),
            None => episode.or(movie),
        }
    }

    fn try_parse_episode(&self, file_path: &Path) -> Option<ParsedMediaInfo> {
        let filename = file_path.file_stem()?.to_str()?;
        let info = TvParser::parse_episode_info(file_path)?;
        let episode_title = TvParser::extract_episode_title(file_path)
            .and_then(|title| self.clean_episode_title(&title));

        let raw_show_name = self
            .preferred_series_name(file_path)
            .unwrap_or_else(|| self.clean_filename(filename));
        let show_name = self.clean_series_name(&raw_show_name);

        Some(ParsedMediaInfo::Episode(ParsedEpisodeInfo {
            show_name,
            season: info.season,
            episode: info.episode,
            episode_title,
            year: self.extract_year(filename),
            resolution: self.extract_resolution(filename),
            source: self.extract_source(filename),
            release_group: self.extract_release_group(filename),
        }))
    }

    fn try_parse_movie(&self, file_path: &Path) -> Option<ParsedMediaInfo> {
        let filename = file_path.file_stem()?.to_str()?;

        if TvParser::parse_episode_info(file_path).is_some() {
            return None;
        }

        self.parse_as_movie(filename, file_path)
    }

    fn preferred_series_name(&self, file_path: &Path) -> Option<String> {
        TvParser::extract_series_name(file_path).or_else(|| {
            file_path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
        })
    }

    fn clean_series_name(&self, raw: &str) -> String {
        let mut name = raw.replace(['.', '_'], " ");
        let year_pattern = Regex::new(r"\s*\(\d{4}\)\s*$").unwrap();
        name = year_pattern.replace(&name, "").to_string();

        let indicators = [
            r"(?i)[\s_-]s\d{1,3}e\d{1,3}",
            r"(?i)[\s_-]\d{1,2}x\d{1,3}",
            r"(?i)[\s_-]episode\s*\d{1,3}",
            r"(?i)[\s_-]part\s*\d{1,3}",
            r"(?i)[\s_-]chapter\s*\d{1,3}",
            r"(?i)[\s_-]\d{2,3}\s*-",
        ];

        let mut cutoff = name.len();
        for pattern in indicators {
            if let Some(m) = Regex::new(pattern).unwrap().find(&name) {
                cutoff = cutoff.min(m.start());
            }
        }
        name.truncate(cutoff);

        let release_suffix = Regex::new(r"\s*-\s*\w+$").unwrap();
        name = release_suffix.replace(&name, "").to_string();

        let cleaned = name.split_whitespace().collect::<Vec<_>>().join(" ");
        cleaned
            .trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '_')
            .to_string()
    }

    fn clean_episode_title(&self, raw: &str) -> Option<String> {
        let mut title = raw.replace(['.', '_'], " ");

        let noise_tokens = [
            "1080p", "720p", "2160p", "4K", "BluRay", "Bluray", "WEB-DL", "WEBRip", "WEBDL",
            "HDRip", "HDTV", "DVDRip", "x264", "x265", "HEVC", "10bit", "AAC", "DTS", "FLAC",
        ];
        for token in noise_tokens {
            let pattern = Regex::new(&format!(r"(?i)\b{}\b", token)).unwrap();
            title = pattern.replace_all(&title, "").to_string();
        }

        let release_suffix = Regex::new(r"\s*-\s*\w+$").unwrap();
        title = release_suffix.replace(&title, "").to_string();

        let cleaned = title.split_whitespace().collect::<Vec<_>>().join(" ");
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Force parse as movie (used when we know from folder structure)
    fn parse_as_movie(&self, filename: &str, file_path: &Path) -> Option<ParsedMediaInfo> {
        // First, try to parse the parent folder name
        if let Some(parent) = file_path.parent() {
            if let Some(folder_name) = parent.file_name() {
                if let Some(folder_str) = folder_name.to_str() {
                    info!("Trying to parse movie from folder name: {}", folder_str);

                    // Try to match "movie_name (year)" pattern in folder name
                    let folder_regex = Regex::new(r"^(.+?)\s*\((\d{4})\)\s*$").ok();
                    if let Some(regex) = folder_regex {
                        if let Some(captures) = regex.captures(folder_str) {
                            if let (Some(title_match), Some(year_match)) =
                                (captures.get(1), captures.get(2))
                            {
                                let title = title_match.as_str().trim().to_string();
                                if let Ok(year) = year_match.as_str().parse::<u16>() {
                                    if (1900..=2100).contains(&year) {
                                        info!(
                                            "Successfully parsed movie from folder: {} ({})",
                                            title, year
                                        );
                                        return Some(ParsedMediaInfo::Movie(ParsedMovieInfo {
                                            title,
                                            year: Some(year),
                                            resolution: self.extract_resolution(filename),
                                            source: self.extract_source(filename),
                                            release_group: self.extract_release_group(filename),
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fall back to filename parsing if folder parsing failed
        info!("Folder parsing failed, falling back to filename parsing");

        // Extract year from original filename first (before any modifications)
        let year = self.extract_year(filename);

        // Remove file extension first before any processing
        let mut cleaned_title = filename.to_string();
        cleaned_title = Regex::new(r"(?i)\.(mkv|mp4|avi|mov|wmv|flv|webm|m4v|mpg|mpeg)$")
            .unwrap()
            .replace(&cleaned_title, "")
            .to_string();

        // Handle multi-language titles (e.g., "Il Gladiatore II - Gladiator II")
        // If there's a dash with potential duplicate title, take the part after the dash
        if let Some(dash_pos) = cleaned_title.find(" - ") {
            let _before_dash = &cleaned_title[..dash_pos];
            let after_dash = &cleaned_title[dash_pos + 3..];

            // Check if the part after dash looks like an English title
            if after_dash.chars().any(|c| c.is_ascii_alphabetic())
                && !after_dash.chars().all(|c| c.is_ascii_uppercase())
            {
                // Use the part after the dash if it looks like a proper title
                cleaned_title = after_dash.to_string();
            }
        }

        // Remove year from the title if present
        if let Some(y) = year {
            cleaned_title = cleaned_title.replace(&format!(" {y}"), "");
            cleaned_title = cleaned_title.replace(&format!("({y})"), "");
            cleaned_title = cleaned_title.replace(&format!(".{y}"), ""); // Handle .2008 at end
            cleaned_title = cleaned_title.replace(&format!(".{y}."), " ");
            cleaned_title = cleaned_title.replace(&format!(" {y} "), " ");
        }

        // Now clean the title
        cleaned_title = self.clean_movie_title(&cleaned_title);

        Some(ParsedMediaInfo::Movie(ParsedMovieInfo {
            title: cleaned_title,
            year,
            resolution: self.extract_resolution(filename),
            source: self.extract_source(filename),
            release_group: self.extract_release_group(filename),
        }))
    }

    /// Clean movie title more aggressively for TMDB search
    fn clean_movie_title(&self, title: &str) -> String {
        let mut cleaned = title.to_string();

        // Remove file extensions first (case-insensitive)
        cleaned = Regex::new(r"(?i)\.(mkv|mp4|avi|mov|wmv|flv|webm|m4v|mpg|mpeg)$")
            .unwrap()
            .replace(&cleaned, "")
            .to_string();

        // First pass: Remove everything in square brackets
        cleaned = Regex::new(r"\[.*?\]")
            .unwrap()
            .replace_all(&cleaned, " ")
            .to_string();

        // Remove everything after the first occurrence of common quality/format indicators
        // This handles cases like "Movie Title (2014) (1080p..." by cutting at the second parenthesis
        let quality_cutoff_regex = Regex::new(
            r"(?i)\s*[\(\[]?\s*(BluRay|Bluray|BDRip|BRRip|WEBRip|WEB-DL|WEBDL-1080p|WebDl|SDTV|HDTV|DVDRip|CAM|TS|HC|HDCAM|HDRip|dvd|dvdrip|xvid|divx|x264|x265|h264|h265|hevc|10bit|10\s*bit|HDR|HDR10|DV|AC3|AAC|DTS|FLAC|Remux|REMUX|1080p|720p|480p|2160p|4K|UHD|[\(\[]?\d{3,4}p).*$"
        ).unwrap();
        cleaned = quality_cutoff_regex.replace(&cleaned, "").to_string();

        // Remove edition info
        let edition_regex = Regex::new(
            r"(?i)[\s\-]*(unrated|extended|director'?s?\s*cut|theatrical|special\s*edition|ultimate\s*edition|final\s*cut|remastered|uncut|unknown).*$"
        ).unwrap();
        cleaned = edition_regex.replace(&cleaned, "").to_string();

        // Now remove any remaining content in parentheses (but be careful about nested or unmatched)
        // This regex handles nested parentheses better
        while cleaned.contains('(') || cleaned.contains(')') {
            let old_len = cleaned.len();
            cleaned = Regex::new(r"\([^()]*\)")
                .unwrap()
                .replace_all(&cleaned, " ")
                .to_string();
            // Also remove any lone parentheses
            cleaned = cleaned.replace(['(', ')'], " ");
            if cleaned.len() == old_len {
                break; // Prevent infinite loop
            }
        }

        // Replace dots and underscores with spaces
        cleaned = cleaned.replace(['.', '_'], " ");

        // Remove standalone years (1900-2100)
        cleaned = Regex::new(r"\b(19|20)\d{2}\b")
            .unwrap()
            .replace_all(&cleaned, "")
            .to_string();

        // Remove release group patterns (dash followed by group name at end)
        cleaned = Regex::new(r"\s*-\s*\w+$")
            .unwrap()
            .replace(&cleaned, "")
            .to_string();

        // Clean up extra whitespace and punctuation
        cleaned = cleaned.split_whitespace().collect::<Vec<&str>>().join(" ");

        // Final cleanup: remove any trailing punctuation
        cleaned = cleaned
            .trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
            .to_string();

        cleaned
    }

    /// Extract year from filename
    pub fn extract_year(&self, filename: &str) -> Option<u16> {
        // Updated regex to match years more accurately
        // Look for 4-digit years between 1900-2100, surrounded by non-digit characters
        let year_regex = Regex::new(r"(?:^|[^\d])(19\d{2}|20\d{2})(?:[^\d]|$)").ok()?;

        if let Some(captures) = year_regex.captures(filename) {
            if let Some(year_match) = captures.get(1) {
                if let Ok(year) = year_match.as_str().parse::<u16>() {
                    // Validate the year is reasonable
                    if (1900..=2100).contains(&year) {
                        return Some(year);
                    }
                }
            }
        }

        None
    }

    /// Extract resolution (e.g., 1080p, 720p, 4K)
    pub fn extract_resolution(&self, filename: &str) -> Option<String> {
        let resolutions = [
            "2160p", "4K", "UHD", "1080p", "720p", "480p", "576p", "360p",
        ];

        for res in resolutions {
            if filename.contains(res) {
                return Some(res.to_string());
            }
        }

        None
    }

    /// Extract source (e.g., BluRay, WEB-DL, HDTV)
    pub fn extract_source(&self, filename: &str) -> Option<String> {
        let sources = [
            "BluRay", "Bluray", "BDRip", "BRRip", "WEBRip", "WEB-DL", "WebDl", "HDTV", "SDTV",
            "DVDRip", "DVD", "CAM", "TS", "HC", "HDCAM", "HDRip",
        ];

        for source in sources {
            if filename.contains(source) {
                return Some(source.to_string());
            }
        }

        None
    }

    /// Extract release group (usually at the end after a dash)
    pub fn extract_release_group(&self, filename: &str) -> Option<String> {
        // Look for pattern like "-GROUP" at the end of filename (before extension)
        let group_regex = Regex::new(r"-(\w+)(?:\.\w+)?$").ok()?;

        if let Some(captures) = group_regex.captures(filename) {
            if let Some(group_match) = captures.get(1) {
                return Some(group_match.as_str().to_string());
            }
        }

        None
    }

    /// Clean filename by removing common artifacts
    pub fn clean_filename(&self, filename: &str) -> String {
        let mut cleaned = filename.to_string();

        // Remove file extensions
        cleaned = cleaned
            .replace(".mkv", "")
            .replace(".mp4", "")
            .replace(".avi", "")
            .replace(".mov", "")
            .replace(".wmv", "")
            .replace(".flv", "")
            .replace(".webm", "");

        // Replace dots and underscores with spaces
        cleaned = cleaned.replace(['.', '_'], " ");

        // Remove quality indicators
        let quality_indicators = [
            "1080p", "720p", "480p", "2160p", "4K", "BluRay", "BDRip", "WEBRip", "WEB-DL", "HDTV",
            "x264", "x265", "HEVC", "10bit", "HDR",
        ];

        for indicator in quality_indicators {
            cleaned = cleaned.replace(indicator, "");
        }

        // Remove brackets and their contents
        cleaned = Regex::new(r"\[.*?\]")
            .unwrap()
            .replace_all(&cleaned, "")
            .to_string();
        cleaned = Regex::new(r"\(.*?\)")
            .unwrap()
            .replace_all(&cleaned, "")
            .to_string();

        // Clean up extra whitespace
        cleaned = cleaned.split_whitespace().collect::<Vec<&str>>().join(" ");

        cleaned.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_year() {
        let parser = FilenameParser::new();

        assert_eq!(parser.extract_year("Movie.2023.1080p.mkv"), Some(2023));
        assert_eq!(parser.extract_year("Movie (2023)"), Some(2023));
        assert_eq!(parser.extract_year("Movie 2023"), Some(2023));
        assert_eq!(parser.extract_year("Movie.mkv"), None);
        assert_eq!(parser.extract_year("12345.mkv"), None); // Not a valid year
    }

    #[test]
    fn test_extract_resolution() {
        let parser = FilenameParser::new();

        assert_eq!(
            parser.extract_resolution("Movie.1080p.BluRay.mkv"),
            Some("1080p".to_string())
        );
        assert_eq!(
            parser.extract_resolution("Movie.4K.WEB-DL.mkv"),
            Some("4K".to_string())
        );
        assert_eq!(parser.extract_resolution("Movie.mkv"), None);
    }

    #[test]
    fn test_try_parse_episode() {
        let parser = FilenameParser::new();

        // Test pattern 1
        let result = parser
            .try_parse_episode(Path::new("Breaking.Bad.S01E01.Pilot.1080p.BluRay.mkv"))
            .unwrap();
        if let ParsedMediaInfo::Episode(info) = result {
            assert_eq!(info.show_name, "Breaking Bad");
            assert_eq!(info.season, 1);
            assert_eq!(info.episode, 1);
            assert_eq!(info.episode_title, Some("Pilot".to_string()));
        } else {
            panic!("Expected Episode variant");
        }

        // Test pattern 2
        let result = parser
            .try_parse_episode(Path::new("Breaking Bad S01E01.mkv"))
            .unwrap();
        if let ParsedMediaInfo::Episode(info) = result {
            assert_eq!(info.show_name, "Breaking Bad");
            assert_eq!(info.season, 1);
            assert_eq!(info.episode, 1);
        } else {
            panic!("Expected Episode variant");
        }
    }

    #[test]
    fn test_parse_movie() {
        let parser = FilenameParser::new();

        let result = parser
            .parse_as_movie("The.Dark.Knight.2008.1080p.BluRay.x264.mkv", Path::new(""))
            .unwrap();
        if let ParsedMediaInfo::Movie(info) = result {
            assert_eq!(info.title, "The Dark Knight");
            assert_eq!(info.year, Some(2008));
            assert_eq!(info.resolution, Some("1080p".to_string()));
        } else {
            panic!("Expected Movie variant");
        }
    }
}
