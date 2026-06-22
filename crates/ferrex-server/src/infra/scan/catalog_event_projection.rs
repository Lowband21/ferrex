use std::{collections::HashSet, fmt, sync::Arc};

use ferrex_core::{
    api::types::{MovieReferenceBatchResponse, SeriesBundleResponse},
    application::unit_of_work::AppUnitOfWork,
    domain::scan::actors::index::{IndexingChange, IndexingOutcome},
    error::MediaError,
    traits::prelude::MediaIDLike,
    types::{
        LibraryId, Media, MediaEvent, MediaID, MovieBatchId, ScanEventMetadata,
        ScanProgressEvent, SeriesID,
    },
};
use sha2::Digest;
use tokio::sync::Mutex;
use tracing::{error, warn};

use super::{
    media_event_bus::{MediaEventBus, MediaEventFrame},
    scan_manager::ScanEventKind,
};

#[derive(Clone)]
pub struct CatalogEventProjection {
    inner: Arc<CatalogEventProjectionInner>,
}

struct CatalogEventProjectionInner {
    unit_of_work: Arc<AppUnitOfWork>,
    media_bus: Arc<MediaEventBus>,
    seen_media: Mutex<HashSet<uuid::Uuid>>,
}

impl fmt::Debug for CatalogEventProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unit_of_work_ptr = Arc::as_ptr(&self.inner.unit_of_work);
        let media_bus_ptr = Arc::as_ptr(&self.inner.media_bus);
        f.debug_struct("CatalogEventProjection")
            .field("unit_of_work_ptr", &unit_of_work_ptr)
            .field("media_bus_ptr", &media_bus_ptr)
            .field("receiver_count", &self.receiver_count())
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogEventProjectionError {
    #[error(
        "missing media reference for library {library_id} path {path_norm}"
    )]
    MissingMedia {
        library_id: LibraryId,
        path_norm: String,
    },
    #[error(
        "movie batch versioning upsert failed (library {library_id}, batch {batch_id}): {source}"
    )]
    MovieBatchVersionHash {
        library_id: LibraryId,
        batch_id: MovieBatchId,
        #[source]
        source: anyhow::Error,
    },
}

impl CatalogEventProjection {
    pub fn new(
        unit_of_work: Arc<AppUnitOfWork>,
        media_bus: Arc<MediaEventBus>,
    ) -> Self {
        Self {
            inner: Arc::new(CatalogEventProjectionInner {
                unit_of_work,
                media_bus,
                seen_media: Mutex::new(HashSet::new()),
            }),
        }
    }

    pub fn receiver_count(&self) -> usize {
        self.inner.media_bus.receiver_count()
    }

    pub fn publish_event(&self, event: MediaEvent) -> MediaEventFrame {
        self.inner.media_bus.publish(event)
    }

    pub fn publish_scan_progress_event(
        &self,
        event: ScanEventKind,
        payload: ScanProgressEvent,
        error: Option<String>,
    ) -> MediaEventFrame {
        self.publish_event(Self::scan_progress_media_event(
            event, payload, error,
        ))
    }

    pub async fn publish_indexed_outcome(
        &self,
        outcome: IndexingOutcome,
    ) -> Result<Option<MediaEventFrame>, CatalogEventProjectionError> {
        let mut media = outcome.media.clone();

        if media.is_none() {
            media = self.load_media(outcome.media_id).await;
        }

        let media =
            media.ok_or_else(|| CatalogEventProjectionError::MissingMedia {
                library_id: outcome.library_id,
                path_norm: outcome.path_norm.clone(),
            })?;

        let first_seen = {
            let mut seen = self.inner.seen_media.lock().await;
            seen.insert(outcome.media_id.to_uuid())
        };

        Ok(Self::indexed_media_event(media, &outcome, first_seen)
            .map(|event| self.publish_event(event)))
    }

    pub async fn publish_movie_batch_finalized(
        &self,
        library_id: LibraryId,
        batch_id: MovieBatchId,
    ) -> Result<MediaEventFrame, CatalogEventProjectionError> {
        self.upsert_movie_batch_hash(&library_id, batch_id)
            .await
            .map_err(|source| {
                CatalogEventProjectionError::MovieBatchVersionHash {
                    library_id,
                    batch_id,
                    source,
                }
            })?;

        Ok(self.publish_event(Self::movie_batch_finalized_event(
            library_id, batch_id,
        )))
    }

