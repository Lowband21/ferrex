//! Playback diagnostics helpers that avoid leaking bearer-style URL secrets.

const ACCESS_TOKEN_PARAM: &str = "access_token=";
const REDACTED_TOKEN: &str = "<redacted>";

/// Redact access-token query values from playback URLs or log lines that contain them.
pub fn redact_playback_url(input: &str) -> String {
    redact_query_value(input, ACCESS_TOKEN_PARAM, REDACTED_TOKEN)
}

/// Returns true when the string contains an access-token query parameter.
pub(crate) fn contains_access_token(input: &str) -> bool {
    input.contains(ACCESS_TOKEN_PARAM)
}

fn redact_query_value(input: &str, key: &str, replacement: &str) -> String {
    let mut redacted = String::with_capacity(input.len());
    let mut remainder = input;

    while let Some(index) = remainder.find(key) {
        let (prefix, suffix) = remainder.split_at(index);
        redacted.push_str(prefix);
        redacted.push_str(key);
        redacted.push_str(replacement);

        let value_start = key.len();
        let value = &suffix[value_start..];
        let value_end = value
            .find(|ch: char| {
                matches!(
                    ch,
                    '&' | '#'
                        | ' '
                        | '\t'
                        | '\r'
                        | '\n'
                        | '"'
                        | '\''
                        | ')'
                        | ']'
                        | '}'
                )
            })
            .unwrap_or(value.len());
        remainder = &value[value_end..];
    }

    redacted.push_str(remainder);
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_access_token_in_playback_url() {
        let url = "https://ferrex.example/api/v1/stream/file?access_token=secret-token&quality=best";

        let redacted = redact_playback_url(url);

        assert_eq!(
            redacted,
            "https://ferrex.example/api/v1/stream/file?access_token=<redacted>&quality=best"
        );
        assert!(!redacted.contains("secret-token"));
    }

    #[test]
    fn redacts_access_token_inside_mpv_log_line() {
        let line = "Playing: https://ferrex.example/api/v1/stream/file?access_token=raw-secret)";

        let redacted = redact_playback_url(line);

        assert_eq!(
            redacted,
            "Playing: https://ferrex.example/api/v1/stream/file?access_token=<redacted>)"
        );
        assert!(!redacted.contains("raw-secret"));
    }

    #[test]
    fn leaves_urls_without_access_token_unchanged() {
        let url = "https://ferrex.example/api/v1/stream/file?ticket=public";

        assert_eq!(redact_playback_url(url), url);
        assert!(!contains_access_token(url));
    }
}
