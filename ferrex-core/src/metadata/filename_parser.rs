use crate::{
    ExtrasParser, LibraryType, ParsedEpisodeInfo, ParsedMediaInfo, ParsedMovieInfo, TvParser,
};
use regex::Regex;
use std::path::Path;
use tracing::{debug, info};

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

        // Check if this is an extra - if so, skip parsing
        if ExtrasParser::is_extra(file_path) {
            return None;
        }

        // Based on library type, try to parse appropriately
        match self.library_type {
            Some(LibraryType::Series) => {
                // Try to parse as TV episode
                if let Some(episode_info) = TvParser::parse_episode_info(file_path) {
                    let show_name = TvParser::extract_series_name(file_path)
                        .or_else(|| self.extract_show_name_from_path(file_path));
                    let episode_title = TvParser::extract_episode_title(file_path);
                    let filename = file_path.file_stem()?.to_str()?;

                    // Clean show name to remove year in parentheses
                    let cleaned_show_name = show_name.map(|name| {
                        let year_pattern = regex::Regex::new(r"\s*\(\d{4}\)\s*$").unwrap();
                        if year_pattern.is_match(&name) {
                            let cleaned = year_pattern.replace(&name, "").to_string();
                            info!("Cleaned show name from '{}' to '{}'", name, cleaned);
                            cleaned
                        } else {
                            name
                        }
                    });

                    if let Some(show_name) = cleaned_show_name {
                        return Some(ParsedMediaInfo::Episode(ParsedEpisodeInfo {
                            show_name,
                            season: episode_info.season,
                            episode: episode_info.episode,
                            episode_title,
                            year: self.extract_year(filename),
                            resolution: self.extract_resolution(filename),
                            source: self.extract_source(filename),
                            release_group: self.extract_release_group(filename),
                        }));
                    }
                }
            }
            Some(LibraryType::Movies) | None => {
                // Try to parse as movie
                if let Some(filename) = file_path.file_stem().and_then(|s| s.to_str()) {
                    return self.parse_as_movie(filename, file_path);
                }
            }
        }

        // Fallback to old parsing logic
        self.parse_filename(file_path)
    }

    /// Parse filename to extract show/episode information (legacy method)
    pub fn parse_filename<P: AsRef<Path>>(&self, file_path: P) -> Option<ParsedMediaInfo> {
        let file_path = file_path.as_ref();
        let filename = file_path.file_stem()?.to_str()?;

        info!("=== METADATA PARSING ===");
        info!("Full path: {:?}", file_path);
        info!("Filename: {}", filename);

        // First, check folder structure to determine media type
        let path_str = file_path.to_string_lossy();
        info!("Path string: {}", path_str);

        // Check for folder names case-insensitively
        let path_lower = path_str.to_lowercase();
        let is_in_movies_folder =
            path_lower.contains("/movies/") || path_lower.contains("\\movies\\");
        let is_in_tvshows_folder = path_lower.contains("/tvshows/")
            || path_lower.contains("\\tvshows\\")
            || path_lower.contains("/tv shows/")
            || path_lower.contains("\\tv shows\\")
            || path_lower.contains("/tv-shows/")
            || path_lower.contains("\\tv-shows\\")
            || path_lower.contains("/series/")
            || path_lower.contains("\\series\\");

        info!("Is in movies folder: {}", is_in_movies_folder);
        info!("Is in tvshows folder: {}", is_in_tvshows_folder);

        // If in movies folder, parse as movie
        if is_in_movies_folder {
            info!("File is in movies folder, parsing as movie");
            return self.parse_as_movie(filename, file_path);
        }

        // If in tvshows folder, parse as TV show
        if is_in_tvshows_folder {
            info!("File is in tvshows folder, parsing as TV show");

            // Try to extract show name from path first
            let show_name_from_path = self.extract_show_name_from_path(file_path);
            info!("Show name extracted from path: {:?}", show_name_from_path);

            if let Some(tv_info) = self.parse_tv_episode(filename) {
                info!("Successfully parsed TV episode pattern from filename");
                return Some(tv_info);
            }

            // If TV parsing fails but we're in TV folder, create a basic TV episode entry
            info!("TV pattern parsing failed, creating basic TV episode entry");
            let mut show_name =
                show_name_from_path.unwrap_or_else(|| self.clean_filename(filename));

            // Clean show name to remove year in parentheses
            let year_pattern = regex::Regex::new(r"\s*\(\d{4}\)\s*$").unwrap();
            if year_pattern.is_match(&show_name) {
                let cleaned = year_pattern.replace(&show_name, "").to_string();
                info!("Cleaned show name from '{}' to '{}'", show_name, cleaned);
                show_name = cleaned;
            }

            // Try to extract season from folder path
            let season = self.extract_season_from_path(file_path).unwrap_or(1);
            info!("Extracted season from path: {}", season);

            return Some(ParsedMediaInfo::Episode(ParsedEpisodeInfo {
                show_name,
                season,
                episode: self
                    .extract_episode_number_from_filename(filename)
                    .unwrap_or(1),
                episode_title: None,
                year: self.extract_year(filename),
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            }));
        }

        // If not in a specific folder, try to detect based on patterns
        // Try TV show pattern first (SxxExx format)
        if let Some(tv_info) = self.parse_tv_episode(filename) {
            return Some(tv_info);
        }

        // Default to movie
        self.parse_as_movie(filename, file_path)
    }

    /// Extract show name from folder path structure
    fn extract_show_name_from_path(&self, file_path: &Path) -> Option<String> {
        // Try to extract show name from path like /tvshows/Show Name/Season X/file.mkv
        let path_str = file_path.to_string_lossy();
        let path_lower = path_str.to_lowercase();

        info!("Extracting show name from path: {}", path_str);

        // Find the position of TV folder variations in the path (case-insensitive)
        let tv_folder_patterns = vec![
            ("/tvshows/", "\\tvshows\\"),
            ("/tv shows/", "\\tv shows\\"),
            ("/tv-shows/", "\\tv-shows\\"),
            ("/series/", "\\series\\"),
        ];

        for (unix_pattern, win_pattern) in tv_folder_patterns {
            if let Some(pos) = path_lower
                .find(unix_pattern)
                .or_else(|| path_lower.find(win_pattern))
            {
                // Get the actual case-sensitive path part after the TV folder
                let pattern_len = unix_pattern.len();
                let after_tv_folder = &path_str[pos + pattern_len..];

                // Get the first directory after tvshows - this should be the show name
                let parts: Vec<&str> = after_tv_folder.split(&['/', '\\'][..]).collect();
                if !parts.is_empty() && !parts[0].is_empty() {
                    let mut show_name = parts[0].to_string();

                    // Clean up show name - remove year in parentheses if present
                    // This is important for TMDB searches
                    if let Some(year_match) =
                        Regex::new(r"\s*\(\d{4}\)\s*$").unwrap().find(&show_name)
                    {
                        show_name = show_name[..year_match.start()].to_string();
                        info!("Removed year from show name for cleaner search");
                    }

                    info!("Extracted show name: {}", show_name);
                    return Some(show_name);
                }
            }
        }

        info!("Could not extract show name from path");
        None
    }

    /// Extract season number from folder path
    fn extract_season_from_path(&self, file_path: &Path) -> Option<u32> {
        let path_str = file_path.to_string_lossy();

        // Look for patterns like "Season 1", "Season 01", "S1", "S01" in the path
        let season_patterns = vec![
            Regex::new(r"[/\\][Ss]eason\s*(\d{1,2})[/\\]").unwrap(),
            Regex::new(r"[/\\][Ss](\d{1,2})[/\\]").unwrap(),
        ];

        for pattern in season_patterns {
            if let Some(captures) = pattern.captures(&path_str) {
                if let Some(season_str) = captures.get(1) {
                    if let Ok(season) = season_str.as_str().parse::<u32>() {
                        return Some(season);
                    }
                }
            }
        }

        None
    }

    /// Try to extract episode number from filename even without standard patterns
    fn extract_episode_number_from_filename(&self, filename: &str) -> Option<u32> {
        // Look for standalone numbers that might be episode numbers
        // E.g., "01.mkv", "episode_01.mkv", "01 - Title.mkv"
        let patterns = vec![
            Regex::new(r"^(\d{1,3})\.").unwrap(), // Starts with number
            Regex::new(r"[Ee]pisode[\s_-]*(\d{1,3})").unwrap(), // "Episode 01"
            Regex::new(r"[Ee]p[\s_-]*(\d{1,3})").unwrap(), // "Ep 01"
            Regex::new(r"[\s_-](\d{1,3})[\s_-]").unwrap(), // " 01 " or "_01_"
        ];

        for pattern in patterns {
            if let Some(captures) = pattern.captures(filename) {
                if let Some(ep_str) = captures.get(1) {
                    if let Ok(episode) = ep_str.as_str().parse::<u32>() {
                        if episode > 0 && episode < 1000 {
                            // Reasonable episode range
                            return Some(episode);
                        }
                    }
                }
            }
        }

        None
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

    /// Parse TV episode filename
    fn parse_tv_episode(&self, filename: &str) -> Option<ParsedMediaInfo> {
        // Try multiple TV patterns

        // Pattern 1: Show.Name.S01E01.Episode.Title.Quality.Info-Group
        let tv_regex1 =
            Regex::new(r"^(.+?)\.S(\d{1,2})E(\d{1,3})\.(.+?)\.(\d{3,4}p)\.(.+?)-(\w+)$").ok()?;

        if let Some(captures) = tv_regex1.captures(filename) {
            let show_name = captures.get(1)?.as_str().replace('.', " ");
            let season: u32 = captures.get(2)?.as_str().parse().ok()?;
            let episode: u32 = captures.get(3)?.as_str().parse().ok()?;
            let episode_title = captures.get(4)?.as_str().replace('.', " ");
            let resolution = captures.get(5)?.as_str().to_string();
            let quality_info = captures.get(6)?.as_str();
            let release_group = captures.get(7)?.as_str().to_string();

            debug!(
                "Parsed TV episode (pattern 1): {} S{}E{} - {}",
                show_name, season, episode, episode_title
            );

            return Some(ParsedMediaInfo::Episode(ParsedEpisodeInfo {
                show_name: {
                    // Remove year in parentheses
                    let year_pattern = regex::Regex::new(r"\s*\(\d{4}\)\s*$").unwrap();
                    if year_pattern.is_match(&show_name) {
                        year_pattern.replace(&show_name, "").to_string()
                    } else {
                        show_name
                    }
                },
                season,
                episode,
                episode_title: Some(episode_title),
                year: None,
                resolution: Some(resolution),
                source: self.extract_source(quality_info),
                release_group: Some(release_group),
            }));
        }

        // Pattern 2: Show Name S01E01 or Show.Name.S01E01 (more flexible)
        let tv_regex2 =
            Regex::new(r"(?i)^(.+?)[\s\.]S(\d{1,2})E(\d{1,3})(?:[\s\.\-](.+))?$").ok()?;

        if let Some(captures) = tv_regex2.captures(filename) {
            let show_name = captures
                .get(1)?
                .as_str()
                .replace(['.', '_'], " ")
                .trim()
                .to_string();
            let season: u32 = captures.get(2)?.as_str().parse().ok()?;
            let episode: u32 = captures.get(3)?.as_str().parse().ok()?;

            // Extract episode title - don't clean it if it's already readable
            let remainder = captures.get(4).map(|m| m.as_str()).unwrap_or("");
            let episode_title = if remainder.contains('.') || remainder.contains('_') {
                self.clean_filename(remainder)
            } else {
                remainder.trim().to_string()
            };

            debug!(
                "Parsed TV episode (pattern 2): {} S{}E{}",
                show_name, season, episode
            );

            return Some(ParsedMediaInfo::Episode(ParsedEpisodeInfo {
                show_name: {
                    let cleaned = self.clean_filename(&show_name);
                    // Remove year in parentheses
                    let year_pattern = regex::Regex::new(r"\s*\(\d{4}\)\s*$").unwrap();
                    if year_pattern.is_match(&cleaned) {
                        year_pattern.replace(&cleaned, "").to_string()
                    } else {
                        cleaned
                    }
                },
                season,
                episode,
                episode_title: if episode_title.is_empty() {
                    None
                } else {
                    Some(episode_title)
                },
                year: self.extract_year(&show_name),
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            }));
        }

        // Pattern 3: 1x01 format (e.g., "Show Name - 1x01 - Episode Title")
        let tv_regex3 =
            Regex::new(r"(?i)^(.+?)\s*-?\s*(\d{1,2})x(\d{1,3})(?:\s*-\s*(.+))?$").ok()?;

        if let Some(captures) = tv_regex3.captures(filename) {
            let show_name = captures.get(1)?.as_str().trim().to_string();
            let season: u32 = captures.get(2)?.as_str().parse().ok()?;
            let episode: u32 = captures.get(3)?.as_str().parse().ok()?;
            let episode_title = captures
                .get(4)
                .map(|m| self.clean_filename(m.as_str()))
                .filter(|s| !s.is_empty());

            debug!(
                "Parsed TV episode (pattern 3): {} {}x{}",
                show_name, season, episode
            );

            return Some(ParsedMediaInfo::Episode(ParsedEpisodeInfo {
                show_name: {
                    let cleaned = self.clean_filename(&show_name);
                    // Remove year in parentheses
                    let year_pattern = regex::Regex::new(r"\s*\(\d{4}\)\s*$").unwrap();
                    if year_pattern.is_match(&cleaned) {
                        year_pattern.replace(&cleaned, "").to_string()
                    } else {
                        cleaned
                    }
                },
                season,
                episode,
                episode_title,
                year: self.extract_year(&show_name),
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            }));
        }

        None
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
    fn test_parse_tv_episode() {
        let parser = FilenameParser::new();

        // Test pattern 1
        let result = parser
            .parse_tv_episode("Breaking.Bad.S01E01.Pilot.1080p.BluRay-RARBG")
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
        let result = parser.parse_tv_episode("Breaking Bad S01E01").unwrap();
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
            .parse_as_movie(
                "The.Dark.Knight.2008.1080p.BluRay.x264-YIFY.mkv",
                Path::new(""),
            )
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
