use once_cell::sync::Lazy;
use regex::Regex;

static COLLAPSE_WHITESPACE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+").expect("whitespace regex should compile"));

/// Normalize a series title by collapsing punctuation and redundant whitespace.
pub fn clean_series_title(title: &str) -> String {
    let collapsed = title.replace(['.', '_', '-'], " ");
    COLLAPSE_WHITESPACE_REGEX
        .replace_all(collapsed.trim(), " ")
        .to_string()
}

/// Generate a filesystem and database friendly slug from a canonical series title.
pub fn slugify_series_title(title: &str) -> Option<String> {
    let cleaned = clean_series_title(title);
    let slug: String = cleaned
        .chars()
        .map(|ch| match ch {
            ch if ch.is_ascii_alphanumeric() => ch.to_ascii_lowercase(),
            ' ' | '\t' | '\n' => '-',
            _ => '-',
        })
        .collect();

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        None
    } else {
        // Collapse duplicate dashes produced by punctuation stripping.
        Some(
            trimmed
                .split('-')
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
                .join("-"),
        )
    }
}

/// Collapse repeated whitespace sequences into single spaces while trimming ends.
pub fn collapse_whitespace(value: &str) -> String {
    COLLAPSE_WHITESPACE_REGEX
        .replace_all(value.trim(), " ")
        .to_string()
}
