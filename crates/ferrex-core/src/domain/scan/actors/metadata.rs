use crate::domain::scan::orchestration::context::ScanNodeKind;
use crate::domain::scan::{
    AnalyzeScanHierarchy, ImageFetchJob, MediaAnalyzed, MetadataEnrichJob,
};
use crate::error;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrex_model::{LibraryId, MediaID, VideoMediaType};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct MetadataCommand {
    pub job: MetadataEnrichJob,
    pub analyzed: MediaAnalyzed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaReadyForIndex {
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub variant: VideoMediaType,
    pub hierarchy: AnalyzeScanHierarchy,
    pub node: ScanNodeKind,
    pub normalized_title: Option<String>,
    pub analyzed: MediaAnalyzed,
    pub prepared_at: DateTime<Utc>,
    #[serde(default)]
    pub image_jobs: Vec<ImageFetchJob>,
}

#[async_trait]
pub trait MetadataActor: Send + Sync {
    async fn enrich(
        &self,
        command: MetadataCommand,
    ) -> error::Result<MediaReadyForIndex>;
}

#[derive(Debug)]
pub struct DefaultMetadataActor;

impl Default for DefaultMetadataActor {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultMetadataActor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl MetadataActor for DefaultMetadataActor {
    async fn enrich(
        &self,
        command: MetadataCommand,
    ) -> error::Result<MediaReadyForIndex> {
        Ok(MediaReadyForIndex {
            library_id: command.job.library_id,
            media_id: command.job.media_id,
            variant: command.job.variant,
            hierarchy: command.job.hierarchy.clone(),
            node: command.job.node.clone(),
            normalized_title: None,
            analyzed: command.analyzed,
            prepared_at: Utc::now(),
            image_jobs: Vec::new(),
        })
    }
}
