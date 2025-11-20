use crate::MediaType;
use regex::Regex;
use std::path::Path;
use tracing::debug;
use chrono::NaiveDate;

/// TV show parsing utilities for Jellyfin-compatible patterns
pub struct TvParser;

/// Represents parsed episode information
#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeInfo {
    pub season: u32,
    pub episode: u32,
    pub end_episode: Option<u32>, // For multi-episode files
    pub year: Option<u32>, // For date-based episodes
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub absolute_episode: Option<u32>, // For anime
    pub is_special: bool, // Season 0 episodes
}

impl TvParser {
    /// Episode naming patterns in order of preference
    /// Based on Jellyfin's episode parsing logic
    pub fn episode_patterns() -> Vec<(&'static str, Regex)> {
        vec![
            // Multi-episode patterns (must check before single episode)
            ("multi_episode_dash", Regex::new(r"[Ss](\d+)[Ee](\d+)(?:-[Ee]?(\d+))").unwrap()),
            ("multi_episode_concat", Regex::new(r"[Ss](\d+)[Ee](\d+)[Ee](\d+)").unwrap()),
            ("multi_episode_x", Regex::new(r"(\d+)[xX](\d+)(?:-[xX]?(\d+))").unwrap()),
            
            // Standard patterns
            ("s00e00", Regex::new(r"[Ss](\d+)[Ee](\d+)").unwrap()),
            ("0x00", Regex::new(r"(\d+)[xX](\d+)").unwrap()),
            ("season_episode", Regex::new(r"(?i)season\s*(\d+)\s*episode\s*(\d+)").unwrap()),
            ("s00_e00", Regex::new(r"[Ss](\d+)\s+[Ee](\d+)").unwrap()),
            ("ep000", Regex::new(r"(?i)(?:ep|episode)\s*(\d)(\d{2})").unwrap()),
            ("000", Regex::new(r"^(\d)(\d{2})(?:\D|$)").unwrap()),
            
            // Absolute episode number (common in anime)
            ("absolute", Regex::new(r"(?:^|\D)(\d{2,4})(?:\D|$)").unwrap()),
        ]
    }
    
    /// Date-based episode patterns
    pub fn date_patterns() -> Vec<(&'static str, Regex)> {
        vec![
            // YYYY-MM-DD or YYYY.MM.DD
            ("date_ymd", Regex::new(r"(\d{4})[\-\.](\d{1,2})[\-\.](\d{1,2})").unwrap()),
            // DD-MM-YYYY or DD.MM.YYYY
            ("date_dmy", Regex::new(r"(\d{1,2})[\-\.](\d{1,2})[\-\.](\d{4})").unwrap()),
            // YYYYMMDD
            ("date_compact", Regex::new(r"(\d{4})(\d{2})(\d{2})").unwrap()),
        ]
    }

    /// Season folder patterns
    pub fn season_folder_patterns() -> Vec<Regex> {
        vec![
            // Season 01, Season 1
            Regex::new(r"(?i)^season\s*(\d+)$").unwrap(),
            // S01, S1, S02, etc.
            Regex::new(r"(?i)^s(\d{1,2})$").unwrap(),
            // Season01, Season1
            Regex::new(r"(?i)^season(\d+)$").unwrap(),
            // Specials
            Regex::new(r"(?i)^specials?$").unwrap(),
            // Series 1 (British convention)
            Regex::new(r"(?i)^series\s*(\d+)$").unwrap(),
        ]
    }

