use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use ferrex_core::api::types::ScanRunMode;
use ferrex_core::database::repositories::manifest::PostgresManifestRepository;
use ferrex_core::database::repository_ports::scan_observability::ScanRunRetentionPolicy;
use ferrex_core::domain::scan::orchestration::{
    JobEvent, JobEventPayload, JobId, JobKind, MaintenanceLibrary,
    MaintenancePlanningLimits, config::MaintenanceConfig,
    plan_manifest_maintenance_sweep,
};
use ferrex_core::types::LibraryId;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{Duration, MissedTickBehavior, interval, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::ScanOrchestrator;

#[derive(Debug)]
pub(super) struct MaintenanceSchedulerHandle {
    token: CancellationToken,
    task: JoinHandle<()>,
}

impl MaintenanceSchedulerHandle {
    pub(super) async fn shutdown(self) {
        self.token.cancel();
        match timeout(Duration::from_secs(5), self.task).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!(error = %err, "maintenance scheduler task failed during shutdown")
            }
            Err(_) => warn!("maintenance scheduler timed out during shutdown"),
        }
    }
}

pub(super) fn spawn_maintenance_scheduler(
    orchestrator: Arc<ScanOrchestrator>,
    config: MaintenanceConfig,
) -> MaintenanceSchedulerHandle {
    let token = CancellationToken::new();
    let scheduler = Arc::new(MaintenanceScheduler {
        orchestrator,
        config,
        token: token.clone(),
        in_flight: Mutex::new(HashMap::new()),
        backoff_until: Mutex::new(HashMap::new()),
    });
    let task = tokio::spawn(async move { scheduler.run().await });
    MaintenanceSchedulerHandle { token, task }
}

#[derive(Debug)]
struct MaintenanceScheduler {
    orchestrator: Arc<ScanOrchestrator>,
    config: MaintenanceConfig,
    token: CancellationToken,
    in_flight: Mutex<HashMap<LibraryId, InFlightRun>>,
    backoff_until: Mutex<HashMap<LibraryId, DateTime<Utc>>>,
}

#[derive(Debug)]
struct InFlightRun {
    correlation_id: Uuid,
    pending_jobs: HashSet<JobId>,
    failed: bool,
    started_at: DateTime<Utc>,
}

