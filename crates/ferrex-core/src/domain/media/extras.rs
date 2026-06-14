use crate::types::files::ExtraType;
use regex::Regex;
use std::path::Path;
use tracing::debug;

/// Extras parsing utilities for detecting and categorizing movie/TV show extras
#[derive(Debug, Default, Clone, Copy)]
pub struct ExtrasParser;

impl ExtrasParser {
    /// Known extras folder patterns
    pub fn extras_folder_patterns() -> Vec<(&'static str, Regex, ExtraType)> {
        vec![
            // Common folder names
            (
                "behind_the_scenes",
                Regex::new(r"(?i)^behind[_\s-]?the[_\s-]?scenes?$").unwrap(),
                ExtraType::BehindTheScenes,
            ),
            (
                "deleted_scenes",
                Regex::new(r"(?i)^deleted[_\s-]?scenes?$").unwrap(),
                ExtraType::DeletedScenes,
            ),
            (
                "featurettes",
                Regex::new(r"(?i)^featurettes?$").unwrap(),
                ExtraType::Featurette,
            ),
            (
                "interviews",
                Regex::new(r"(?i)^interviews?$").unwrap(),
                ExtraType::Interview,
            ),
            (
                "scenes",
                Regex::new(r"(?i)^scenes?$").unwrap(),
                ExtraType::Scene,
            ),
            (
                "shorts",
                Regex::new(r"(?i)^shorts?$").unwrap(),
                ExtraType::Short,
            ),
            (
                "trailers",
                Regex::new(r"(?i)^trailers?$").unwrap(),
                ExtraType::Trailer,
            ),
            (
                "extras",
                Regex::new(r"(?i)^extras?$").unwrap(),
                ExtraType::Other,
            ),
            (
                "special_features",
                Regex::new(r"(?i)^special[_\s-]?features?$").unwrap(),
                ExtraType::Other,
            ),
            // Plex/Jellyfin standard folder names
            (
                "behindthescenes",
                Regex::new(r"(?i)^behindthescenes$").unwrap(),
                ExtraType::BehindTheScenes,
            ),
            (
                "deletedscenes",
                Regex::new(r"(?i)^deletedscenes$").unwrap(),
                ExtraType::DeletedScenes,
            ),
            (
                "featurette",
                Regex::new(r"(?i)^featurette$").unwrap(),
                ExtraType::Featurette,
            ),
            (
                "interview",
                Regex::new(r"(?i)^interview$").unwrap(),
                ExtraType::Interview,
            ),
            (
                "scene",
                Regex::new(r"(?i)^scene$").unwrap(),
                ExtraType::Scene,
            ),
            (
                "short",
                Regex::new(r"(?i)^short$").unwrap(),
                ExtraType::Short,
            ),
            (
                "trailer",
                Regex::new(r"(?i)^trailer$").unwrap(),
                ExtraType::Trailer,
            ),
            (
                "other",
                Regex::new(r"(?i)^other$").unwrap(),
                ExtraType::Other,
            ),
        ]
    }

    /// Filename patterns for extras (when not in dedicated folders)
    pub fn extras_filename_patterns() -> Vec<(&'static str, Regex, ExtraType)> {
        vec![
            // Filename contains extra type
            (
                "bts_filename",
                Regex::new(r"(?i)(behind[_\s-]?the[_\s-]?scenes?|bts)")
                    .unwrap(),
                ExtraType::BehindTheScenes,
            ),
            (
                "deleted_filename",
                Regex::new(r"(?i)deleted[_\s-]?scenes?").unwrap(),
                ExtraType::DeletedScenes,
            ),
            (
                "featurette_filename",
                Regex::new(r"(?i)featurettes?").unwrap(),
                ExtraType::Featurette,
            ),
            (
                "interview_filename",
                Regex::new(r"(?i)interviews?").unwrap(),
                ExtraType::Interview,
            ),
            (
                "trailer_filename",
                Regex::new(r"(?i)trailers?").unwrap(),
                ExtraType::Trailer,
            ),
            (
                "making_of",
                Regex::new(r"(?i)making[_\s-]?of").unwrap(),
                ExtraType::BehindTheScenes,
            ),
            (
                "commentary",
                Regex::new(r"(?i)commentary").unwrap(),
                ExtraType::Other,
            ),
            (
                "gag_reel",
                Regex::new(r"(?i)(gag[_\s-]?reel|bloopers?)").unwrap(),
                ExtraType::Other,
            ),
            // Common suffixes/prefixes
            (
                "extra_suffix",
                Regex::new(r"(?i)[_\s-]extra[_\s-]").unwrap(),
                ExtraType::Other,
            ),
            (
                "bonus_suffix",
                Regex::new(r"(?i)[_\s-]bonus[_\s-]").unwrap(),
                ExtraType::Other,
            ),
            (
                "special_suffix",
                Regex::new(r"(?i)[_\s-]special[_\s-]").unwrap(),
                ExtraType::Other,
            ),
        ]
    }

