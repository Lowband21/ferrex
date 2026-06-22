use std::{any::type_name, fmt, sync::Arc};

use uuid::Uuid;

use crate::{
    domain::scan::orchestration::{
        correlation::CorrelationCache,
        events::{
            JobEvent, JobEventPayload, JobEventPublisher, stable_path_key,
        },
        job::{DependencyKey, EnqueueRequest, JobHandle},
        queue::QueueService,
    },
    error::{MediaError, Result},
    types::LibraryId,
};

/// Publishes queue enqueue outcomes as durable scan pipeline events and keeps
/// the in-process correlation cache aligned with the emitted event metadata.
pub struct JobPublisher<P: ?Sized> {
    events: Arc<P>,
    correlations: CorrelationCache,
}

impl<P: ?Sized> Clone for JobPublisher<P> {
    fn clone(&self) -> Self {
        Self {
            events: Arc::clone(&self.events),
            correlations: self.correlations.clone(),
        }
    }
}

impl<P: ?Sized> fmt::Debug for JobPublisher<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobPublisher")
            .field("events_type", &type_name::<P>())
            .field("correlations", &self.correlations)
            .finish()
    }
}

impl<P> JobPublisher<P>
where
    P: JobEventPublisher + ?Sized,
{
    pub fn new(events: Arc<P>, correlations: CorrelationCache) -> Self {
        Self {
            events,
            correlations,
        }
    }

    async fn correlation_for_event(
        &self,
        handle: &JobHandle,
        request: &EnqueueRequest,
    ) -> Option<Uuid> {
        if handle.accepted {
            request.correlation_id
        } else if let Some(existing) = handle.merged_into {
            self.correlations
                .fetch(&existing)
                .await
                .or(request.correlation_id)
        } else {
            request.correlation_id
        }
    }

    fn payload_for_handle(handle: &JobHandle) -> Result<JobEventPayload> {
        if handle.accepted {
            return Ok(JobEventPayload::Enqueued {
                job_id: handle.job_id,
                kind: handle.kind,
                priority: handle.priority,
            });
        }

        let Some(existing_job_id) = handle.merged_into else {
            return Err(MediaError::Internal(format!(
                "queue returned non-accepted handle without merge target for job {}",
                handle.job_id
            )));
        };

        Ok(JobEventPayload::Merged {
            existing_job_id,
            merged_job_id: handle.job_id,
            kind: handle.kind,
            priority: handle.priority,
        })
    }

    async fn remember_correlation(
        &self,
        handle: &JobHandle,
        event_correlation_id: Uuid,
    ) {
        if handle.accepted {
            self.correlations
                .remember(handle.job_id, event_correlation_id)
                .await;
            return;
        }

        if let Some(existing_job_id) = handle.merged_into {
            self.correlations
                .remember_if_absent(existing_job_id, event_correlation_id)
                .await;
        }

        if handle.merged_into != Some(handle.job_id) {
            self.correlations
                .remember_if_absent(handle.job_id, event_correlation_id)
                .await;
        }
    }

    pub async fn publish_enqueue_result(
        &self,
        handle: &JobHandle,
        request: &EnqueueRequest,
    ) -> Result<()> {
        let event_payload = Self::payload_for_handle(handle)?;
        let correlation_id = self.correlation_for_event(handle, request).await;
        let event = JobEvent::from_handle(
            handle,
            correlation_id,
            event_payload,
            stable_path_key(&request.payload),
        );

        self.remember_correlation(handle, event.meta.correlation_id)
            .await;
        self.events.publish(event).await
    }

    pub async fn publish_enqueue_results(
        &self,
        handles: &[JobHandle],
        requests: &[EnqueueRequest],
    ) -> Result<()> {
        if handles.len() != requests.len() {
            return Err(MediaError::Internal(format!(
                "queue returned {} handles for {} enqueue requests",
                handles.len(),
                requests.len()
            )));
        }

        for (handle, request) in handles.iter().zip(requests.iter()) {
            self.publish_enqueue_result(handle, request).await?;
        }

        Ok(())
    }
}

/// Enqueue facade for scan pipeline producers.
///
/// The durable queue decides whether a request is accepted or merged; this
/// boundary interprets the returned handle, records enqueue correlation, derives
/// stable metadata keys from the original request, and publishes the matching
/// `JobEvent` frame for every production enqueue path.
pub struct PipelineEnqueuer<Q: ?Sized, P: ?Sized> {
    queue: Arc<Q>,
    publisher: JobPublisher<P>,
}

