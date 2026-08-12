use std::{collections::HashMap, sync::Arc, time::Duration};

use ferrex_core::{
    application::unit_of_work::AppUnitOfWork,
    types::{LibraryId, MovieBatchId},
};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};
use tracing::{info, warn};

use super::catalog_event_projection::CatalogEventProjection;

const MOVIE_BATCH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const MOVIE_BATCH_FINAL_DRAIN_RETRY: Duration = Duration::from_millis(25);
const MOVIE_BATCH_FINAL_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MovieBatchDrainStatus {
    Complete,
    Pending,
}

#[derive(Debug)]
struct LibraryNotifier {
    active_runs: usize,
    stop_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// Tracks active scans per library and emits `MediaEvent::MovieBatchFinalized`
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
        catalog_events: CatalogEventProjection,
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
            catalog_events,
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
        drop(guard);
        if let Err(err) = notifier.task.await {
            warn!(
                library = %library_id,
                error = %err,
                "movie batch finalization task failed during final drain"
            );
        }
    }

    /// Stop notifier work immediately when its library has been deleted.
    pub async fn forget_library(&self, library_id: LibraryId) {
        let notifier = self.libraries.lock().await.remove(&library_id);
        if let Some(notifier) = notifier {
            let _ = notifier.stop_tx.send(true);
            notifier.task.abort();
        }
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
    catalog_events: CatalogEventProjection,
    mut stop_rx: watch::Receiver<bool>,
    mut last_notified: Option<MovieBatchId>,
) {
    let mut ticker = time::interval(MOVIE_BATCH_POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        let stopping = tokio::select! {
            result = stop_rx.changed() => {
                result.is_err() || *stop_rx.borrow()
            }
            _ = ticker.tick() => false,
        };

        let status = drain_ready_movie_batches(
            library_id,
            &unit_of_work,
            &catalog_events,
            &mut last_notified,
        )
        .await;

        if !stopping {
            continue;
        }

        let deadline = time::Instant::now() + MOVIE_BATCH_FINAL_DRAIN_TIMEOUT;
        let mut status = status;
        while status == MovieBatchDrainStatus::Pending
            && time::Instant::now() < deadline
        {
            time::sleep(MOVIE_BATCH_FINAL_DRAIN_RETRY).await;
            status = drain_ready_movie_batches(
                library_id,
                &unit_of_work,
                &catalog_events,
                &mut last_notified,
            )
            .await;
        }
        if status == MovieBatchDrainStatus::Pending {
            warn!(
                library = %library_id,
                "movie batch finalization remained pending after final drain"
            );
        }
        break;
    }
}

async fn drain_ready_movie_batches(
    library_id: LibraryId,
    unit_of_work: &AppUnitOfWork,
    catalog_events: &CatalogEventProjection,
    last_notified: &mut Option<MovieBatchId>,
) -> MovieBatchDrainStatus {
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
            return MovieBatchDrainStatus::Pending;
        }
    };

    for batch_id in finalized {
        if let Some(last) = *last_notified
            && batch_id <= last
        {
            continue;
        }

        let receivers = catalog_events.receiver_count();
        let frame = match catalog_events
            .publish_movie_batch_finalized(library_id, batch_id)
            .await
        {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                tracing::debug!(
                    library = %library_id,
                    batch_id = %batch_id,
                    "deferring movie batch finalization until per-item projections catch up"
                );
                return MovieBatchDrainStatus::Pending;
            }
            Err(err) => {
                warn!(
                    "movie batch finalization projection failed (library {}, batch {}): {}",
                    library_id, batch_id, err
                );
                return MovieBatchDrainStatus::Pending;
            }
        };
        info!(
            library = %library_id,
            batch_id = %batch_id,
            receivers = receivers,
            sequence = frame.sequence,
            "published movie batch finalization"
        );

        *last_notified = Some(batch_id);
    }

    MovieBatchDrainStatus::Complete
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn run_finish_waits_for_notifier_final_drain() {
        let library_id = LibraryId::new();
        let notifiers = MovieBatchFinalizationNotifiers::new();
        let drained = Arc::new(AtomicBool::new(false));
        let drained_by_task = Arc::clone(&drained);
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            stop_rx.changed().await.expect("stop sender remains alive");
            assert!(*stop_rx.borrow());
            tokio::task::yield_now().await;
            drained_by_task.store(true, Ordering::SeqCst);
        });

        notifiers.libraries.lock().await.insert(
            library_id,
            LibraryNotifier {
                active_runs: 1,
                stop_tx,
                task,
            },
        );

        notifiers.on_run_finished(library_id).await;

        assert!(drained.load(Ordering::SeqCst));
        assert!(!notifiers.libraries.lock().await.contains_key(&library_id));
    }
}
