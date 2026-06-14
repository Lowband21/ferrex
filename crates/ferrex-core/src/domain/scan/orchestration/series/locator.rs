use std::fmt;
use std::sync::Arc;

use crate::domain::scan::orchestration::context::WithSeriesHierarchy;
use crate::{
    database::repository_ports::media_references::MediaReferencesRepository,
    types::media::Series,
};

/// Helper responsible for locating existing series references using canonical hints.
#[derive(Clone)]
pub struct SeriesLocator {
    media_refs: Arc<dyn MediaReferencesRepository>,
}

impl fmt::Debug for SeriesLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backend_type = std::any::type_name_of_val(self.media_refs.as_ref());
        f.debug_struct("SeriesLocator")
            .field("backend_type", &backend_type)
            .finish()
    }
}

impl SeriesLocator {
    pub fn new(media_refs: Arc<dyn MediaReferencesRepository>) -> Self {
        Self { media_refs }
    }

    /// Attempt to find an existing series using the parent descriptors and a fallback title.
    pub async fn find_existing_series(
        &self,
        desc: &dyn WithSeriesHierarchy,
    ) -> Option<Series> {
        if let Some(id) = desc.series_id()
            && let Ok(series) = self.media_refs.get_series_reference(&id).await
        {
            return Some(series);
        }

        None
    }
}
