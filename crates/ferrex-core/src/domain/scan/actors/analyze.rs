use ferrex_model::{MediaFileMetadata, MediaID, VideoMediaType};

use crate::domain::scan::AnalyzeScanHierarchy;
use crate::domain::scan::orchestration::job::MediaAnalyzeJob;
use crate::{
    error::{MediaError, Result},
    infra::media::metadata::MetadataExtractor,
    types::{ids::LibraryId, library::LibraryType},
};

use crate::domain::scan::orchestration::job::MediaFingerprint;

use crate::domain::scan::orchestration::context::ScanNodeKind;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaAnalyzed {
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub variant: VideoMediaType,
    pub hierarchy: AnalyzeScanHierarchy,
    pub node: ScanNodeKind,
    pub path_norm: String,
    pub fingerprint: MediaFingerprint,
    pub analyzed_at: DateTime<Utc>,
    pub analysis: AnalysisContext,
    #[serde(default)]
    pub thumbnails: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AnalysisContext {
    pub technical: Option<MediaFileMetadata>,
    pub demo_note: Option<String>,
    pub tmdb_id_hint: Option<u64>,
}

#[async_trait]
pub trait MediaAnalyzeActor: Send + Sync {
    async fn analyze(&self, command: MediaAnalyzeJob) -> Result<MediaAnalyzed>;
}

#[derive(Debug)]
pub struct DefaultMediaAnalyzeActor;

impl Default for DefaultMediaAnalyzeActor {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultMediaAnalyzeActor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl MediaAnalyzeActor for DefaultMediaAnalyzeActor {
    async fn analyze(&self, command: MediaAnalyzeJob) -> Result<MediaAnalyzed> {
        #[cfg(feature = "demo")]
        if crate::domain::demo::policy()
            .is_some_and(|policy| policy.skip_metadata_probe)
            && crate::domain::demo::is_demo_library(&command.library_id)
        {
            return Ok(MediaAnalyzed {
                library_id: command.library_id,
                media_id: command.media_id,
                variant: command.variant,
                path_norm: command.path_norm,
                fingerprint: command.fingerprint,
                hierarchy: command.hierarchy,
                node: command.node,
                analyzed_at: Utc::now(),
                analysis: AnalysisContext {
                    technical: None,
                    demo_note: Some(
                        "metadata probe skipped in demo mode".into(),
                    ),
                    tmdb_id_hint: None,
                },
                thumbnails: Vec::new(),
            });
        }

        let library_type = match command.variant {
            VideoMediaType::Movie => LibraryType::Movies,
            VideoMediaType::Series
            | VideoMediaType::Season
            | VideoMediaType::Episode => LibraryType::Series,
        };
        let path_for_probe = command.path_norm.clone();
        let extraction = tokio::task::spawn_blocking(move || {
            let path = PathBuf::from(&path_for_probe);
            let mut extractor =
                MetadataExtractor::with_library_type(library_type);
            extractor.extract_metadata(&path)
        })
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "metadata extraction task failed: {e}"
            ))
        })?;

        let technical = match extraction {
            Ok(metadata) => Some(metadata),
            Err(err) => {
                warn!(
                    "Metadata extraction failed for {}: {}",
                    command.path_norm, err
                );
                None
            }
        };

        // TODO: Impl from<MediaAnalyzeJob> for MediaAnalyzed
        Ok(MediaAnalyzed {
            library_id: command.library_id,
            media_id: command.media_id,
            variant: command.variant,
            hierarchy: command.hierarchy,
            node: command.node,
            path_norm: command.path_norm,
            fingerprint: command.fingerprint,
            analyzed_at: Utc::now(),
            analysis: AnalysisContext {
                technical,
                demo_note: None,
                tmdb_id_hint: None,
            },
            thumbnails: Vec::new(),
        })
    }
}