    /// Extract detailed episode information from a file path
    pub fn parse_episode_info(path: &Path) -> Option<EpisodeInfo> {
        let filename = path.file_stem()?.to_str()?;
        
        // First, try date-based patterns (common for daily shows)
        for (pattern_name, pattern) in Self::date_patterns() {
            if let Some(captures) = pattern.captures(filename) {
                let (year, month, day) = match pattern_name {
                    "date_ymd" | "date_compact" => (
                        captures[1].parse().ok()?,
                        captures[2].parse().ok()?,
                        captures[3].parse().ok()?,
                    ),
                    "date_dmy" => (
                        captures[3].parse().ok()?,
                        captures[2].parse().ok()?,
                        captures[1].parse().ok()?,
                    ),
                    _ => continue,
                };
                
                // Validate date
                if NaiveDate::from_ymd_opt(year as i32, month, day).is_some() {
                    debug!("Parsed date-based episode: {}-{:02}-{:02} from {}", year, month, day, filename);
                    return Some(EpisodeInfo {
                        season: year,
                        episode: (month * 100 + day), // Encode as MMDD
                        end_episode: None,
                        year: Some(year),
                        month: Some(month),
                        day: Some(day),
                        absolute_episode: None,
                        is_special: false,
                    });
                }
            }
        }
        
        // Try standard episode patterns
        for (pattern_name, pattern) in Self::episode_patterns() {
            if let Some(captures) = pattern.captures(filename) {
                match pattern_name {
                    "multi_episode_dash" | "multi_episode_x" => {
                        if captures.len() >= 4 {
                            let season: u32 = captures[1].parse().ok()?;
                            let start_ep: u32 = captures[2].parse().ok()?;
                            let end_ep: u32 = captures[3].parse().ok()?;
                            debug!("Parsed multi-episode: S{:02}E{:02}-E{:02} from {}", season, start_ep, end_ep, filename);
                            return Some(EpisodeInfo {
                                season,
                                episode: start_ep,
                                end_episode: Some(end_ep),
                                year: None,
                                month: None,
                                day: None,
                                absolute_episode: None,
                                is_special: season == 0,
                            });
                        }
                    }
                    "multi_episode_concat" => {
                        if captures.len() >= 4 {
                            let season: u32 = captures[1].parse().ok()?;
                            let start_ep: u32 = captures[2].parse().ok()?;
                            let end_ep: u32 = captures[3].parse().ok()?;
                            debug!("Parsed concat episodes: S{:02}E{:02}E{:02} from {}", season, start_ep, end_ep, filename);
                            return Some(EpisodeInfo {
                                season,
                                episode: start_ep,
                                end_episode: Some(end_ep),
                                year: None,
                                month: None,
                                day: None,
                                absolute_episode: None,
                                is_special: season == 0,
                            });
                        }
                    }
                    "absolute" => {
                        // Only use absolute numbering if no other pattern matched
                        // and we're in an anime-like structure
                        if Self::is_likely_anime(path) {
                            if let Ok(abs_ep) = captures[1].parse::<u32>() {
                                if abs_ep > 0 && abs_ep < 10000 {
                                    debug!("Parsed absolute episode: {} from {}", abs_ep, filename);
                                    return Some(EpisodeInfo {
                                        season: 1,
                                        episode: abs_ep,
                                        end_episode: None,
                                        year: None,
                                        month: None,
                                        day: None,
                                        absolute_episode: Some(abs_ep),
                                        is_special: false,
                                    });
                                }
                            }
                        }
                    }
                    _ => {
                        if captures.len() >= 3 {
                            let season: u32 = captures[1].parse().ok()?;
                            let episode: u32 = captures[2].parse().ok()?;
                            debug!("Parsed episode: S{:02}E{:02} from {}", season, episode, filename);
                            return Some(EpisodeInfo {
                                season,
                                episode,
                                end_episode: None,
                                year: None,
                                month: None,
                                day: None,
                                absolute_episode: None,
                                is_special: season == 0,
                            });
                        }
                    }
                }
            }
        }
        
        // Fallback to folder-based parsing
        Self::parse_from_folder_structure(path)
    }
    
    /// Extract season and episode numbers from a file path (simplified version)
    pub fn parse_episode(path: &Path) -> Option<(u32, u32)> {
        Self::parse_episode_info(path).map(|info| (info.season, info.episode))
    }
    
