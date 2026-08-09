use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use ferrex_core::api::types::ScanRunMode;
use ferrex_core::domain::scan::orchestration::{
    DurableJobState, JobEvent, JobEventPayload, JobId, JobKind,
    MaintenanceLibrary, MaintenancePlanningLimits, config::MaintenanceConfig,
    plan_maintenance_sweep,
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
    enrolling: bool,
    reconciliation_required: bool,
    started_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunOutcome {
    Success,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReconcileStatus {
    Missing,
    Active,
    Finished,
}

impl InFlightRun {
    fn new(
        correlation_id: Uuid,
        failed: bool,
        started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            correlation_id,
            pending_jobs: HashSet::new(),
            failed,
            enrolling: true,
            reconciliation_required: true,
            started_at,
        }
    }

    fn track_job(&mut self, job_id: JobId) {
        self.pending_jobs.insert(job_id);
    }

    fn mark_failed(&mut self) {
        self.failed = true;
    }

    fn observe_terminal(
        &mut self,
        job_id: JobId,
        terminal_failure: bool,
    ) -> Option<RunOutcome> {
        if !self.pending_jobs.remove(&job_id) {
            return None;
        }
        self.failed |= terminal_failure;
        self.outcome_if_finished()
    }

    fn finish_enrollment(&mut self) {
        self.enrolling = false;
    }

    fn require_reconciliation(&mut self) {
        self.reconciliation_required = true;
    }

    fn reconcile(&mut self, jobs: &[DurableJobState]) -> Option<RunOutcome> {
        // Discover downstream folder jobs carrying this run correlation, then
        // reconcile all known IDs. Explicitly tracked merged jobs are retained
        // even when PostgreSQL kept their older correlation.
        for job in jobs {
            if job.kind == JobKind::FolderScan
                && job.correlation_id == Some(self.correlation_id)
            {
                self.pending_jobs.insert(job.job_id);
            }
        }

        for job in jobs {
            if self.pending_jobs.contains(&job.job_id) && job.is_terminal() {
                self.pending_jobs.remove(&job.job_id);
                self.failed |= job.is_terminal_failure();
            }
        }

        self.reconciliation_required = false;
        self.outcome_if_finished()
    }

    fn outcome_if_finished(&self) -> Option<RunOutcome> {
        if self.enrolling
            || self.reconciliation_required
            || !self.pending_jobs.is_empty()
        {
            return None;
        }
        Some(if self.failed {
            RunOutcome::Failed
        } else {
            RunOutcome::Success
        })
    }
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
                    self.reconcile_required_runs("scheduled_retry").await;
                    self.expire_stalled_runs().await;
                    self.schedule_due_libraries().await;
                }
                event = events.recv() => {
                    match event {
                        Ok(event) => self.observe_job_event(event).await,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!(skipped, "maintenance scheduler lagged job events");
                            {
                                let mut guard = self.in_flight.lock().await;
                                for run in guard.values_mut() {
                                    run.require_reconciliation();
                                }
                            }
                            self.reconcile_all_runs("job_event_lag").await;
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

            let plan = match plan_maintenance_sweep(
                &maintenance_library,
                self.orchestrator.cursors.as_ref(),
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

            if plan.root_discovery_truncated {
                self.orchestrator.record_root_discovery_truncation(
                    plan.deferred_root_entries,
                );
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
        let initially_failed = plan.has_errors();
        let mut accepted = 0usize;
        let mut merged = 0usize;

        {
            let mut guard = self.in_flight.lock().await;
            if guard.contains_key(&library_id) {
                // Preserve the existing run; queue dedupe is only a secondary
                // guard and must not replace its enrollment state.
                return;
            }
            guard.insert(
                library_id,
                InFlightRun::new(correlation_id, initially_failed, now),
            );
        }

        for mut request in plan.requests {
            request.correlation_id = Some(correlation_id);
            match self.orchestrator.enqueue(request).await {
                Ok(handle) => {
                    if handle.accepted {
                        accepted += 1;
                    } else {
                        merged += 1;
                    }
                    let mut guard = self.in_flight.lock().await;
                    if let Some(run) = guard.get_mut(&library_id)
                        && run.correlation_id == correlation_id
                    {
                        run.track_job(handle.job_id);
                    }
                }
                Err(err) => {
                    let mut guard = self.in_flight.lock().await;
                    if let Some(run) = guard.get_mut(&library_id)
                        && run.correlation_id == correlation_id
                    {
                        run.mark_failed();
                    }
                    warn!(
                        library = %library_id,
                        error = %err,
                        "failed to enqueue maintenance sweep job"
                    );
                }
            }
        }

        let pending = {
            let mut guard = self.in_flight.lock().await;
            let Some(run) = guard.get_mut(&library_id) else {
                return;
            };
            if run.correlation_id != correlation_id {
                return;
            }
            run.finish_enrollment();
            run.pending_jobs.len()
        };

        info!(
            library = %library_id,
            correlation = %correlation_id,
            accepted,
            merged,
            pending,
            stale_cursors = plan.stale_cursor_count,
            new_root_folders = plan.new_root_folder_count,
            skipped_root_entries = plan.skipped_root_entries,
            root_discovery_truncated = plan.root_discovery_truncated,
            deferred_root_entries = plan.deferred_root_entries,
            plan_errors = plan.errors.len(),
            "scheduled incremental maintenance sweep"
        );

        // Jobs may have reached a terminal PostgreSQL state while enqueue was
        // still returning (or their terminal events may already have fallen
        // out of the bounded broadcast channel). Reconcile before waiting.
        if let Err(err) =
            self.reconcile_library(library_id, "post_enqueue").await
        {
            warn!(
                library = %library_id,
                correlation = %correlation_id,
                error = %err,
                "failed to reconcile maintenance jobs after enqueue"
            );
        }
    }

    async fn observe_job_event(&self, event: JobEvent) {
        let library_id = event.meta.library_id;
        let event_correlation = event.meta.correlation_id;
        let (job_id, terminal_failure) = match event.payload {
            JobEventPayload::Enqueued {
                kind: JobKind::FolderScan,
                job_id,
                ..
            }
            | JobEventPayload::Merged {
                kind: JobKind::FolderScan,
                existing_job_id: job_id,
                ..
            } => {
                let mut guard = self.in_flight.lock().await;
                if let Some(run) = guard.get_mut(&library_id)
                    && run.correlation_id == event_correlation
                {
                    run.track_job(job_id);
                }
                return;
            }
            JobEventPayload::Completed {
                kind: JobKind::FolderScan,
                job_id,
                ..
            } => (job_id, false),
            JobEventPayload::DeadLettered {
                kind: JobKind::FolderScan,
                job_id,
                ..
            } => (job_id, true),
            JobEventPayload::Failed {
                kind: JobKind::FolderScan,
                job_id,
                retryable: false,
                ..
            } => (job_id, true),
            JobEventPayload::Failed {
                kind: JobKind::FolderScan,
                ..
            } => return,
            _ => return,
        };

        let finished = {
            let mut guard = self.in_flight.lock().await;
            let Some(run) = guard.get_mut(&library_id) else {
                return;
            };

            let outcome = run.observe_terminal(job_id, terminal_failure);
            let correlation_id = run.correlation_id;
            if outcome.is_some() {
                guard.remove(&library_id);
            }
            outcome.map(|outcome| (correlation_id, outcome))
        };

        if let Some((correlation_id, outcome)) = finished {
            self.finish_run(library_id, correlation_id, outcome).await;
        }
    }

    async fn expire_stalled_runs(&self) {
        let now = Utc::now();
        let stall = ChronoDuration::milliseconds(
            self.config.run_stall_timeout_ms.max(1) as i64,
        );
        let expired = {
            let guard = self.in_flight.lock().await;
            guard
                .iter()
                .filter_map(|(library_id, run)| {
                    (now.signed_duration_since(run.started_at) >= stall)
                        .then_some((*library_id, run.correlation_id))
                })
                .collect::<Vec<_>>()
        };

        for (library_id, correlation_id) in expired {
            match self.reconcile_library(library_id, "pre_stall").await {
                Ok(ReconcileStatus::Missing | ReconcileStatus::Finished) => {
                    continue;
                }
                Ok(ReconcileStatus::Active) => {}
                Err(err) => {
                    warn!(
                        library = %library_id,
                        correlation = %correlation_id,
                        error = %err,
                        "durable maintenance reconciliation failed; deferring stall decision"
                    );
                    continue;
                }
            }

            let removed = {
                let mut guard = self.in_flight.lock().await;
                let still_expired = guard
                    .get(&library_id)
                    .map(|run| {
                        run.correlation_id == correlation_id
                            && now.signed_duration_since(run.started_at)
                                >= stall
                    })
                    .unwrap_or(false);
                still_expired.then(|| guard.remove(&library_id)).flatten()
            };
            if removed.is_none() {
                continue;
            }

            warn!(
                library = %library_id,
                correlation = %correlation_id,
                "maintenance sweep stalled before terminal state; last_scan not updated"
            );
            self.backoff(library_id, now).await;
        }
    }

    async fn reconcile_all_runs(&self, reason: &'static str) {
        let libraries: Vec<LibraryId> =
            self.in_flight.lock().await.keys().copied().collect();
        for library_id in libraries {
            if let Err(err) = self.reconcile_library(library_id, reason).await {
                warn!(
                    library = %library_id,
                    error = %err,
                    reason,
                    "failed to reconcile maintenance run from durable job state"
                );
            }
        }
    }

    async fn reconcile_required_runs(&self, reason: &'static str) {
        let libraries: Vec<LibraryId> = self
            .in_flight
            .lock()
            .await
            .iter()
            .filter_map(|(library_id, run)| {
                run.reconciliation_required.then_some(*library_id)
            })
            .collect();
        for library_id in libraries {
            if let Err(err) = self.reconcile_library(library_id, reason).await {
                warn!(
                    library = %library_id,
                    error = %err,
                    reason,
                    "required durable maintenance reconciliation still pending"
                );
            }
        }
    }

    async fn reconcile_library(
        &self,
        library_id: LibraryId,
        reason: &'static str,
    ) -> ferrex_core::error::Result<ReconcileStatus> {
        let Some((correlation_id, pending_jobs)) = ({
            let guard = self.in_flight.lock().await;
            guard.get(&library_id).map(|run| {
                (
                    run.correlation_id,
                    run.pending_jobs.iter().copied().collect::<Vec<_>>(),
                )
            })
        }) else {
            return Ok(ReconcileStatus::Missing);
        };

        let durable = self
            .orchestrator
            .durable_job_states(correlation_id, &pending_jobs)
            .await?;
        tracing::debug!(
            library = %library_id,
            correlation = %correlation_id,
            tracked_jobs = pending_jobs.len(),
            durable_jobs = durable.len(),
            reason,
            "reconciling maintenance run from durable job state"
        );

        let finished = {
            let mut guard = self.in_flight.lock().await;
            let Some(run) = guard.get_mut(&library_id) else {
                return Ok(ReconcileStatus::Missing);
            };
            if run.correlation_id != correlation_id {
                return Ok(ReconcileStatus::Missing);
            }
            let outcome = run.reconcile(&durable);
            if outcome.is_some() {
                guard.remove(&library_id);
            }
            outcome
        };

        if let Some(outcome) = finished {
            self.finish_run(library_id, correlation_id, outcome).await;
            Ok(ReconcileStatus::Finished)
        } else {
            Ok(ReconcileStatus::Active)
        }
    }

    async fn finish_run(
        &self,
        library_id: LibraryId,
        correlation_id: Uuid,
        outcome: RunOutcome,
    ) {
        match outcome {
            RunOutcome::Success => {
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
            }
            RunOutcome::Failed => {
                warn!(
                    library = %library_id,
                    correlation = %correlation_id,
                    "maintenance sweep reached terminal state with errors; last_scan not updated"
                );
                self.backoff(library_id, Utc::now()).await;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::domain::scan::orchestration::JobState;

    fn durable_folder_job(
        correlation_id: Uuid,
        job_id: JobId,
        state: JobState,
    ) -> DurableJobState {
        DurableJobState {
            job_id,
            kind: JobKind::FolderScan,
            state,
            attempts: 0,
            dedupe_key: format!("scan:test:{job_id}"),
            correlation_id: Some(correlation_id),
            path_key: None,
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn immediate_completion_during_enqueue_requires_post_enqueue_reconcile() {
        let correlation_id = Uuid::now_v7();
        let job_id = JobId::new();
        let mut run = InFlightRun::new(correlation_id, false, Utc::now());

        // The terminal event can beat the enqueue handle, so it cannot remove
        // an ID that has not been enrolled yet.
        assert_eq!(run.observe_terminal(job_id, false), None);
        run.track_job(job_id);
        run.finish_enrollment();
        assert!(run.reconciliation_required);

        assert_eq!(
            run.reconcile(&[durable_folder_job(
                correlation_id,
                job_id,
                JobState::Completed,
            )]),
            Some(RunOutcome::Success)
        );
    }

    #[test]
    fn lag_gate_blocks_event_only_completion_until_durable_read_succeeds() {
        let correlation_id = Uuid::now_v7();
        let job_id = JobId::new();
        let mut run = InFlightRun::new(correlation_id, false, Utc::now());
        run.track_job(job_id);
        run.finish_enrollment();

        assert_eq!(
            run.reconcile(&[durable_folder_job(
                correlation_id,
                job_id,
                JobState::Leased,
            )]),
            None
        );
        assert!(!run.reconciliation_required);

        run.require_reconciliation();
        assert_eq!(run.observe_terminal(job_id, false), None);
        assert!(run.pending_jobs.is_empty());
        assert!(run.reconciliation_required);

        assert_eq!(
            run.reconcile(&[durable_folder_job(
                correlation_id,
                job_id,
                JobState::Completed,
            )]),
            Some(RunOutcome::Success)
        );
    }

    #[test]
    fn durable_terminal_failure_prevents_last_scan_success() {
        let correlation_id = Uuid::now_v7();
        let job_id = JobId::new();
        let mut run = InFlightRun::new(correlation_id, false, Utc::now());
        run.track_job(job_id);
        run.finish_enrollment();

        assert_eq!(
            run.reconcile(&[durable_folder_job(
                correlation_id,
                job_id,
                JobState::DeadLetter,
            )]),
            Some(RunOutcome::Failed)
        );
    }
}
