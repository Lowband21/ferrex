//! Input validation for configuration values

/// Validates that a media root path looks like a valid filesystem path.
/// Returns an error message if the path appears invalid.
pub fn validate_media_root(path: &str) -> Result<(), String> {
    let trimmed = path.trim();

    // Empty is allowed (optional field)
    if trimmed.is_empty() {
        return Ok(());
    }

    // Check if it looks like a path (starts with /, ./, ~/, or is an absolute Windows path)
    let looks_like_path = trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("~/")
        || trimmed.starts_with("../")
        || (trimmed.len() >= 3
            && trimmed.chars().nth(1) == Some(':')
            && (trimmed.chars().nth(2) == Some('\\')
                || trimmed.chars().nth(2) == Some('/')));

    if !looks_like_path {
        return Err(format!(
            "MEDIA_ROOT must be a valid path starting with '/', './', '~/', or '../'. Got: '{}'",
            trimmed
        ));
    }

    Ok(())
}

/// Validates that a TMDB API key looks valid.
/// Returns an error message if the key appears invalid.
pub fn validate_tmdb_api_key(key: &str) -> Result<(), String> {
    let trimmed = key.trim();

    // Empty is allowed (optional - disables TMDB metadata)
    if trimmed.is_empty() {
        return Ok(());
    }

    // TMDB API keys are typically 32 characters, alphanumeric
    // Accept between 20-64 chars to allow for format changes
    let len = trimmed.len();
    if len < 20 || len > 64 {
        return Err(format!(
            "TMDB_API_KEY should be between 20-64 characters (got {})",
            len
        ));
    }

    // Should be alphanumeric
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(
            "TMDB_API_KEY should contain only letters and numbers".to_string()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_media_root_valid_paths() {
        assert!(validate_media_root("/mnt/media").is_ok());
        assert!(validate_media_root("/home/user/videos").is_ok());
        assert!(validate_media_root("./media").is_ok());
        assert!(validate_media_root("~/media").is_ok());
        assert!(validate_media_root("../media").is_ok());
        assert!(validate_media_root("C:\\Users\\Media").is_ok());
        assert!(validate_media_root("").is_ok()); // Empty is ok
    }

    #[test]
    fn test_validate_media_root_invalid() {
        // Relative paths without prefix are invalid
        assert!(validate_media_root("media").is_err());
        assert!(validate_media_root("not-a-path").is_err());
        assert!(validate_media_root("some/path").is_err());
    }

    #[test]
    fn test_validate_tmdb_api_key_valid() {
        assert!(
            validate_tmdb_api_key("3cb2d7e5faad13c2ae258607483d2de1").is_ok()
        );
        assert!(
            validate_tmdb_api_key("abcdef1234567890abcdef1234567890").is_ok()
        );
        assert!(validate_tmdb_api_key("12345678901234567890").is_ok()); // 20 chars
        assert!(validate_tmdb_api_key("").is_ok()); // Empty is ok
    }

    #[test]
    fn test_validate_tmdb_api_key_invalid() {
        assert!(validate_tmdb_api_key("tooshort").is_err()); // < 20 chars
        assert!(
            validate_tmdb_api_key("has-dashes-not-allowed-123456789012")
                .is_err()
        );
        assert!(
            validate_tmdb_api_key("has/slashes/1234567890123456789012")
                .is_err()
        );
    }
}
