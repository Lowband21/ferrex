use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

use crate::database::ports::media_references::MediaReferencesRepository;
use crate::error::{MediaError, Result};
use crate::metadata::MetadataExtractor;
use crate::orchestration::job::{
    ImageFetchJob, IndexUpsertJob, MediaAnalyzeJob, MediaFingerprint, MetadataEnrichJob,
};
use crate::types::ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID};
use crate::types::library::LibraryType;
use crate::types::media::Media;

mod image_fetch;
mod tmdb;

pub use image_fetch::DefaultImageFetchActor;
pub use tmdb::TmdbMetadataActor;

#[derive(Clone, Debug)]
pub struct MediaAnalyzeCommand {
    pub job: MediaAnalyzeJob,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaAnalyzed {
    pub library_id: LibraryID,
    pub path_norm: String,
    pub fingerprint: MediaFingerprint,
    pub analyzed_at: DateTime<Utc>,
    pub streams_json: serde_json::Value,
    pub thumbnails: Vec<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub context: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct MetadataCommand {
    pub job: MetadataEnrichJob,
    pub analyzed: MediaAnalyzed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaReadyForIndex {
    pub library_id: LibraryID,
    pub logical_id: Option<String>,
    pub normalized_title: Option<String>,
    pub analyzed: MediaAnalyzed,
    pub prepared_at: DateTime<Utc>,
    #[serde(default)]
    pub image_jobs: Vec<ImageFetchJob>,
}

#[derive(Clone, Debug)]
pub struct IndexCommand {
    pub job: IndexUpsertJob,
    pub ready: MediaReadyForIndex,
}

#[derive(Clone, Debug)]
pub struct ImageFetchCommand {
    pub job: ImageFetchJob,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexingChange {
    Created,
    Updated,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexingOutcome {
    pub library_id: LibraryID,
    pub path_norm: String,
    pub indexed_at: DateTime<Utc>,
    pub upserted: bool,
    #[serde(default)]
    pub media: Option<Media>,
    #[serde(default)]
    pub media_id: Option<Uuid>,
    #[serde(default = "IndexingOutcome::default_change")]
    pub change: IndexingChange,
}

impl IndexingOutcome {
    fn default_change() -> IndexingChange {
        IndexingChange::Created
    }
}

#[async_trait]
pub trait MediaAnalyzeActor: Send + Sync {
    async fn analyze(&self, command: MediaAnalyzeCommand) -> Result<MediaAnalyzed>;
}

#[async_trait]
pub trait MetadataActor: Send + Sync {
    async fn enrich(&self, command: MetadataCommand) -> Result<MediaReadyForIndex>;
}

#[async_trait]
pub trait IndexerActor: Send + Sync {
    async fn index(&self, command: IndexCommand) -> Result<IndexingOutcome>;
}

#[async_trait]
pub trait ImageFetchActor: Send + Sync {
    async fn fetch(&self, command: ImageFetchCommand) -> Result<()>;
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

    fn infer_library_type(context: &Value) -> LibraryType {
        context
            .as_object()
            .and_then(|obj| obj.get("library_type"))
            .and_then(|val| val.as_str())
            .and_then(|raw| match raw {
                "Movies" => Some(LibraryType::Movies),
                "Series" => Some(LibraryType::Series),
                _ => None,
            })
            .unwrap_or(LibraryType::Movies)
    }
}

#[async_trait]
impl MediaAnalyzeActor for DefaultMediaAnalyzeActor {
    async fn analyze(&self, command: MediaAnalyzeCommand) -> Result<MediaAnalyzed> {
        let MediaAnalyzeCommand { job } = command;
        let MediaAnalyzeJob {
            library_id,
            path_norm,
            fingerprint,
            discovered_at: _,
            context,
            scan_reason: _,
        } = job;

        #[cfg(feature = "demo")]
        if crate::demo::policy().is_some_and(|policy| policy.skip_metadata_probe)
            && crate::demo::is_demo_library(&library_id)
        {
            let mut context_obj: Map<String, Value> = match context {
                Value::Object(map) => map,
                Value::Null => Map::new(),
                other => {
                    let mut map = Map::new();
                    map.insert("raw_context".into(), other);
                    map
                }
            };
            context_obj.insert("demo_mode".into(), Value::Bool(true));
            context_obj.insert(
                "note".into(),
                Value::String("metadata probe skipped in demo mode".into()),
            );

            return Ok(MediaAnalyzed {
                library_id,
                path_norm,
                fingerprint,
                analyzed_at: Utc::now(),
                streams_json: serde_json::json!({ "demo": true }),
                thumbnails: Vec::new(),
                context: Value::Object(context_obj),
            });
        }

        let library_type = Self::infer_library_type(&context);
        let path_for_probe = path_norm.clone();
        let extraction = tokio::task::spawn_blocking(move || {
            let path = PathBuf::from(&path_for_probe);
            let mut extractor = MetadataExtractor::with_library_type(library_type);
            extractor.extract_metadata(&path)
        })
        .await
        .map_err(|e| MediaError::Internal(format!("metadata extraction task failed: {e}")))?;

        let (streams_json, metadata_value) = match extraction {
            Ok(metadata) => {
                let json = serde_json::to_value(&metadata).unwrap_or(Value::Null);
                (json.clone(), Some(json))
            }
            Err(err) => {
                warn!("Metadata extraction failed for {}: {}", path_norm, err);
                (serde_json::json!({ "placeholder": true }), None)
            }
        };

        let mut context_obj: Map<String, Value> = match context {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            other => {
                let mut map = Map::new();
                map.insert("raw_context".into(), other);
                map
            }
        };

        if let Some(meta) = metadata_value {
            context_obj.insert("technical_metadata".into(), meta);
        }

        Ok(MediaAnalyzed {
            library_id,
            path_norm,
            fingerprint,
            analyzed_at: Utc::now(),
            streams_json,
            thumbnails: Vec::new(),
            context: Value::Object(context_obj),
        })
    }
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
    async fn enrich(&self, command: MetadataCommand) -> Result<MediaReadyForIndex> {
        Ok(MediaReadyForIndex {
            library_id: command.job.library_id,
            logical_id: None,
            normalized_title: None,
            analyzed: command.analyzed,
            prepared_at: Utc::now(),
            image_jobs: Vec::new(),
        })
    }
}

pub struct DefaultIndexerActor {
    media_refs: Arc<dyn MediaReferencesRepository>,
}

impl fmt::Debug for DefaultIndexerActor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultIndexerActor")
            .finish_non_exhaustive()
    }
}

impl DefaultIndexerActor {
    pub fn new(media_refs: Arc<dyn MediaReferencesRepository>) -> Self {
        Self { media_refs }
    }

    async fn hydrate_media(&self, uuid: Uuid) -> Result<Option<Media>> {
        match self.media_refs.get_movie_reference(&MovieID(uuid)).await {
            Ok(movie) => return Ok(Some(Media::Movie(movie))),
            Err(MediaError::NotFound(_)) => {}
            Err(err) => return Err(err),
        }

        match self.media_refs.get_series_reference(&SeriesID(uuid)).await {
            Ok(series) => return Ok(Some(Media::Series(series))),
            Err(MediaError::NotFound(_)) => {}
            Err(err) => return Err(err),
        }

        match self.media_refs.get_season_reference(&SeasonID(uuid)).await {
            Ok(season) => return Ok(Some(Media::Season(season))),
            Err(MediaError::NotFound(_)) => {}
            Err(err) => return Err(err),
        }

        match self
            .media_refs
            .get_episode_reference(&EpisodeID(uuid))
            .await
        {
            Ok(episode) => return Ok(Some(Media::Episode(episode))),
            Err(MediaError::NotFound(_)) => {}
            Err(err) => return Err(err),
        }

        Ok(None)
    }

    async fn resolve_media(
        &self,
        candidate: Option<Uuid>,
    ) -> Result<(Option<Media>, Option<Uuid>)> {
        if let Some(uuid) = candidate {
            match self.hydrate_media(uuid).await? {
                Some(media) => Ok((Some(media), Some(uuid))),
                None => Ok((None, Some(uuid))),
            }
        } else {
            Ok((None, None))
        }
    }

    fn parse_logical_id(job: &IndexUpsertJob, ready: &MediaReadyForIndex) -> Option<Uuid> {
        ready
            .logical_id
            .as_deref()
            .or_else(|| {
                job.logical_entity
                    .get("logical_id")
                    .and_then(|value| value.as_str())
            })
            .and_then(|raw| Uuid::parse_str(raw).ok())
    }
}

#[async_trait]
impl IndexerActor for DefaultIndexerActor {
    async fn index(&self, command: IndexCommand) -> Result<IndexingOutcome> {
        let IndexCommand { job, ready } = command;
        let logical_id = Self::parse_logical_id(&job, &ready);
        let (media, media_id) = self.resolve_media(logical_id).await?;

        Ok(IndexingOutcome {
            library_id: job.library_id,
            path_norm: job.path_norm,
            indexed_at: Utc::now(),
            upserted: true,
            media,
            media_id,
            change: IndexingOutcome::default_change(),
        })
    }
}