impl<Q: ?Sized, P: ?Sized> Clone for PipelineEnqueuer<Q, P> {
    fn clone(&self) -> Self {
        Self {
            queue: Arc::clone(&self.queue),
            publisher: self.publisher.clone(),
        }
    }
}

impl<Q: ?Sized, P: ?Sized> fmt::Debug for PipelineEnqueuer<Q, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipelineEnqueuer")
            .field("queue_type", &type_name::<Q>())
            .field("publisher", &self.publisher)
            .finish()
    }
}

impl<Q, P> PipelineEnqueuer<Q, P>
where
    Q: QueueService + ?Sized,
    P: JobEventPublisher + ?Sized,
{
    pub fn new(
        queue: Arc<Q>,
        events: Arc<P>,
        correlations: CorrelationCache,
    ) -> Self {
        Self {
            queue,
            publisher: JobPublisher::new(events, correlations),
        }
    }

    pub async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
        let handle = self.queue.enqueue(request.clone()).await?;
        self.publisher
            .publish_enqueue_result(&handle, &request)
            .await?;
        Ok(handle)
    }

    pub async fn enqueue_many(
        &self,
        requests: Vec<EnqueueRequest>,
    ) -> Result<Vec<JobHandle>> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }

        let handles = self.queue.enqueue_many(requests.clone()).await?;
        self.publisher
            .publish_enqueue_results(&handles, &requests)
            .await?;
        Ok(handles)
    }

    /// Release queued jobs that were blocked on a scan dependency.
    pub async fn release_dependency(
        &self,
        library_id: LibraryId,
        dependency_key: &DependencyKey,
    ) -> Result<u64> {
        self.queue
            .release_dependency(library_id, dependency_key)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;

    use async_trait::async_trait;
    use ferrex_model::{ImageSize, MediaID, VideoMediaType};
    use tokio::sync::Mutex;

    use crate::domain::scan::orchestration::{
        context::{
            EpisodeHint, EpisodeLink, EpisodeScanHierarchy, SeasonLink,
            SeriesHint, SeriesLink, SeriesRootPath,
        },
        job::{
            DependencyKey, EpisodeMatchJob, ImageFetchJob, ImageFetchPriority,
            JobId, JobKind, JobPayload, JobPriority, MediaFingerprint,
            ScanReason,
        },
        lease::{DequeueRequest, JobLease, LeaseRenewal},
    };
    use crate::types::LibraryId;

    #[derive(Default)]
    struct RecordingPublisher {
        events: Mutex<Vec<JobEvent>>,
    }

    #[async_trait]
    impl JobEventPublisher for RecordingPublisher {
        async fn publish(&self, event: JobEvent) -> Result<()> {
            self.events.lock().await.push(event);
            Ok(())
        }
    }

    impl RecordingPublisher {
        async fn events(&self) -> Vec<JobEvent> {
            self.events.lock().await.clone()
        }
    }

    #[derive(Default)]
    struct RecordingQueue {
        enqueue_results: Mutex<VecDeque<JobHandle>>,
        enqueue_many_results: Mutex<VecDeque<Vec<JobHandle>>>,
        single_requests: Mutex<Vec<EnqueueRequest>>,
        batch_requests: Mutex<Vec<Vec<EnqueueRequest>>>,
    }

    impl RecordingQueue {
        async fn push_enqueue_result(&self, handle: JobHandle) {
            self.enqueue_results.lock().await.push_back(handle);
        }

        async fn push_enqueue_many_result(&self, handles: Vec<JobHandle>) {
            self.enqueue_many_results.lock().await.push_back(handles);
        }

        async fn single_requests(&self) -> Vec<EnqueueRequest> {
            self.single_requests.lock().await.clone()
        }

        async fn batch_requests(&self) -> Vec<Vec<EnqueueRequest>> {
            self.batch_requests.lock().await.clone()
        }
    }

    #[async_trait]
    impl QueueService for RecordingQueue {
        async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
            self.single_requests.lock().await.push(request.clone());
            if let Some(handle) = self.enqueue_results.lock().await.pop_front()
            {
                return Ok(handle);
            }

            Ok(JobHandle::accepted(
                JobId::new(),
                &request.payload,
                request.priority,
            ))
        }

        async fn dequeue(
            &self,
            _request: DequeueRequest,
        ) -> Result<Option<JobLease>> {
            Ok(None)
        }

        async fn renew(&self, renewal: LeaseRenewal) -> Result<JobLease> {
            Err(MediaError::Internal(format!(
                "recording queue cannot renew lease {:?}",
                renewal.lease_id
            )))
        }

        async fn complete(
            &self,
            _lease_id: crate::domain::scan::orchestration::lease::LeaseId,
        ) -> Result<()> {
            Ok(())
        }

        async fn fail(
            &self,
            _lease_id: crate::domain::scan::orchestration::lease::LeaseId,
            _retryable: bool,
            _error: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn dead_letter(
            &self,
            _lease_id: crate::domain::scan::orchestration::lease::LeaseId,
            _error: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn cancel_job(&self, _job_id: JobId) -> Result<()> {
            Ok(())
        }

        async fn queue_depth(&self, _kind: JobKind) -> Result<usize> {
            Ok(0)
        }

        async fn release_dependency(
            &self,
            _library_id: LibraryId,
            _dependency_key: &DependencyKey,
        ) -> Result<u64> {
            Ok(0)
        }

        async fn enqueue_many(
            &self,
            requests: Vec<EnqueueRequest>,
        ) -> Result<Vec<JobHandle>> {
            self.batch_requests.lock().await.push(requests.clone());
            if let Some(handles) =
                self.enqueue_many_results.lock().await.pop_front()
            {
                return Ok(handles);
            }

            Ok(requests
                .iter()
                .map(|request| {
                    JobHandle::accepted(
                        JobId::new(),
                        &request.payload,
                        request.priority,
                    )
                })
                .collect())
        }
    }

    fn library_id() -> LibraryId {
        LibraryId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa))
    }

    fn image_payload(image_id: Uuid) -> JobPayload {
        JobPayload::ImageFetch(ImageFetchJob {
            library_id: library_id(),
            iid: image_id,
            imz: ImageSize::poster(),
            priority_hint: ImageFetchPriority::Poster,
        })
    }

    fn image_request(priority: JobPriority, image_id: Uuid) -> EnqueueRequest {
        EnqueueRequest::new(priority, image_payload(image_id))
    }

    fn episode_request() -> EnqueueRequest {
        let series_root = SeriesRootPath::try_new("/shows/Demo Show")
            .expect("valid series root");
        let dependency_key = DependencyKey::series_root(&series_root);
        let job = EpisodeMatchJob {
            library_id: library_id(),
            media_id: MediaID::new(VideoMediaType::Episode),
            path_norm: "/shows/Demo Show/Season 1/S01E01.mkv".into(),
            fingerprint: MediaFingerprint {
                device_id: None,
                inode: Some(1),
                size: 100,
                mtime: 1_700_000_000,
                weak_hash: Some("episode-1".into()),
            },
            hierarchy: EpisodeScanHierarchy {
                series_root_path: series_root,
                series: SeriesLink::Hint(SeriesHint {
                    title: "Demo Show".into(),
                    slug: Some("demo-show".into()),
                    year: None,
                    region: None,
                }),
                season: SeasonLink::Number(1),
                episode: EpisodeLink::Hint(EpisodeHint {
                    number: 1,
                    title: Some("Pilot".into()),
                }),
            },
            node: crate::domain::scan::orchestration::context::ScanNodeKind::EpisodeFile,
            scan_reason: ScanReason::BulkSeed,
        };

        EnqueueRequest::new(JobPriority::P1, JobPayload::EpisodeMatch(job))
            .with_dependency(dependency_key)
    }

    fn enqueuer(
        queue: Arc<RecordingQueue>,
        publisher: Arc<RecordingPublisher>,
        correlations: CorrelationCache,
    ) -> PipelineEnqueuer<RecordingQueue, RecordingPublisher> {
        PipelineEnqueuer::new(queue, publisher, correlations)
    }

    #[tokio::test]
    async fn enqueue_publishes_accepted_event_and_caches_correlation() {
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let correlations = CorrelationCache::default();
        let pipeline = enqueuer(
            Arc::clone(&queue),
            Arc::clone(&publisher),
            correlations.clone(),
        );
        let image_id = Uuid::from_u128(0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb);
        let request = image_request(JobPriority::P1, image_id);
        let handle = JobHandle::accepted(
            JobId(Uuid::from_u128(0x11111111111111111111111111111111)),
            &request.payload,
            request.priority,
        );
        queue.push_enqueue_result(handle).await;

        let handle = pipeline.enqueue(request).await.expect("enqueue");

        let events = publisher.events().await;
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].payload,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::ImageFetch,
                priority: JobPriority::P1,
            } if job_id == handle.job_id
        ));
        assert_eq!(events[0].meta.idempotency_key, handle.dedupe_key);
        assert_eq!(
            events[0]
                .meta
                .path_key
                .as_ref()
                .map(|key| key.as_str().to_string()),
            Some(image_id.to_string())
        );
        assert_eq!(
            correlations.fetch(&handle.job_id).await,
            Some(events[0].meta.correlation_id)
        );
    }

    #[tokio::test]
    async fn merged_enqueue_uses_existing_correlation_and_reports_priority() {
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let correlations = CorrelationCache::default();
        let existing_job =
            JobId(Uuid::from_u128(0x22222222222222222222222222222222));
        let existing_correlation =
            Uuid::from_u128(0x33333333333333333333333333333333);
        correlations
            .remember(existing_job, existing_correlation)
            .await;
        let pipeline = enqueuer(
            Arc::clone(&queue),
            Arc::clone(&publisher),
            correlations.clone(),
        );
        let request = image_request(
            JobPriority::P0,
            Uuid::from_u128(0x44444444444444444444444444444444),
        );
        queue
            .push_enqueue_result(JobHandle::merged(
                existing_job,
                &request.payload,
                JobPriority::P0,
            ))
            .await;

        let handle = pipeline.enqueue(request).await.expect("enqueue");

        assert!(!handle.accepted);
        let events = publisher.events().await;
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].payload,
            JobEventPayload::Merged {
                existing_job_id,
                merged_job_id,
                kind: JobKind::ImageFetch,
                priority: JobPriority::P0,
            } if existing_job_id == existing_job && merged_job_id == existing_job
        ));
        assert_eq!(events[0].meta.correlation_id, existing_correlation);
        assert_eq!(
            correlations.fetch(&existing_job).await,
            Some(existing_correlation)
        );
    }

    #[tokio::test]
    async fn merged_enqueue_backfills_missing_correlation_from_request() {
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let correlations = CorrelationCache::default();
        let pipeline = enqueuer(
            Arc::clone(&queue),
            Arc::clone(&publisher),
            correlations.clone(),
        );
        let existing_job =
            JobId(Uuid::from_u128(0x55555555555555555555555555555555));
        let requested_correlation =
            Uuid::from_u128(0x66666666666666666666666666666666);
        let mut request = image_request(
            JobPriority::P2,
            Uuid::from_u128(0x77777777777777777777777777777777),
        );
        request.correlation_id = Some(requested_correlation);
        queue
            .push_enqueue_result(JobHandle::merged(
                existing_job,
                &request.payload,
                request.priority,
            ))
            .await;

        pipeline.enqueue(request).await.expect("enqueue");

        let events = publisher.events().await;
        assert_eq!(events[0].meta.correlation_id, requested_correlation);
        assert_eq!(
            correlations.fetch(&existing_job).await,
            Some(requested_correlation)
        );
    }

    #[tokio::test]
    async fn enqueue_preserves_dependency_key_for_episode_match() {
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let correlations = CorrelationCache::default();
        let pipeline =
            enqueuer(Arc::clone(&queue), Arc::clone(&publisher), correlations);
        let request = episode_request();
        let expected_dependency = request.dependency_key.clone();

        pipeline.enqueue(request).await.expect("enqueue");

        let recorded = queue.single_requests().await;
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].dependency_key, expected_dependency);
    }

    #[tokio::test]
    async fn enqueue_many_delegates_batch_and_publishes_each_event() {
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let correlations = CorrelationCache::default();
        let pipeline =
            enqueuer(Arc::clone(&queue), Arc::clone(&publisher), correlations);
        let request_a = image_request(
            JobPriority::P1,
            Uuid::from_u128(0x88888888888888888888888888888888),
        );
        let request_b = image_request(
            JobPriority::P0,
            Uuid::from_u128(0x99999999999999999999999999999999),
        );
        let accepted = JobHandle::accepted(
            JobId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1)),
            &request_a.payload,
            request_a.priority,
        );
        let merged = JobHandle::merged(
            JobId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2)),
            &request_b.payload,
            request_b.priority,
        );
        queue
            .push_enqueue_many_result(vec![accepted.clone(), merged.clone()])
            .await;

        let handles = pipeline
            .enqueue_many(vec![request_a.clone(), request_b.clone()])
            .await
            .expect("enqueue many");

        assert_eq!(handles.len(), 2);
        let batches = queue.batch_requests().await;
        assert_eq!(batches.len(), 1, "batch enqueue should delegate once");
        assert_eq!(batches[0].len(), 2);
        assert_eq!(batches[0][0].dedupe_key(), request_a.dedupe_key());
        assert_eq!(batches[0][1].dedupe_key(), request_b.dedupe_key());

        let events = publisher.events().await;
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0].payload,
            JobEventPayload::Enqueued { .. }
        ));
        assert!(matches!(events[1].payload, JobEventPayload::Merged { .. }));
    }
}
