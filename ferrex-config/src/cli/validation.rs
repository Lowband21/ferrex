//! Input validation for configuration values

use std::collections::HashMap;

use url::Url;

/// Error type for credential validation issues.
#[derive(Debug, Clone)]
pub struct CredentialError {
    pub message: String,
    pub hint: String,
}

/// Extract the password component from a PostgreSQL URL.
/// Returns `None` if the URL is invalid or has no password.
pub fn extract_password_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    parsed.password().map(|p| p.to_string())
}

/// Validate that DATABASE_URL passwords match the standalone password values.
/// Returns a list of errors if inconsistencies are found.
pub fn validate_credential_consistency(
    kv: &[(String, String)],
) -> Vec<CredentialError> {
    let map: HashMap<&str, &str> =
        kv.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    let mut errors = Vec::new();

    // Check DATABASE_URL vs DATABASE_APP_PASSWORD
    if let (Some(url), Some(pw)) =
        (map.get("DATABASE_URL"), map.get("DATABASE_APP_PASSWORD"))
    {
        if let Err(e) = validate_url_password_match(
            "DATABASE_URL",
            url,
            "DATABASE_APP_PASSWORD",
            pw,
        ) {
            errors.push(e);
        }
    }

    // Check DATABASE_URL_ADMIN vs DATABASE_ADMIN_PASSWORD
    if let (Some(url), Some(pw)) = (
        map.get("DATABASE_URL_ADMIN"),
        map.get("DATABASE_ADMIN_PASSWORD"),
    ) {
        if let Err(e) = validate_url_password_match(
            "DATABASE_URL_ADMIN",
            url,
            "DATABASE_ADMIN_PASSWORD",
            pw,
        ) {
            errors.push(e);
        }
    }

    // Check DATABASE_URL_CONTAINER vs DATABASE_APP_PASSWORD
    if let (Some(url), Some(pw)) = (
        map.get("DATABASE_URL_CONTAINER"),
        map.get("DATABASE_APP_PASSWORD"),
    ) {
        if let Err(e) = validate_url_password_match(
            "DATABASE_URL_CONTAINER",
            url,
            "DATABASE_APP_PASSWORD",
            pw,
        ) {
            errors.push(e);
        }
    }

    errors
}

fn validate_url_password_match(
    url_key: &str,
    url: &str,
    password_key: &str,
    expected_password: &str,
) -> Result<(), CredentialError> {
    let url_password = match extract_password_from_url(url) {
        Some(p) => p,
        None => {
            return Err(CredentialError {
                message: format!("{url_key} does not contain a password"),
                hint: format!(
                    "Ensure {url_key} is formatted as postgresql://user:password@host:port/db"
                ),
            });
        }
    };

    // URL-decode the password for comparison (passwords may be URL-encoded in the URL)
    let decoded_url_password = urlencoding::decode(&url_password)
        .unwrap_or_else(|_| url_password.clone().into())
        .to_string();

    if decoded_url_password != expected_password {
        return Err(CredentialError {
            message: format!(
                "{url_key} contains password that does not match {password_key}"
            ),
            hint: format!(
                "Run `ferrex-init init --rotate db` to regenerate consistent credentials"
            ),
        });
    }

    Ok(())
}

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

    #[test]
    fn test_extract_password_from_url() {
        assert_eq!(
            extract_password_from_url(
                "postgresql://user:secret@localhost:5432/db"
            ),
            Some("secret".to_string())
        );
        assert_eq!(
            extract_password_from_url("postgresql://user@localhost:5432/db"),
            None
        );
        assert_eq!(extract_password_from_url("not-a-url"), None);
        // URL-encoded password
        assert_eq!(
            extract_password_from_url(
                "postgresql://user:p%40ssword@localhost:5432/db"
            ),
            Some("p%40ssword".to_string())
        );
    }

    #[test]
    fn test_validate_credential_consistency_matching() {
        let kv = vec![
            (
                "DATABASE_URL".into(),
                "postgresql://user:secret@localhost:5432/db".into(),
            ),
            ("DATABASE_APP_PASSWORD".into(), "secret".into()),
            (
                "DATABASE_URL_ADMIN".into(),
                "postgresql://admin:admin_pw@localhost:5432/db".into(),
            ),
            ("DATABASE_ADMIN_PASSWORD".into(), "admin_pw".into()),
            (
                "DATABASE_URL_CONTAINER".into(),
                "postgresql://user:secret@db:5432/db".into(),
            ),
        ];
        let errors = validate_credential_consistency(&kv);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_validate_credential_consistency_mismatch() {
        let kv = vec![
            (
                "DATABASE_URL".into(),
                "postgresql://user:old_secret@localhost:5432/db".into(),
            ),
            ("DATABASE_APP_PASSWORD".into(), "new_secret".into()),
        ];
        let errors = validate_credential_consistency(&kv);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("DATABASE_URL"));
        assert!(errors[0].message.contains("does not match"));
    }

    #[test]
    fn test_validate_credential_consistency_url_encoded_password() {
        // Password with special characters that get URL-encoded
        let kv = vec![
            (
                "DATABASE_URL".into(),
                "postgresql://user:p%40ssword@localhost:5432/db".into(),
            ),
            ("DATABASE_APP_PASSWORD".into(), "p@ssword".into()),
        ];
        let errors = validate_credential_consistency(&kv);
        assert!(
            errors.is_empty(),
            "URL-encoded passwords should match: {:?}",
            errors
        );
    }
}