    pub async fn publish_series_bundle_finalized(
        &self,
        library_id: LibraryId,
        series_id: SeriesID,
    ) -> Option<MediaEventFrame> {
        if !self.upsert_series_bundle_hash(library_id, series_id).await {
            return None;
        }

        Some(self.publish_event(Self::series_bundle_finalized_event(
            library_id, series_id,
        )))
    }

    fn scan_progress_media_event(
        event: ScanEventKind,
        payload: ScanProgressEvent,
        error: Option<String>,
    ) -> MediaEvent {
        let scan_id = payload.scan_id;
        match event {
            ScanEventKind::Started => MediaEvent::ScanStarted {
                scan_id,
                metadata: Self::metadata_from_progress(&payload),
            },
            ScanEventKind::Progress | ScanEventKind::Quiescing => {
                MediaEvent::ScanProgress {
                    scan_id,
                    progress: payload,
                }
            }
            ScanEventKind::Completed => MediaEvent::ScanCompleted {
                scan_id,
                metadata: Self::metadata_from_progress(&payload),
            },
            ScanEventKind::Failed => MediaEvent::ScanFailed {
                scan_id,
                error: error.unwrap_or_else(|| "scan_failed".to_string()),
                metadata: Self::metadata_from_progress(&payload),
            },
        }
    }

    fn metadata_from_progress(
        payload: &ScanProgressEvent,
    ) -> ScanEventMetadata {
        ScanEventMetadata {
            version: payload.version.clone(),
            correlation_id: payload.correlation_id,
            idempotency_key: payload.idempotency_key.clone(),
            library_id: payload.library_id,
        }
    }

    fn indexed_media_event(
        media: Media,
        outcome: &IndexingOutcome,
        first_seen: bool,
    ) -> Option<MediaEvent> {
        let change = match outcome.change {
            IndexingChange::Created if first_seen => IndexingChange::Created,
            _ => IndexingChange::Updated,
        };

        match (media, change) {
            (Media::Movie(movie), IndexingChange::Created) => {
                Some(MediaEvent::MovieAdded { movie: *movie })
            }
            (Media::Movie(movie), IndexingChange::Updated) => {
                Some(MediaEvent::MovieUpdated { movie: *movie })
            }
            (Media::Series(series), IndexingChange::Created) => {
                Some(MediaEvent::SeriesAdded { series: *series })
            }
            (Media::Series(series), IndexingChange::Updated) => {
                Some(MediaEvent::SeriesUpdated { series: *series })
            }
            (Media::Season(_) | Media::Episode(_), _) => None,
        }
    }

    fn movie_batch_finalized_event(
        library_id: LibraryId,
        batch_id: MovieBatchId,
    ) -> MediaEvent {
        MediaEvent::MovieBatchFinalized {
            library_id,
            batch_id,
        }
    }

    fn series_bundle_finalized_event(
        library_id: LibraryId,
        series_id: SeriesID,
    ) -> MediaEvent {
        MediaEvent::SeriesBundleFinalized {
            library_id,
            series_id,
        }
    }

    async fn load_media(&self, mid: MediaID) -> Option<Media> {
        let media_refs = &self.inner.unit_of_work.media_refs;

        match mid {
            MediaID::Movie(movie_id) => {
                match media_refs.get_movie_reference(&movie_id).await {
                    Ok(movie) => Some(Media::Movie(Box::new(movie))),
                    Err(MediaError::NotFound(_)) => None,
                    Err(err) => {
                        warn!("failed to hydrate movie reference {mid}: {err}");
                        None
                    }
                }
            }
            MediaID::Series(series_id) => {
                match media_refs.get_series_reference(&series_id).await {
                    Ok(series) => Some(Media::Series(Box::new(series))),
                    Err(MediaError::NotFound(_)) => None,
                    Err(err) => {
                        warn!(
                            "failed to hydrate series reference {mid}: {err}"
                        );
                        None
                    }
                }
            }
            MediaID::Season(season_id) => {
                match media_refs.get_season_reference(&season_id).await {
                    Ok(season) => Some(Media::Season(Box::new(season))),
                    Err(MediaError::NotFound(_)) => None,
                    Err(err) => {
                        warn!(
                            "failed to hydrate season reference {mid}: {err}"
                        );
                        None
                    }
                }
            }
            MediaID::Episode(episode_id) => {
                match media_refs.get_episode_reference(&episode_id).await {
                    Ok(episode) => Some(Media::Episode(Box::new(episode))),
                    Err(MediaError::NotFound(_)) => None,
                    Err(err) => {
                        warn!(
                            "failed to hydrate episode reference {mid}: {err}"
                        );
                        None
                    }
                }
            }
        }
    }