    /// Parse episode info from folder structure
    fn parse_from_folder_structure(path: &Path) -> Option<EpisodeInfo> {

        let filename = path.file_stem()?.to_str()?;
        
        if let Some(parent) = path.parent() {
            if let Some(parent_name) = parent.file_name() {
                if let Some(season) = Self::parse_season_folder(parent_name.to_str()?) {
                    // Look for just episode number in filename
                    let ep_patterns = vec![
                        Regex::new(r"(?i)(?:e|ep|episode)\s*(\d+)").unwrap(),
                        Regex::new(r"^\s*(\d+)\s*[-_.]").unwrap(),
                        Regex::new(r"^(\d{1,3})\s").unwrap(), // "01 Title"
                        Regex::new(r"^(\d{1,3})$").unwrap(), // Just "01" or "05"
                    ];
                    
                    for pattern in ep_patterns {
                        if let Some(captures) = pattern.captures(filename) {
                            if let Ok(episode) = captures[1].parse::<u32>() {
                                debug!("Parsed episode: S{:02}E{:02} from folder+filename", season, episode);
                                return Some(EpisodeInfo {
                                    season,
                                    episode,
                                    end_episode: None,
                                    year: None,
                                    month: None,
                                    day: None,
                                    absolute_episode: None,
                                    is_special: season == 0,
                                });
                            }
                        }
                    }
                }
            }
        }
        
        None
    }

    /// Parse season number from folder name
    pub fn parse_season_folder(folder_name: &str) -> Option<u32> {
        // Check for specials folder
        if folder_name.to_lowercase() == "specials" || folder_name.to_lowercase() == "special" {
            return Some(0);
        }
        
        for pattern in Self::season_folder_patterns() {
            if let Some(captures) = pattern.captures(folder_name) {
                if captures.len() >= 2 {
                    if let Ok(season) = captures[1].parse::<u32>() {
                        return Some(season);
                    }
                }
            }
        }
        None
    }

    /// Extract series name from path
    /// Looks for the show folder (parent of season folder or grandparent of episode file)
    pub fn extract_series_name(path: &Path) -> Option<String> {
        // First, check if this is in a season folder
        if let Some(parent) = path.parent() {
            if let Some(parent_name) = parent.file_name() {
                if let Some(parent_str) = parent_name.to_str() {
                    // If parent is a season folder, get grandparent
                    if Self::parse_season_folder(parent_str).is_some() {
                        if let Some(grandparent) = parent.parent() {
                            if let Some(show_name) = grandparent.file_name() {
                                return show_name.to_str().map(|s| s.to_string());
                            }
                        }
                    } else {
                        // Parent might be the show folder directly
                        return Some(parent_str.to_string());
                    }
                }
            }
        }
        None
    }

    /// Determine media type based on path structure
    pub fn determine_media_type(path: &Path, library_type: Option<&crate::LibraryType>) -> MediaType {
        // If library type is specified, use it as a strong hint
        match library_type {
            Some(crate::LibraryType::TvShows) => {
                // In a TV library, default to TV episode
                // Even if we can't parse episode info, it's still TV content
                return MediaType::TvEpisode;
            }
            Some(crate::LibraryType::Movies) => {
                // In a movie library, ALWAYS return Movie type
                // Library type should be authoritative
                return MediaType::Movie;
            }
            None => {
                // No library context, use heuristics
                if Self::parse_episode(path).is_some() {
                    return MediaType::TvEpisode;
                }
                return MediaType::Movie;
            }
        }
    }

