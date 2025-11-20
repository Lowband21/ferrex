use std::collections::HashSet;
use std::fmt;

use crate::database::traits::MediaDatabaseTrait;
use crate::orchestration::actors::messages::ParentDescriptors;
use crate::orchestration::series::clean_series_title;
use crate::types::media::SeriesReference;
use crate::{LibraryID, Result};

/// Helper responsible for locating existing series references using canonical hints.
pub struct SeriesLocator<'a> {
    backend: &'a dyn MediaDatabaseTrait,
}

impl<'a> fmt::Debug for SeriesLocator<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backend_type = std::any::type_name_of_val(self.backend);
        f.debug_struct("SeriesLocator")
            .field("backend_type", &backend_type)
            .finish()
    }
}

impl<'a> SeriesLocator<'a> {
    pub fn new(backend: &'a dyn MediaDatabaseTrait) -> Self {
        Self { backend }
    }

    /// Attempt to find an existing series using the parent descriptors and a fallback title.
    pub async fn find_existing_series(
        &self,
        library_id: LibraryID,
        descriptors: Option<&ParentDescriptors>,
        fallback_title: &str,
    ) -> Result<Option<SeriesReference>> {
        if let Some(desc) = descriptors
            && let Some(id) = desc.series_id
            && let Ok(series) = self.backend.get_series_reference(&id).await
        {
            return Ok(Some(series));
        }

        let mut seen = HashSet::new();
        for title in candidate_titles(descriptors, fallback_title) {
            if !seen.insert(title.clone()) {
                continue;
            }

            if let Some(existing) = self.backend.find_series_by_name(library_id, &title).await? {
                return Ok(Some(existing));
            }
        }

        Ok(None)
    }
}

fn candidate_titles(descriptors: Option<&ParentDescriptors>, fallback_title: &str) -> Vec<String> {
    let mut titles = Vec::new();

    if let Some(desc) = descriptors {
        if let Some(hint) = &desc.series_title_hint {
            titles.push(clean_series_title(hint));
        }

        if let Some(slug) = &desc.series_slug {
            let slug_title = slug.replace('-', " ");
            titles.push(clean_series_title(&slug_title));
        }
    }

    titles.push(clean_series_title(fallback_title));
    titles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::actors::messages::ParentDescriptors;

    #[test]
    fn candidate_titles_prefers_hints() {
        let desc = ParentDescriptors {
            series_title_hint: Some("My Show".into()),
            series_slug: Some("my-show".into()),
            ..ParentDescriptors::default()
        };

        let titles = candidate_titles(Some(&desc), "Fallback Title");
        assert_eq!(titles[0], "My Show");
        assert_eq!(titles[1], "my show");
        assert_eq!(titles[2], "Fallback Title");
    }
}