    async fn upsert_movie_batch_hash(
        &self,
        library_id: &LibraryId,
        batch_id: MovieBatchId,
    ) -> anyhow::Result<()> {
        let movies = self
            .inner
            .unit_of_work
            .media_refs
            .get_movie_references_by_batch(library_id, batch_id)
            .await?;

        let response = MovieReferenceBatchResponse {
            library_id: *library_id,
            batch_id,
            movies,
        };

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)?;
        let digest = sha2::Sha256::digest(bytes.as_slice());
        let hash = u64::from_be_bytes(
            digest[..8]
                .try_into()
                .expect("sha256 digest must be at least 8 bytes"),
        );

        self.inner
            .unit_of_work
            .media_refs
            .upsert_movie_batch_hash(
                library_id,
                &batch_id,
                hash,
                response.movies.len() as u32,
            )
            .await?;

        Ok(())
    }

    async fn upsert_series_bundle_hash(
        &self,
        library_id: LibraryId,
        series_id: SeriesID,
    ) -> bool {
        let uow = &self.inner.unit_of_work;

        let (series, seasons, episodes) = tokio::join!(
            uow.media_refs.get_series_reference(&series_id),
            uow.media_refs.get_series_seasons(&series_id),
            uow.media_refs.get_series_episodes(&series_id),
        );

        let mut series = match series {
            Ok(series) if series.library_id == library_id => series,
            Ok(_) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    "series bundle finalization library mismatch"
                );
                return false;
            }
            Err(err) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    error = %err,
                    "series bundle finalization failed to hydrate series"
                );
                return false;
            }
        };

        let seasons = match seasons {
            Ok(seasons) => seasons,
            Err(err) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    error = %err,
                    "series bundle finalization failed to hydrate seasons"
                );
                return false;
            }
        };

        let episodes = match episodes {
            Ok(episodes) => episodes,
            Err(err) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    error = %err,
                    "series bundle finalization failed to hydrate episodes"
                );
                return false;
            }
        };

        // Keep the server-side versioning record synchronized with the exact
        // serialized bundle that clients invalidate after the emitted event.
        series.details.available_seasons = Some(seasons.len() as u16);
        series.details.available_episodes = Some(episodes.len() as u16);

        let response = SeriesBundleResponse {
            library_id,
            series_id,
            series,
            seasons,
            episodes,
        };

        let bytes = match rkyv::to_bytes::<rkyv::rancor::Error>(&response) {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    error = ?err,
                    "series bundle finalization failed to serialize bundle response"
                );
                return false;
            }
        };

        let digest = sha2::Sha256::digest(bytes.as_slice());
        let hash = u64::from_be_bytes(
            digest[..8]
                .try_into()
                .expect("sha256 digest must be at least 8 bytes"),
        );

        match uow
            .media_refs
            .upsert_series_bundle_hash(&library_id, &series_id, hash)
            .await
        {
            Ok(()) => true,
            Err(err) => {
                error!(
                    library = %library_id,
                    series_id = %series_id,
                    error = %err,
                    "failed to upsert series bundle hash during finalization"
                );
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CatalogEventProjection;
    use crate::infra::scan::scan_manager::ScanEventKind;
    use chrono::Utc;
    use ferrex_core::{
        domain::scan::{
            actors::index::{IndexingChange, IndexingOutcome},
            context::{MovieRootPath, MovieScanHierarchy},
            orchestration::AnalyzeScanHierarchy,
        },
        types::{
            LibraryId, Media, MediaEvent, MediaID, MovieBatchId, MovieID,
            ScanProgressEvent, ScanStageLatencySummary, SeriesID,
        },
    };
    use ferrex_model::{
        EnhancedMovieDetails, EnhancedSeriesDetails, MediaFile, MovieReference,
        Series, image::MediaImages,
    };
    use std::path::PathBuf;
    use uuid::Uuid;

    fn movie_details(title: &str) -> EnhancedMovieDetails {
        EnhancedMovieDetails {
            id: 100,
            title: title.to_string(),
            original_title: Some(title.to_string()),
            overview: None,
            release_date: None,
            runtime: None,
            vote_average: None,
            vote_count: None,
            popularity: None,
            content_rating: None,
            content_ratings: Vec::new(),
            release_dates: Vec::new(),
            genres: Vec::new(),
            spoken_languages: Vec::new(),
            production_companies: Vec::new(),
            production_countries: Vec::new(),
            homepage: None,
            status: None,
            tagline: None,
            budget: None,
            revenue: None,
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            primary_poster_iid: None,
            primary_backdrop_iid: None,
            images: MediaImages::default(),
            cast: Vec::new(),
            crew: Vec::new(),
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ferrex_model::details::ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            collection: None,
            recommendations: Vec::new(),
            similar: Vec::new(),
        }
    }

    fn series_details(name: &str) -> EnhancedSeriesDetails {
        EnhancedSeriesDetails {
            id: 200,
            name: name.to_string(),
            original_name: Some(name.to_string()),
            overview: None,
            first_air_date: None,
            last_air_date: None,
            number_of_seasons: None,
            number_of_episodes: None,
            available_seasons: None,
            available_episodes: None,
            vote_average: None,
            vote_count: None,
            popularity: None,
            content_rating: None,
            content_ratings: Vec::new(),
            release_dates: Vec::new(),
            genres: Vec::new(),
            networks: Vec::new(),
            origin_countries: Vec::new(),
            spoken_languages: Vec::new(),
            production_companies: Vec::new(),
            production_countries: Vec::new(),
            homepage: None,
            status: None,
            tagline: None,
            in_production: None,
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            primary_poster_iid: None,
            primary_backdrop_iid: None,
            images: MediaImages::default(),
            cast: Vec::new(),
            crew: Vec::new(),
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ferrex_model::details::ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            episode_groups: Vec::new(),
            recommendations: Vec::new(),
            similar: Vec::new(),
        }
    }

    fn media_file(media_id: MediaID, library_id: LibraryId) -> MediaFile {
        let now = Utc::now();
        MediaFile {
            id: Uuid::now_v7(),
            media_id,
            path: PathBuf::from("/library/item.mkv"),
            filename: "item.mkv".to_string(),
            size: 123,
            discovered_at: now,
            created_at: now,
            media_file_metadata: None,
            library_id,
        }
    }

    fn movie_reference() -> MovieReference {
        let library_id = LibraryId::new();
        let movie_id = MovieID::new();
        MovieReference {
            id: movie_id,
            library_id,
            batch_id: Some(MovieBatchId(7)),
            tmdb_id: 100,
            title: "Projected Movie".into(),
            details: movie_details("Projected Movie"),
            endpoint: "/api/v1/stream/movie".to_string().into(),
            file: media_file(MediaID::Movie(movie_id), library_id),
            theme_color: None,
        }
    }

    fn series_reference() -> Series {
        let now = Utc::now();
        Series {
            id: SeriesID::new(),
            library_id: LibraryId::new(),
            tmdb_id: 200,
            title: "Projected Series".into(),
            details: series_details("Projected Series"),
            endpoint: "/api/v1/series/projected".to_string().into(),
            discovered_at: now,
            created_at: now,
            theme_color: None,
        }
    }

    fn indexing_outcome(
        library_id: LibraryId,
        media_id: MediaID,
        change: IndexingChange,
    ) -> IndexingOutcome {
        IndexingOutcome {
            library_id,
            path_norm: "/library/Projected Movie/item.mkv".to_string(),
            media_id,
            hierarchy: AnalyzeScanHierarchy::Movie(MovieScanHierarchy {
                movie_root_path: MovieRootPath::try_new(
                    "/library/Projected Movie",
                )
                .expect("movie root path"),
                movie_id: match media_id {
                    MediaID::Movie(id) => Some(id),
                    _ => None,
                },
                extra_tag: None,
            }),
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change,
        }
    }

    fn progress(kind: &str) -> ScanProgressEvent {
        let scan_id = Uuid::now_v7();
        ScanProgressEvent {
            version: "2".to_string(),
            scan_id,
            library_id: LibraryId::new(),
            status: kind.to_string(),
            completed_items: 1,
            total_items: 1,
            validated_items: 1,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items: 0,
            needs_attention_items: 0,
            retrying_items: 0,
            sequence: 42,
            current_path: None,
            path_key: None,
            p95_stage_latencies_ms: ScanStageLatencySummary {
                scan: 1,
                analyze: 2,
                index: 3,
            },
            correlation_id: scan_id,
            idempotency_key: format!("scan:{scan_id}:42"),
            emitted_at: Utc::now(),
            terminal_at: None,
            reason_details: Vec::new(),
        }
    }

    #[test]
    fn indexed_created_outcomes_map_to_added_only_on_first_seen() {
        let movie = movie_reference();
        let movie_outcome = indexing_outcome(
            movie.library_id,
            MediaID::Movie(movie.id),
            IndexingChange::Created,
        );

        let first = CatalogEventProjection::indexed_media_event(
            Media::Movie(Box::new(movie.clone())),
            &movie_outcome,
            true,
        )
        .expect("movie event");
        assert!(matches!(first, MediaEvent::MovieAdded { .. }));

        let repeated = CatalogEventProjection::indexed_media_event(
            Media::Movie(Box::new(movie)),
            &movie_outcome,
            false,
        )
        .expect("movie event");
        assert!(matches!(repeated, MediaEvent::MovieUpdated { .. }));

        let series = series_reference();
        let series_outcome = indexing_outcome(
            series.library_id,
            MediaID::Series(series.id),
            IndexingChange::Created,
        );
        let first_series = CatalogEventProjection::indexed_media_event(
            Media::Series(Box::new(series)),
            &series_outcome,
            true,
        )
        .expect("series event");
        assert!(matches!(first_series, MediaEvent::SeriesAdded { .. }));
    }

    #[test]
    fn indexed_updated_outcomes_map_to_updated_even_when_first_seen() {
        let movie = movie_reference();
        let movie_outcome = indexing_outcome(
            movie.library_id,
            MediaID::Movie(movie.id),
            IndexingChange::Updated,
        );

        let event = CatalogEventProjection::indexed_media_event(
            Media::Movie(Box::new(movie)),
            &movie_outcome,
            true,
        )
        .expect("movie event");
        assert!(matches!(event, MediaEvent::MovieUpdated { .. }));

        let series = series_reference();
        let series_outcome = indexing_outcome(
            series.library_id,
            MediaID::Series(series.id),
            IndexingChange::Updated,
        );
        let event = CatalogEventProjection::indexed_media_event(
            Media::Series(Box::new(series)),
            &series_outcome,
            true,
        )
        .expect("series event");
        assert!(matches!(event, MediaEvent::SeriesUpdated { .. }));
    }

    #[test]
    fn finalization_events_keep_existing_sse_names() {
        let library_id = LibraryId::new();
        let movie = CatalogEventProjection::movie_batch_finalized_event(
            library_id,
            MovieBatchId(3),
        );
        assert!(matches!(
            movie,
            MediaEvent::MovieBatchFinalized { batch_id, .. } if batch_id == MovieBatchId(3)
        ));
        assert_eq!(
            movie.sse_event_type().event_name(),
            "media.movie_batch_finalized"
        );

        let series_id = SeriesID::new();
        let series = CatalogEventProjection::series_bundle_finalized_event(
            library_id, series_id,
        );
        assert!(matches!(
            series,
            MediaEvent::SeriesBundleFinalized { series_id: id, .. } if id == series_id
        ));
        assert_eq!(
            series.sse_event_type().event_name(),
            "media.series_bundle_finalized"
        );
    }

    #[test]
    fn scan_progress_projection_preserves_wire_variants() {
        let started = CatalogEventProjection::scan_progress_media_event(
            ScanEventKind::Started,
            progress("discovering"),
            None,
        );
        assert!(matches!(started, MediaEvent::ScanStarted { .. }));
        assert_eq!(started.sse_event_type().event_name(), "scan.started");

        let progress_event = CatalogEventProjection::scan_progress_media_event(
            ScanEventKind::Progress,
            progress("processing"),
            None,
        );
        assert!(matches!(progress_event, MediaEvent::ScanProgress { .. }));
        assert_eq!(
            progress_event.sse_event_type().event_name(),
            "scan.progress"
        );

        let quiescing = CatalogEventProjection::scan_progress_media_event(
            ScanEventKind::Quiescing,
            progress("quiescing"),
            None,
        );
        assert!(matches!(quiescing, MediaEvent::ScanProgress { .. }));
        assert_eq!(quiescing.sse_event_type().event_name(), "scan.progress");

        let completed = CatalogEventProjection::scan_progress_media_event(
            ScanEventKind::Completed,
            progress("completed"),
            None,
        );
        assert!(matches!(completed, MediaEvent::ScanCompleted { .. }));
        assert_eq!(completed.sse_event_type().event_name(), "scan.completed");

        let failed = CatalogEventProjection::scan_progress_media_event(
            ScanEventKind::Failed,
            progress("failed"),
            Some("boom".to_string()),
        );
        assert!(matches!(
            failed,
            MediaEvent::ScanFailed { ref error, .. } if error == "boom"
        ));
        assert_eq!(failed.sse_event_type().event_name(), "scan.failed");
    }
}