    /// Check if a path is within a TV show folder structure
    pub fn is_in_tv_structure(path: &Path) -> bool {
        // Check parent for season folder
        if let Some(parent) = path.parent() {
            if let Some(parent_name) = parent.file_name() {
                if let Some(parent_str) = parent_name.to_str() {
                    if Self::parse_season_folder(parent_str).is_some() {
                        return true;
                    }
                }
            }

            // Check grandparent path
            if let Some(grandparent) = parent.parent() {
                if let Some(grandparent_name) = grandparent.file_name() {
                    // If grandparent exists and parent is a season folder
                    if let Some(parent_name) = parent.file_name() {
                        if let Some(parent_str) = parent_name.to_str() {
                            if Self::parse_season_folder(parent_str).is_some() {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        // Check filename for episode patterns
        if let Some(filename) = path.file_stem() {
            if let Some(name_str) = filename.to_str() {
                // Quick check for common episode indicators
                let lower = name_str.to_lowercase();
                if lower.contains("s0") || lower.contains("s1") || 
                   lower.contains("e0") || lower.contains("e1") ||
                   lower.contains("x0") || lower.contains("x1") {
                    return true;
                }
            }
        }

        false
    }
    
    /// Check if path likely contains anime based on common patterns
    pub fn is_likely_anime(path: &Path) -> bool {
        let path_str = path.to_string_lossy().to_lowercase();
        
        // Common anime folder indicators
        let anime_indicators = vec![
            "anime", "[subbed]", "[dubbed]", "[bd]", "[dvd]",
            "[720p]", "[1080p]", "[hevc]", "[x264]", "[x265]",
        ];
        
        for indicator in anime_indicators {
            if path_str.contains(indicator) {
                return true;
            }
        }
        
        // Check for fansub group tags [GroupName]
        if let Some(filename) = path.file_stem() {
            if let Some(name) = filename.to_str() {
                if name.starts_with('[') && name.contains(']') {
                    return true;
                }
            }
        }
        
        false
    }
    
    /// Extract episode title from filename
    pub fn extract_episode_title(path: &Path) -> Option<String> {
        let filename = path.file_stem()?.to_str()?;
        
        // Remove common patterns to get to the title
        let title_patterns = vec![
            // S01E01 - Title
            Regex::new(r"^.*?[Ss]\d+[Ee]\d+(?:[Ee]\d+)?\s*-\s*(.+)$").unwrap(),
            // 1x01 - Title  
            Regex::new(r"^.*?\d+[xX]\d+\s*-\s*(.+)$").unwrap(),
            // 01 - Title
            Regex::new(r"^\d+\s*-\s*(.+)$").unwrap(),
            // S01E01.Title format (dot separator)
            Regex::new(r"^.*?[Ss]\d+[Ee]\d+\.(.+)$").unwrap(),
        ];
        
        for pattern in title_patterns {
            if let Some(captures) = pattern.captures(filename) {
                if let Some(title) = captures.get(1) {
                    let cleaned = title.as_str()
                        .trim()
                        .trim_end_matches(|c: char| c == '.' || c == '_' || c == '-')
                        .replace('_', " ")
                        .replace('.', " ");
                    
                    if !cleaned.is_empty() {
                        return Some(cleaned);
                    }
                }
            }
        }
        
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_episode_s01e01() {
        let path = PathBuf::from("/media/Show Name/Season 1/S01E01 - Pilot.mkv");
        assert_eq!(TvParser::parse_episode(&path), Some((1, 1)));
    }

    #[test]
    fn test_parse_episode_1x01() {
        let path = PathBuf::from("/media/Show Name/1x01 - Pilot.mkv");
        assert_eq!(TvParser::parse_episode(&path), Some((1, 1)));
    }

    #[test]
    fn test_parse_episode_from_folder() {
        let path = PathBuf::from("/media/Show Name/Season 2/03 - Episode Name.mkv");
        assert_eq!(TvParser::parse_episode(&path), Some((2, 3)));
    }
    
    #[test]
    fn test_parse_multi_episode() {
        let path1 = PathBuf::from("/media/Show/S01E01-E02.mkv");
        let info1 = TvParser::parse_episode_info(&path1).unwrap();
        assert_eq!(info1.season, 1);
        assert_eq!(info1.episode, 1);
        assert_eq!(info1.end_episode, Some(2));
        
        let path2 = PathBuf::from("/media/Show/S01E01E02.mkv");
        let info2 = TvParser::parse_episode_info(&path2).unwrap();
        assert_eq!(info2.season, 1);
        assert_eq!(info2.episode, 1);
        assert_eq!(info2.end_episode, Some(2));
    }
    
    #[test]
    fn test_parse_date_episode() {
        let path1 = PathBuf::from("/media/Daily Show/2024-01-15.mkv");
        let info1 = TvParser::parse_episode_info(&path1).unwrap();
        assert_eq!(info1.year, Some(2024));
        assert_eq!(info1.month, Some(1));
        assert_eq!(info1.day, Some(15));
        
        let path2 = PathBuf::from("/media/Daily Show/2024.01.15.mkv");
        let info2 = TvParser::parse_episode_info(&path2).unwrap();
        assert_eq!(info2.year, Some(2024));
        assert_eq!(info2.month, Some(1));
        assert_eq!(info2.day, Some(15));
    }
    
    #[test]
    fn test_parse_special_episode() {
        let path = PathBuf::from("/media/Show/Specials/S00E01 - Christmas Special.mkv");
        let info = TvParser::parse_episode_info(&path).unwrap();
        assert_eq!(info.season, 0);
        assert_eq!(info.episode, 1);
        assert_eq!(info.is_special, true);
    }

    #[test]
    fn test_extract_series_name() {
        let path = PathBuf::from("/media/Breaking Bad/Season 1/S01E01.mkv");
        assert_eq!(TvParser::extract_series_name(&path), Some("Breaking Bad".to_string()));
    }

    #[test]
    fn test_parse_season_folder() {
        assert_eq!(TvParser::parse_season_folder("Season 1"), Some(1));
        assert_eq!(TvParser::parse_season_folder("S01"), Some(1));
        assert_eq!(TvParser::parse_season_folder("season01"), Some(1));
        assert_eq!(TvParser::parse_season_folder("Specials"), Some(0));
        assert_eq!(TvParser::parse_season_folder("Series 2"), Some(2));
        assert_eq!(TvParser::parse_season_folder("Random Folder"), None);
    }

    #[test]
    fn test_is_in_tv_structure() {
        let tv_path = PathBuf::from("/media/Shows/Breaking Bad/Season 1/S01E01.mkv");
        assert!(TvParser::is_in_tv_structure(&tv_path));

        let movie_path = PathBuf::from("/media/Movies/The Matrix (1999).mkv");
        assert!(!TvParser::is_in_tv_structure(&movie_path));
    }
    
    #[test]
    fn test_extract_episode_title() {
        let path1 = PathBuf::from("/media/Show/S01E01 - Pilot Episode.mkv");
        assert_eq!(TvParser::extract_episode_title(&path1), Some("Pilot Episode".to_string()));
        
        let path2 = PathBuf::from("/media/Show/1x01 - The Beginning.mkv");
        assert_eq!(TvParser::extract_episode_title(&path2), Some("The Beginning".to_string()));
        
        let path3 = PathBuf::from("/media/Show/01 - First Episode.mkv");
        assert_eq!(TvParser::extract_episode_title(&path3), Some("First Episode".to_string()));
    }
    
    #[test]
    fn test_is_likely_anime() {
        let anime_path1 = PathBuf::from("/media/Anime/Attack on Titan/[HorribleSubs] AOT - 01 [720p].mkv");
        assert!(TvParser::is_likely_anime(&anime_path1));
        
        let anime_path2 = PathBuf::from("/media/Shows/Naruto [Dubbed]/Episode 001.mkv");
        assert!(TvParser::is_likely_anime(&anime_path2));
        
        let regular_path = PathBuf::from("/media/Shows/Breaking Bad/S01E01.mkv");
        assert!(!TvParser::is_likely_anime(&regular_path));
    }
}