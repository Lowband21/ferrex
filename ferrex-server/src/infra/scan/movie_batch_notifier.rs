use std::{collections::HashMap, sync::Arc, time::Duration};

use ferrex_core::{
    api::types::MovieReferenceBatchResponse,
    application::unit_of_work::AppUnitOfWork,
    types::{LibraryId, MediaEvent, MovieBatchId},
};
use sha2::Digest;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};
use tracing::{info, warn};

use super::media_event_bus::MediaEventBus;

const MOVIE_BATCH_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug)]
struct LibraryNotifier {
    active_runs: usize,
    stop_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// Tracks active scans per library and emits `MediaEvent::MovieReferenceBatchFinalized`
/// for newly-finalized movie reference batches.
///
/// This is intentionally polling-based (via `list_finalized_movie_reference_batches`)
/// to avoid coupling scan orchestration to database triggers/NOTIFY plumbing.
#[derive(Debug, Default)]
pub struct MovieBatchFinalizationNotifiers {
    libraries: Mutex<HashMap<LibraryId, LibraryNotifier>>,
}

impl MovieBatchFinalizationNotifiers {
    pub fn new() -> Self {
        Self {
            libraries: Mutex::new(HashMap::new()),
        }
    }

    pub async fn on_run_started(
        &self,
        library_id: LibraryId,
        unit_of_work: Arc<AppUnitOfWork>,
        media_bus: Arc<MediaEventBus>,
    ) {
        let mut guard = self.libraries.lock().await;
        if let Some(notifier) = guard.get_mut(&library_id) {
            notifier.active_runs += 1;
            return;
        }

        let (stop_tx, stop_rx) = watch::channel(false);
        let initial_last_finalized =
            fetch_last_finalized_batch_id(&unit_of_work, &library_id).await;
        let task = tokio::spawn(movie_batch_notifier_loop(
            library_id,
            unit_of_work,
            media_bus,
            stop_rx,
            initial_last_finalized,
        ));

        guard.insert(
            library_id,
            LibraryNotifier {
                active_runs: 1,
                stop_tx,
                task,
            },
        );
    }

    pub async fn on_run_finished(&self, library_id: LibraryId) {
        let mut guard = self.libraries.lock().await;
        let Some(mut notifier) = guard.remove(&library_id) else {
            return;
        };

        notifier.active_runs = notifier.active_runs.saturating_sub(1);
        if notifier.active_runs > 0 {
            guard.insert(library_id, notifier);
            return;
        }

        let _ = notifier.stop_tx.send(true);
        notifier.task.abort();
    }
}

async fn fetch_last_finalized_batch_id(
    unit_of_work: &AppUnitOfWork,
    library_id: &LibraryId,
) -> Option<MovieBatchId> {
    match unit_of_work
        .media_refs
        .list_finalized_movie_reference_batches(library_id)
        .await
    {
        Ok(batch_ids) => batch_ids.last().copied(),
        Err(err) => {
            warn!(
                "failed to fetch initial finalized movie batches for library {}: {}",
                library_id, err
            );
            None
        }
    }
}

async fn movie_batch_notifier_loop(
    library_id: LibraryId,
    unit_of_work: Arc<AppUnitOfWork>,
    media_bus: Arc<MediaEventBus>,
    mut stop_rx: watch::Receiver<bool>,
    mut last_notified: Option<MovieBatchId>,
) {
    let mut ticker = time::interval(MOVIE_BATCH_POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    break;
                }
            }
            _ = ticker.tick() => {}
        }

        let finalized = match unit_of_work
            .media_refs
            .list_finalized_movie_reference_batches(&library_id)
            .await
        {
            Ok(batch_ids) => batch_ids,
            Err(err) => {
                warn!(
                    "failed to list finalized movie batches for library {}: {}",
                    library_id, err
                );
                continue;
            }
        };

        if finalized.is_empty() {
            continue;
        }

        for batch_id in finalized {
            if let Some(last) = last_notified
                && batch_id <= last
            {
                continue;
            }

            // Keep the server-side hash/version record in sync as batches are
            // finalized by the scan pipeline (not only when a client downloads).
            if let Err(err) =
                upsert_movie_batch_hash(&unit_of_work, &library_id, batch_id)
                    .await
            {
                warn!(
                    "movie batch versioning upsert failed (library {}, batch {}): {}",
                    library_id, batch_id, err
                );
            }

            let receivers = media_bus.receiver_count();
            let frame = media_bus.publish(MediaEvent::MovieBatchFinalized {
                library_id,
                batch_id,
            });
            info!(
                library = %library_id,
                batch_id = %batch_id,
                receivers = receivers,
                sequence = frame.sequence,
                "published movie batch finalization"
            );

            last_notified = Some(batch_id);
        }
    }
}

async fn upsert_movie_batch_hash(
    unit_of_work: &AppUnitOfWork,
    library_id: &LibraryId,
    batch_id: MovieBatchId,
) -> anyhow::Result<()> {
    let movies = unit_of_work
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

    unit_of_work
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