    /// Determine if a path is within an extras folder structure
    pub fn is_in_extras_folder(path: &Path) -> Option<ExtraType> {
        // Check each parent directory for extras folder patterns
        let mut current_path = path.parent();

        while let Some(parent) = current_path {
            if let Some(folder_name) = parent.file_name()
                && let Some(folder_str) = folder_name.to_str()
            {
                // Check against all extras folder patterns
                for (_name, pattern, extra_type) in
                    Self::extras_folder_patterns()
                {
                    if pattern.is_match(folder_str) {
                        debug!(
                            "Detected extras folder '{}' as {:?}",
                            folder_str, extra_type
                        );
                        return Some(extra_type);
                    }
                }
            }
            current_path = parent.parent();
        }

        None
    }

    /// Extract extra type from filename patterns
    pub fn extract_extra_type_from_filename(path: &Path) -> Option<ExtraType> {
        let filename = path.file_stem()?.to_str()?;

        // Check filename patterns
        for (_name, pattern, extra_type) in Self::extras_filename_patterns() {
            if pattern.is_match(filename) {
                debug!(
                    "Detected extra type {:?} from filename: {}",
                    extra_type, filename
                );
                return Some(extra_type);
            }
        }

        None
    }

    /// Determine if a file is an extra and what type
    pub fn parse_extra_info(path: &Path) -> Option<ExtraType> {
        // First check if we're in an extras folder
        if let Some(folder_extra_type) = Self::is_in_extras_folder(path) {
            return Some(folder_extra_type);
        }

        // Then check filename patterns
        Self::extract_extra_type_from_filename(path)
    }

    /// Check if a file is an extra
    pub fn is_extra(path: &Path) -> bool {
        Self::parse_extra_info(path).is_some()
    }

    /// Extract the parent media title for an extra
    /// Looks for the movie/show folder that contains the extras
    pub fn extract_parent_title(path: &Path) -> Option<String> {
        // First check if this is a filename-based extra and extract from filename
        // E.g., "The Matrix - Behind the Scenes.mkv" -> "The Matrix"
        if let Some(filename) = path.file_stem()
            && let Some(name_str) = filename.to_str()
        {
            // Look for patterns like "MovieTitle - ExtraType"
            for (_name, pattern, _extra_type) in
                Self::extras_filename_patterns()
            {
                if pattern.is_match(name_str) {
                    // Try to extract the title before the extra type
                    if let Some(dash_pos) = name_str.find(" - ") {
                        let potential_title = &name_str[..dash_pos];
                        // Check if the part after dash contains extra keywords
                        let after_dash = &name_str[dash_pos + 3..];
                        if pattern.is_match(after_dash) {
                            debug!(
                                "Extracted parent title '{}' from filename for extra: {}",
                                potential_title,
                                path.display()
                            );
                            return Some(potential_title.to_string());
                        }
                    }
                    break;
                }
            }
        }

        // If we're in an extras folder, the parent should be the movie/show folder
        let mut current_path = path.parent();

        while let Some(parent) = current_path {
            if let Some(folder_name) = parent.file_name()
                && let Some(folder_str) = folder_name.to_str()
            {
                // Check if this folder is an extras folder
                let is_extras_folder = Self::extras_folder_patterns()
                    .iter()
                    .any(|(_, pattern, _)| pattern.is_match(folder_str));

                if is_extras_folder {
                    // Look at the parent of the extras folder
                    if let Some(grandparent) = parent.parent()
                        && let Some(show_folder) = grandparent.file_name()
                    {
                        let title = show_folder.to_str()?.to_string();
                        debug!(
                            "Extracted parent title '{}' for extra: {}",
                            title,
                            path.display()
                        );
                        return Some(title);
                    }
                }
            }
            current_path = parent.parent();
        }

        // If not in a dedicated extras folder, try to infer from path structure
        // For files like "/Movies/The Matrix (1999)/The Matrix - Behind the Scenes.mkv"
        if let Some(parent) = path.parent()
            && let Some(movie_folder) = parent.file_name()
            && let Some(title) = movie_folder.to_str()
        {
            // Skip common extras folder names
            let is_extras_folder = Self::extras_folder_patterns()
                .iter()
                .any(|(_, pattern, _)| pattern.is_match(title));

            if !is_extras_folder {
                return Some(title.to_string());
            }
        }

        None
    }

