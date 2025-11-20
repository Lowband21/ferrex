use std::collections::HashMap;

use crate::{
    media_library::MediaFile,
    models::TvShow,
    state::{SortBy, SortOrder},
};

pub fn sort_media(
    movies: &mut Vec<MediaFile>,
    _tv_shows: &mut HashMap<String, TvShow>,
    sort_by: SortBy,
    sort_order: SortOrder,
) {
    // Sort movies
    movies.sort_by(|a, b| {
        let cmp = match sort_by {
            SortBy::DateAdded => a.created_at.cmp(&b.created_at),
            SortBy::Title => {
                let title_a = a
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .map(|p| &p.title)
                    .unwrap_or(&a.filename);
                let title_b = b
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .map(|p| &p.title)
                    .unwrap_or(&b.filename);
                title_a.cmp(title_b)
            }
            SortBy::Year => {
                let year_a = a
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .and_then(|p| p.year);
                let year_b = b
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .and_then(|p| p.year);
                year_a.cmp(&year_b)
            }
            SortBy::Rating => {
                let rating_a = a
                    .metadata
                    .as_ref()
                    .and_then(|m| m.external_info.as_ref())
                    .and_then(|e| e.rating);
                let rating_b = b
                    .metadata
                    .as_ref()
                    .and_then(|m| m.external_info.as_ref())
                    .and_then(|e| e.rating);
                rating_a
                    .partial_cmp(&rating_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        };

        match sort_order {
            SortOrder::Ascending => cmp,
            SortOrder::Descending => cmp.reverse(),
        }
    });

    // For TV shows, we'll sort the values when displaying them
    // since HashMap doesn't maintain order
}

// Helper function to format duration
pub fn format_duration(seconds: f64) -> String {
    let hours = (seconds / 3600.0) as u32;
    let minutes = ((seconds % 3600.0) / 60.0) as u32;
    let secs = (seconds % 60.0) as u32;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}