impl MaintenanceScheduler {
    async fn run(self: Arc<Self>) {
        let mut ticker = interval(Duration::from_millis(
            self.config.tick_interval_ms.max(1),
        ));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut events = self.orchestrator.subscribe_job_events();

        loop {
            tokio::select! {
                _ = self.token.cancelled() => {
                    info!("maintenance scheduler shutting down");
                    break;
                }
                _ = ticker.tick() => {
                    self.expire_stalled_runs().await;
                    self.prune_retained_scan_runs(Utc::now()).await;
                    self.schedule_due_libraries().await;
                }
                event = events.recv() => {
                    match event {
                        Ok(event) => self.observe_job_event(event).await,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!(skipped, "maintenance scheduler lagged job events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    }

    async fn schedule_due_libraries(&self) {
        let libraries = match self
            .orchestrator
            .unit_of_work
            .libraries
            .list_libraries()
            .await
        {
            Ok(libraries) => libraries,
            Err(err) => {
                warn!(error = %err, "failed to list libraries for maintenance scheduling");
                return;
            }
        };

        let now = Utc::now();
        let limits = MaintenancePlanningLimits::new(
            self.config.max_jobs_per_library,
            self.config.max_root_entries_per_library,
        );

        for library in libraries {
            if self.token.is_cancelled() {
                break;
            }

            let maintenance_library =
                MaintenanceLibrary::from_library(&library);
            if self.is_in_flight(maintenance_library.id).await
                || self.is_backing_off(maintenance_library.id, now).await
                || self.manual_scan_active(maintenance_library.id).await
            {
                continue;
            }

            let manifest = PostgresManifestRepository::new(
                self.orchestrator.runtime.queue().pool().clone(),
            );
            let plan = match plan_manifest_maintenance_sweep(
                &maintenance_library,
                self.orchestrator.cursors.as_ref(),
                &manifest,
                limits,
                now,
            )
            .await
            {
                Ok(plan) => plan,
                Err(err) => {
                    warn!(
                        library = %maintenance_library.id,
                        error = %err,
                        "failed to plan maintenance sweep"
                    );
                    self.backoff(maintenance_library.id, now).await;
                    continue;
                }
            };

            if !plan.due {
                continue;
            }

            if plan.requests.is_empty() {
                if plan.has_errors() {
                    warn!(
                        library = %maintenance_library.id,
                        errors = ?plan.errors,
                        "maintenance sweep found no jobs but root discovery had errors"
                    );
                    self.backoff(maintenance_library.id, now).await;
                } else if let Err(err) =
                    self.mark_library_scanned(maintenance_library.id).await
                {
                    warn!(
                        library = %maintenance_library.id,
                        error = %err,
                        "failed to update maintenance last_scan for empty sweep"
                    );
                    self.backoff(maintenance_library.id, now).await;
                }
                continue;
            }

            self.enqueue_plan(maintenance_library.id, plan, now).await;
        }
    }

    async fn enqueue_plan(
        &self,
        library_id: LibraryId,
        plan: ferrex_core::domain::scan::orchestration::MaintenancePlan,
        now: DateTime<Utc>,
    ) {
        let correlation_id = Uuid::now_v7();
        let mut pending_jobs = HashSet::new();
        let mut failed = plan.has_errors();
        let mut accepted = 0usize;
        let mut merged = 0usize;

        for mut request in plan.requests {
            request.correlation_id = Some(correlation_id);
            match self.orchestrator.enqueue(request).await {
                Ok(handle) => {
                    if handle.accepted {
                        accepted += 1;
                    } else {
                        merged += 1;
                    }
                    pending_jobs.insert(handle.job_id);
                }
                Err(err) => {
                    failed = true;
                    warn!(
                        library = %library_id,
                        error = %err,
                        "failed to enqueue maintenance sweep job"
                    );
                }
            }
        }

        if pending_jobs.is_empty() {
            if failed {
                self.backoff(library_id, now).await;
            } else if let Err(err) = self.mark_library_scanned(library_id).await
            {
                warn!(
                    library = %library_id,
                    error = %err,
                    "failed to update maintenance last_scan after empty enqueue"
                );
                self.backoff(library_id, now).await;
            }
            return;
        }

        let mut guard = self.in_flight.lock().await;
        if guard.contains_key(&library_id) {
            // A concurrent terminal event may have scheduled a new run; avoid
            // replacing it. The queue dedupe still prevents duplicate floods.
            return;
        }

        info!(
            library = %library_id,
            correlation = %correlation_id,
            accepted,
            merged,
            pending = pending_jobs.len(),
            stale_cursors = plan.stale_cursor_count,
            new_root_folders = plan.new_root_folder_count,
            skipped_root_entries = plan.skipped_root_entries,
            plan_errors = plan.errors.len(),
            "scheduled incremental maintenance sweep"
        );

        guard.insert(
            library_id,
            InFlightRun {
                correlation_id,
                pending_jobs,
                failed,
                started_at: now,
            },
        );
    }

    async fn observe_job_event(&self, event: JobEvent) {
        let (job_id, terminal_failure) = match event.payload {
            JobEventPayload::Completed {
                kind: JobKind::FolderScan | JobKind::ManifestScan,
                job_id,
                ..
            } => (job_id, false),
            JobEventPayload::DeadLettered {
                kind: JobKind::FolderScan | JobKind::ManifestScan,
                job_id,
                ..
            } => (job_id, true),
            JobEventPayload::Failed {
                kind: JobKind::FolderScan | JobKind::ManifestScan,
                job_id,
                retryable: false,
                ..
            } => (job_id, true),
            JobEventPayload::Failed {
                kind: JobKind::FolderScan | JobKind::ManifestScan,
                ..
            } => return,
            _ => return,
        };

        let library_id = event.meta.library_id;
        let finished = {
            let mut guard = self.in_flight.lock().await;
            let Some(run) = guard.get_mut(&library_id) else {
                return;
            };

            if !run.pending_jobs.remove(&job_id) {
                return;
            }

            if terminal_failure {
                run.failed = true;
            }

            if run.pending_jobs.is_empty() {
                let run = guard.remove(&library_id).expect("run exists");
                Some((run.correlation_id, !run.failed))
            } else {
                None
            }
        };

        if let Some((correlation_id, success)) = finished {
            if success {
                match self.mark_library_scanned(library_id).await {
                    Ok(()) => info!(
                        library = %library_id,
                        correlation = %correlation_id,
                        "maintenance sweep reached terminal success"
                    ),
                    Err(err) => {
                        warn!(
                            library = %library_id,
                            correlation = %correlation_id,
                            error = %err,
                            "failed to update maintenance last_scan"
                        );
                        self.backoff(library_id, Utc::now()).await;
                    }
                }
            } else {
                warn!(
                    library = %library_id,
                    correlation = %correlation_id,
                    "maintenance sweep reached terminal state with errors; last_scan not updated"
                );
                self.backoff(library_id, Utc::now()).await;
            }
        }
    }

    async fn expire_stalled_runs(&self) {
        let now = Utc::now();
        let stall = ChronoDuration::milliseconds(
            self.config.run_stall_timeout_ms.max(1) as i64,
        );
        let expired = {
            let mut guard = self.in_flight.lock().await;
            let expired: Vec<_> = guard
                .iter()
                .filter_map(|(library_id, run)| {
                    (now.signed_duration_since(run.started_at) >= stall)
                        .then_some((*library_id, run.correlation_id))
                })
                .collect();
            for (library_id, _) in &expired {
                guard.remove(library_id);
            }
            expired
        };

        for (library_id, correlation_id) in expired {
            warn!(
                library = %library_id,
                correlation = %correlation_id,
                "maintenance sweep stalled before terminal state; last_scan not updated"
            );
            self.backoff(library_id, now).await;
        }
    }

    async fn prune_retained_scan_runs(&self, now: DateTime<Utc>) {
        let retention_days = self.config.scan_run_retention_days;
        if retention_days == 0 {
            return;
        }

        let terminal_before =
            now - ChronoDuration::days(i64::from(retention_days));
        match self
            .orchestrator
            .unit_of_work
            .scan_observability
            .prune(ScanRunRetentionPolicy { terminal_before })
            .await
        {
            Ok(0) => {}
            Ok(pruned_runs) => info!(
                pruned_runs,
                retention_days,
                terminal_before = %terminal_before,
                "pruned retained scan observability runs"
            ),
            Err(err) => warn!(
                error = %err,
                retention_days,
                terminal_before = %terminal_before,
                "failed to prune retained scan observability runs"
            ),
        }
    }

    async fn is_in_flight(&self, library_id: LibraryId) -> bool {
        self.in_flight.lock().await.contains_key(&library_id)
    }

    async fn manual_scan_active(&self, library_id: LibraryId) -> bool {
        match self
            .orchestrator
            .unit_of_work
            .scan_runs
            .load_active(library_id, ScanRunMode::Manual)
            .await
        {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(err) => {
                warn!(
                    library = %library_id,
                    error = %err,
                    "failed to inspect active manual scan before maintenance scheduling"
                );
                true
            }
        }
    }

    async fn is_backing_off(
        &self,
        library_id: LibraryId,
        now: DateTime<Utc>,
    ) -> bool {
        let mut guard = self.backoff_until.lock().await;
        match guard.get(&library_id).copied() {
            Some(until) if until > now => true,
            Some(_) => {
                guard.remove(&library_id);
                false
            }
            None => false,
        }
    }

    async fn backoff(&self, library_id: LibraryId, now: DateTime<Utc>) {
        let duration = ChronoDuration::milliseconds(
            self.config.error_backoff_ms.max(1) as i64,
        );
        let until = now + duration;
        self.backoff_until.lock().await.insert(library_id, until);
        debug!(library = %library_id, backoff_until = %until, "maintenance scheduler backed off library");
    }

    async fn mark_library_scanned(
        &self,
        library_id: LibraryId,
    ) -> ferrex_core::error::Result<()> {
        self.orchestrator
            .unit_of_work
            .libraries
            .update_library_last_scan(library_id)
            .await
    }
}