    /// Check if a path structure suggests it contains extras
    /// Useful for scanning to know to look deeper
    pub fn path_likely_contains_extras(path: &Path) -> bool {
        // Check if any part of the path contains extras indicators
        for component in path.components() {
            if let Some(name) = component.as_os_str().to_str() {
                for (_pattern_name, pattern, _extra_type) in
                    Self::extras_folder_patterns()
                {
                    if pattern.is_match(name) {
                        return true;
                    }
                }

                // Also check for common extra indicators in filenames
                let lower = name.to_lowercase();
                if lower.contains("extra")
                    || lower.contains("bonus")
                    || lower.contains("special")
                    || lower.contains("behind")
                    || lower.contains("deleted")
                    || lower.contains("trailer")
                {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::library::LibraryType;
    use ferrex_model::ImageMediaType;
    use std::path::PathBuf;

    #[test]
    fn test_extras_folder_detection() {
        let test_cases = vec![
            (
                "/movies/The Matrix/Behind the Scenes/making_of.mkv",
                Some(ExtraType::BehindTheScenes),
            ),
            (
                "/movies/The Matrix/Deleted Scenes/cut_scene.mkv",
                Some(ExtraType::DeletedScenes),
            ),
            (
                "/movies/The Matrix/Featurettes/cast_interview.mkv",
                Some(ExtraType::Featurette),
            ),
            (
                "/movies/The Matrix/Trailers/theatrical_trailer.mkv",
                Some(ExtraType::Trailer),
            ),
            (
                "/movies/The Matrix/Extras/commentary.mkv",
                Some(ExtraType::Other),
            ),
            ("/movies/The Matrix/The Matrix (1999).mkv", None),
        ];

        for (path, expected) in test_cases {
            let path = PathBuf::from(path);
            let result = ExtrasParser::is_in_extras_folder(&path);
            assert_eq!(result, expected, "Failed for path: {}", path.display());
        }
    }

    #[test]
    fn test_filename_extra_detection() {
        let test_cases = vec![
            (
                "/movies/The Matrix - Behind the Scenes.mkv",
                Some(ExtraType::BehindTheScenes),
            ),
            (
                "/movies/The Matrix - Deleted Scenes.mkv",
                Some(ExtraType::DeletedScenes),
            ),
            (
                "/movies/The Matrix - Featurette.mkv",
                Some(ExtraType::Featurette),
            ),
            (
                "/movies/The Matrix - Interview.mkv",
                Some(ExtraType::Interview),
            ),
            ("/movies/The Matrix - Trailer.mkv", Some(ExtraType::Trailer)),
            (
                "/movies/The Matrix - Making of.mkv",
                Some(ExtraType::BehindTheScenes),
            ),
            ("/movies/The Matrix (1999).mkv", None),
        ];

        for (path, expected) in test_cases {
            let path = PathBuf::from(path);
            let result = ExtrasParser::extract_extra_type_from_filename(&path);
            assert_eq!(result, expected, "Failed for path: {}", path.display());
        }
    }

    #[test]
    fn test_parent_title_extraction() {
        let test_cases = vec![
            (
                "/movies/The Matrix (1999)/Behind the Scenes/making_of.mkv",
                Some("The Matrix (1999)".to_string()),
            ),
            (
                "/tv/Breaking Bad/Season 1/Deleted Scenes/pilot_cut.mkv",
                Some("Season 1".to_string()),
            ),
            (
                "/movies/Avatar/Extras/commentary.mkv",
                Some("Avatar".to_string()),
            ),
        ];

        for (path, expected) in test_cases {
            let path = PathBuf::from(path);
            let result = ExtrasParser::extract_parent_title(&path);
            assert_eq!(result, expected, "Failed for path: {}", path.display());
        }
    }

    #[test]
    fn test_media_type_determination() {
        let _movie_lib = LibraryType::Movies;

        let test_cases =
            vec![("/movies/The Matrix (1999).mkv", ImageMediaType::Movie)];

        for (path, _expected) in test_cases {
            let _path = PathBuf::from(path);
        }
    }

    #[test]
    fn test_plex_jellyfin_folder_names() {
        let test_cases = vec![
            (
                "/movies/The Matrix/behindthescenes/making_of.mkv",
                Some(ExtraType::BehindTheScenes),
            ),
            (
                "/movies/The Matrix/deletedscenes/cut_scene.mkv",
                Some(ExtraType::DeletedScenes),
            ),
            (
                "/movies/The Matrix/featurette/cast_talk.mkv",
                Some(ExtraType::Featurette),
            ),
            (
                "/movies/The Matrix/interview/director.mkv",
                Some(ExtraType::Interview),
            ),
            (
                "/movies/The Matrix/trailer/teaser.mkv",
                Some(ExtraType::Trailer),
            ),
        ];

        for (path, expected) in test_cases {
            let path = PathBuf::from(path);
            let result = ExtrasParser::is_in_extras_folder(&path);
            assert_eq!(result, expected, "Failed for path: {}", path.display());
        }
    }
}
