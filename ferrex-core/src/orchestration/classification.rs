use once_cell::sync::Lazy;
use regex::Regex;
use std::borrow::Cow;

use crate::orchestration::actors::messages::ParentDescriptors;
use crate::orchestration::series::{SeriesFolderClues, clean_series_title, slugify_series_title};
use crate::{LibraryType, TvParser};

/// Provides classification utilities for translating filesystem folder names into
/// structured hierarchy hints that travel with scan contexts.
#[derive(Debug)]
pub struct FolderClassifier;

impl FolderClassifier {
    /// Derive the [`ParentDescriptors`] that should accompany a child folder based on the
    /// parent's descriptors, an optional library type hint, and the folder name itself.
    pub fn derive_child_descriptors(
        parent: &ParentDescriptors,
        library_type_hint: Option<LibraryType>,
        folder_name: &str,
    ) -> ParentDescriptors {
        let mut next = parent.clone();
        next.extra_tag = None;

        let trimmed = folder_name.trim();
        if trimmed.is_empty() {
            return next;
        }

        let library_type = library_type_hint
            .or(parent.resolved_type)
            .unwrap_or(LibraryType::Series);

        match library_type {
            LibraryType::Series => classify_series_folder(&mut next, parent, trimmed),
            LibraryType::Movies => classify_movie_folder(&mut next, trimmed),
        }

        next
    }
}

fn classify_movie_folder(descriptors: &mut ParentDescriptors, name: &str) {
    if let Some(tag) = infer_extras_tag(name) {
        descriptors.extra_tag = Some(tag);
    }
}

fn classify_series_folder(
    descriptors: &mut ParentDescriptors,
    parent: &ParentDescriptors,
    name: &str,
) {
    match (parent.series_slug.as_deref(), parent.season_number) {
        (None, _) => {
            if let Some(candidate) = infer_series_candidate(name) {
                descriptors.series_slug = Some(candidate.slug);
                descriptors.series_title_hint = Some(candidate.title);
                descriptors.season_number = candidate.season_number;
                if candidate.season_number.is_some() {
                    descriptors.season_id = None;
                }
            }
        }
        (Some(series_slug), None) => {
            descriptors.series_slug = Some(series_slug.to_string());
            if let Some(season) = infer_season_number(name, parent.series_title_hint.as_deref()) {
                descriptors.season_number = Some(season);
                descriptors.season_id = None;
                descriptors.extra_tag = None;
            } else if let Some(tag) = infer_extras_tag(name) {
                descriptors.extra_tag = Some(tag);
                descriptors.season_number = None;
            } else {
                descriptors.season_number = None;
            }
        }
        (Some(series_slug), Some(current_season)) => {
            descriptors.series_slug = Some(series_slug.to_string());
            if let Some(tag) = infer_extras_tag(name) {
                descriptors.extra_tag = Some(tag);
            }
            descriptors.season_number = Some(current_season);
        }
    }
}

#[derive(Debug, Clone)]
struct SeriesCandidate {
    title: String,
    slug: String,
    season_number: Option<u32>,
}

fn infer_series_candidate(name: &str) -> Option<SeriesCandidate> {
    if let Some((base_title, season)) = extract_trailing_season_hint(name)
        && let Some(slug) = slugify_series_title(&base_title)
    {
        let canonical_title = clean_series_title(&base_title);
        return Some(SeriesCandidate {
            title: canonical_title,
            slug,
            season_number: Some(season),
        });
    }

    let clues = SeriesFolderClues::from_folder_name(name);
    let title_source: Cow<'_, str> = if clues.raw_title == "Unknown Series" {
        Cow::Owned(clean_series_title(name))
    } else {
        Cow::Owned(clues.normalized_title)
    };

    if let Some(slug) = slugify_series_title(&title_source) {
        return Some(SeriesCandidate {
            title: title_source.into_owned(),
            slug,
            season_number: None,
        });
    }

    None
}

fn infer_season_number(name: &str, series_hint: Option<&str>) -> Option<u32> {
    if let Some(season) = TvParser::parse_season_folder(name) {
        return Some(season);
    }

    TvParser::parse_season_folder_with_series(name, series_hint)
}

fn infer_extras_tag(name: &str) -> Option<String> {
    let lowered = name.to_ascii_lowercase();
    match lowered.as_str() {
        "extras" | "specials" | "special" => Some(name.trim().to_string()),
        _ => None,
    }
}

fn extract_trailing_season_hint(name: &str) -> Option<(String, u32)> {
    static SEASON_SUFFIX_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
        vec![
            Regex::new(r"(?i)^(?P<title>.+?)[\s._-]*season[\s._-]*(?P<num>\d{1,3})$").unwrap(),
            Regex::new(r"(?i)^(?P<title>.+?)[\s._-]*series[\s._-]*(?P<num>\d{1,3})$").unwrap(),
            Regex::new(r"(?i)^(?P<title>.+?)[\s._-]*s(?P<num>\d{1,3})$").unwrap(),
            Regex::new(r"(?i)^(?P<title>.+?)\((?P<num>\d{1,3})\)$").unwrap(),
        ]
    });

    let trimmed = name.trim();
    for pattern in SEASON_SUFFIX_PATTERNS.iter() {
        if let Some(captures) = pattern.captures(trimmed) {
            let Some(raw_title) = captures.name("title") else {
                continue;
            };
            let title = raw_title
                .as_str()
                .trim_matches(|ch: char| ch == '-' || ch == '_' || ch.is_whitespace())
                .trim();
            if title.is_empty() {
                continue;
            }

            let Some(num_match) = captures.name("num") else {
                continue;
            };
            if let Ok(number) = num_match.as_str().parse::<u32>()
                && (number > 0 || trimmed.to_ascii_lowercase().contains("special"))
            {
                return Some((clean_series_title(title), number));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_series_slug_from_folder_with_season_suffix() {
        let parent = ParentDescriptors {
            resolved_type: Some(LibraryType::Series),
            ..ParentDescriptors::default()
        };

        let derived = FolderClassifier::derive_child_descriptors(
            &parent,
            Some(LibraryType::Series),
            "Arcane Season 01",
        );
        assert_eq!(derived.series_slug.as_deref(), Some("arcane"));
        assert_eq!(derived.season_number, Some(1));
    }

    #[test]
    fn keeps_slug_while_marking_season_children() {
        let parent = ParentDescriptors {
            resolved_type: Some(LibraryType::Series),
            series_slug: Some("arcane".into()),
            series_title_hint: Some("Arcane".into()),
            ..ParentDescriptors::default()
        };

        let derived = FolderClassifier::derive_child_descriptors(
            &parent,
            Some(LibraryType::Series),
            "Season 02",
        );
        assert_eq!(derived.series_slug.as_deref(), Some("arcane"));
        assert_eq!(derived.season_number, Some(2));
    }

    #[test]
    fn marks_extras_folder() {
        let parent = ParentDescriptors {
            resolved_type: Some(LibraryType::Series),
            series_slug: Some("arcane".into()),
            ..ParentDescriptors::default()
        };

        let derived = FolderClassifier::derive_child_descriptors(
            &parent,
            Some(LibraryType::Series),
            "Extras",
        );
        assert_eq!(derived.extra_tag.as_deref(), Some("Extras"));
    }
}
