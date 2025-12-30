use async_trait::async_trait;

use crate::{
    domain::scan::{
        actors::metadata::MediaReadyForIndex,
        orchestration::{
            context::{SeriesHint, SeriesRef, SeriesRootPath},
            job::SeriesResolveJob,
            series::{SeriesFolderClues, slugify_series_title},
            series_state::{SeriesScanState, SeriesScanStateRepository},
        },
    },
    error::{MediaError, Result},
    types::ids::LibraryId,
};

use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct SeriesResolution {
    pub series_ref: SeriesRef,
    pub ready: MediaReadyForIndex,
}

#[async_trait]
pub trait SeriesMetadataProvider: Send + Sync {
    async fn resolve_series(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
        hint: &SeriesHint,
        folder_name: &str,
    ) -> Result<SeriesResolution>;
}

#[async_trait]
pub trait SeriesResolverPort: Send + Sync {
    async fn resolve(&self, job: &SeriesResolveJob)
    -> Result<SeriesResolution>;

    async fn mark_failed(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        reason: String,
    ) -> Result<()>;

    async fn get_state(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
    ) -> Result<Option<SeriesScanState>>;
}

#[derive(Clone)]
pub struct DefaultSeriesResolver {
    provider: Arc<dyn SeriesMetadataProvider>,
    states: Arc<Box<dyn SeriesScanStateRepository>>,
}

impl std::fmt::Debug for DefaultSeriesResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultSeriesResolver")
            .field("provider", &"<dyn SeriesMetadataProvider>")
            .field("states", &"<dyn SeriesScanStateRepository>")
            .finish()
    }
}

impl DefaultSeriesResolver {
    pub fn new(
        provider: Arc<dyn SeriesMetadataProvider>,
        states: Arc<Box<dyn SeriesScanStateRepository>>,
    ) -> Self {
        Self { provider, states }
    }

    fn derive_hint_from_folder(folder_name: &str) -> Option<SeriesHint> {
        let clues = SeriesFolderClues::from_folder_name(folder_name);
        if clues.raw_title == "Unknown Series" {
            return None;
        }

        let normalized_title = clues.normalized_title;
        let slug = slugify_series_title(&normalized_title);
        Some(SeriesHint {
            title: normalized_title,
            slug,
            year: clues.year,
            region: clues.region,
        })
    }
}

#[async_trait]
impl SeriesResolverPort for DefaultSeriesResolver {
    async fn resolve(
        &self,
        job: &SeriesResolveJob,
    ) -> Result<SeriesResolution> {
        let hint = job
            .hint
            .clone()
            .or_else(|| Self::derive_hint_from_folder(&job.folder_name))
            .ok_or_else(|| {
                MediaError::InvalidMedia(
                    "series resolve requires title hint".into(),
                )
            })?;

        self.states
            .mark_seeded(
                job.library_id,
                job.series_root_path.clone(),
                Some(hint.clone()),
            )
            .await?;

        let resolution = self
            .provider
            .resolve_series(
                job.library_id,
                &job.series_root_path,
                &hint,
                &job.folder_name,
            )
            .await?;

        self.states
            .mark_resolved(
                job.library_id,
                job.series_root_path.clone(),
                resolution.series_ref.clone(),
            )
            .await?;

        Ok(resolution)
    }

    async fn mark_failed(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        reason: String,
    ) -> Result<()> {
        let _ = self
            .states
            .mark_failed(library_id, series_root_path, reason)
            .await?;
        Ok(())
    }

    async fn get_state(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
    ) -> Result<Option<SeriesScanState>> {
        self.states.get(library_id, series_root_path).await
    }
}
