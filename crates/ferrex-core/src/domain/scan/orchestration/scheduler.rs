use std::cmp::max;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::types::ids::LibraryId;

use super::config::{LibraryQueuePolicy, PriorityWeights, QueueConfig};
use super::job::{JobKind, JobPriority};

/// Reservation handle returned by the scheduler when a worker is allowed to
/// attempt leasing work for a (kind, library, priority) tuple.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SchedulingReservation {
    pub id: Uuid,
    pub kind: JobKind,
    pub library_id: LibraryId,
    pub priority: JobPriority,
}

#[derive(Debug, Default)]
struct PriorityLibraryState {
    ready: usize,
    current_weight: i32,
}

struct LibraryState {
    cap: usize,
    weight: u32,
    inflight: usize,
    pending: usize,
    queues: HashMap<(JobKind, JobPriority), PriorityLibraryState>,
}

impl LibraryState {
    fn new(policy: &LibraryQueuePolicy, defaults: (&QueueDefaults,)) -> Self {
        let defaults = defaults.0;
        let cap = policy.max_inflight.unwrap_or(defaults.default_cap).max(1);
        let weight = policy.weight.unwrap_or(defaults.default_weight).max(1);
        Self {
            cap,
            weight,
            inflight: 0,
            pending: 0,
            queues: HashMap::new(),
        }
    }

    fn ensure_queue(
        &mut self,
        kind: JobKind,
        priority: JobPriority,
    ) -> &mut PriorityLibraryState {
        self.queues.entry((kind, priority)).or_default()
    }

    fn queue_state(
        &mut self,
        kind: JobKind,
        priority: JobPriority,
    ) -> Option<&mut PriorityLibraryState> {
        self.queues.get_mut(&(kind, priority))
    }
}

impl fmt::Debug for LibraryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibraryState")
            .field("cap", &self.cap)
            .field("weight", &self.weight)
            .field("inflight", &self.inflight)
            .field("pending", &self.pending)
            .field("queue_count", &self.queues.len())
            .finish()
    }
}

struct QueueDefaults {
    default_cap: usize,
    default_weight: u32,
    overrides: HashMap<LibraryId, LibraryQueuePolicy>,
}

impl QueueDefaults {
    fn new(config: &QueueConfig) -> Self {
        Self {
            default_cap: max(1, config.default_library_cap),
            default_weight: max(1, config.default_library_weight),
            overrides: config.library_overrides.clone(),
        }
    }

    fn policy_for(&self, library_id: LibraryId) -> LibraryQueuePolicy {
        self.overrides.get(&library_id).cloned().unwrap_or_default()
    }
}

impl fmt::Debug for QueueDefaults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueDefaults")
            .field("default_cap", &self.default_cap)
            .field("default_weight", &self.default_weight)
            .field("override_count", &self.overrides.len())
            .finish()
    }
}

struct ReservationState {
    kind: JobKind,
    library_id: LibraryId,
    priority: JobPriority,
    weight_debt: i32,
}

impl fmt::Debug for ReservationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReservationState")
            .field("kind", &self.kind)
            .field("library_id", &self.library_id)
            .field("priority", &self.priority)
            .field("weight_debt", &self.weight_debt)
            .finish()
    }
}

struct SchedulerState {
    libraries: HashMap<LibraryId, LibraryState>,
    reservations: HashMap<Uuid, ReservationState>,
    next_priority_index: usize,
}

impl SchedulerState {
    fn new() -> Self {
        Self {
            libraries: HashMap::new(),
            reservations: HashMap::new(),
            next_priority_index: 0,
        }
    }

    fn ensure_library(
        &mut self,
        library_id: LibraryId,
        defaults: &QueueDefaults,
    ) -> &mut LibraryState {
        self.libraries.entry(library_id).or_insert_with(|| {
            LibraryState::new(&defaults.policy_for(library_id), (defaults,))
        })
    }

    fn select_for_priority(
        &mut self,
        kind: JobKind,
        priority: JobPriority,
    ) -> Option<(LibraryId, i32)> {
        let mut selected: Option<(LibraryId, i32)> = None;
        let mut total_weight = 0i32;

        for (library_id, state) in self.libraries.iter_mut() {
            if state.inflight + state.pending >= state.cap {
                continue;
            }
            let weight = state.weight as i32;
            if let Some(priority_state) = state.queue_state(kind, priority) {
                if priority_state.ready == 0 {
                    continue;
                }
                priority_state.current_weight += weight;
                total_weight += weight;
                match selected {
                    Some((_, weight))
                        if priority_state.current_weight <= weight => {}
                    _ => {
                        selected =
                            Some((*library_id, priority_state.current_weight));
                    }
                }
            }
        }

        if let Some((library_id, _)) = selected {
            if total_weight == 0 {
                return None;
            }
            if let Some(state) = self.libraries.get_mut(&library_id)
                && let Some(priority_state) = state.queue_state(kind, priority)
            {
                priority_state.current_weight -= total_weight;
                priority_state.ready = priority_state.ready.saturating_sub(1);
                state.pending += 1;
            }
            Some((library_id, total_weight))
        } else {
            None
        }
    }
}

impl fmt::Debug for SchedulerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedulerState")
            .field("library_count", &self.libraries.len())
            .field("reservation_count", &self.reservations.len())
            .field("next_priority_index", &self.next_priority_index)
            .finish()
    }
}

fn build_priority_ring(weights: PriorityWeights) -> Vec<JobPriority> {
    let mut ring = Vec::new();
    for _ in 0..weights.p0.max(1) {
        ring.push(JobPriority::P0);
    }
    for _ in 0..weights.p1.max(1) {
        ring.push(JobPriority::P1);
    }
    for _ in 0..weights.p2.max(1) {
        ring.push(JobPriority::P2);
    }
    for _ in 0..weights.p3.max(1) {
        ring.push(JobPriority::P3);
    }
    ring
}

/// Weighted-fair scheduler shared by worker pools. The scheduler keeps a
/// minimal in-memory view of ready counts per (kind, library, priority) and
/// enforces per-library in-flight caps when allocating leases.
#[derive(Clone)]
pub struct WeightedFairScheduler {
    defaults: Arc<QueueDefaults>,
    priority_ring: Arc<Vec<JobPriority>>,
    state: Arc<Mutex<SchedulerState>>,
}

impl fmt::Debug for WeightedFairScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("WeightedFairScheduler");
        debug
            .field("default_cap", &self.defaults.default_cap)
            .field("default_weight", &self.defaults.default_weight)
            .field("override_count", &self.defaults.overrides.len())
            .field("priority_ring_len", &self.priority_ring.len());

        match self.state.try_lock() {
            Ok(state) => {
                debug
                    .field("library_count", &state.libraries.len())
                    .field("reservation_count", &state.reservations.len())
                    .field("next_priority_index", &state.next_priority_index);
            }
            Err(_) => {
                debug.field("state", &"<locked>");
            }
        }

        debug.finish()
    }
}

/// Bulk ready counts used to prime the scheduler without emitting one event per job.
#[derive(Clone, Debug)]
pub struct ReadyCountEntry {
    pub kind: JobKind,
    pub library_id: LibraryId,
    pub priority: JobPriority,
    pub count: usize,
    pub leased: usize,
}

impl WeightedFairScheduler {
    pub fn new(
        config: &QueueConfig,
        priority_weights: PriorityWeights,
    ) -> Self {
        let defaults = Arc::new(QueueDefaults::new(config));
        let ring = Arc::new(build_priority_ring(priority_weights));
        Self {
            defaults,
            priority_ring: ring,
            state: Arc::new(Mutex::new(SchedulerState::new())),
        }
    }

    pub async fn record_ready(
        &self,
        kind: JobKind,
        library_id: LibraryId,
        priority: JobPriority,
    ) {
        let mut state = self.state.lock().await;
        let library = state.ensure_library(library_id, &self.defaults);
        let priority_state = library.ensure_queue(kind, priority);
        priority_state.ready += 1;
    }

    /// Replace the scheduler's ready and in-flight views with a durable queue snapshot.
    ///
    /// Reservations are removed from ready accounting before a worker leases
    /// the corresponding PostgreSQL row. A durable snapshot can therefore
    /// still include those rows; subtracting pending reservations prevents the
    /// same ready row from being advertised twice during reconciliation. The
    /// durable leased total also repairs capacity leaked by an interrupted
    /// worker or an earlier accounting mismatch.
    pub async fn reconcile_ready_absolute<I>(&self, entries: I)
    where
        I: IntoIterator<Item = ReadyCountEntry>,
    {
        let mut state = self.state.lock().await;
        let mut durable = HashMap::new();
        let mut durable_inflight = HashMap::new();
        for entry in entries {
            let count = durable
                .entry((entry.library_id, entry.kind, entry.priority))
                .or_insert(0usize);
            *count = count.saturating_add(entry.count);
            let inflight =
                durable_inflight.entry(entry.library_id).or_insert(0usize);
            *inflight = inflight.saturating_add(entry.leased);
        }

        let mut pending = HashMap::new();
        for reservation in state.reservations.values() {
            *pending
                .entry((
                    reservation.library_id,
                    reservation.kind,
                    reservation.priority,
                ))
                .or_insert(0usize) += 1;
        }

        for (library_id, library) in state.libraries.iter_mut() {
            for queue in library.queues.values_mut() {
                queue.ready = 0;
            }
            library.inflight = durable_inflight
                .get(library_id)
                .copied()
                .unwrap_or_default();
        }

        for ((library_id, kind, priority), durable_count) in durable {
            let library = state.ensure_library(library_id, &self.defaults);
            let queue_state = library.ensure_queue(kind, priority);
            let pending_count = pending
                .get(&(library_id, kind, priority))
                .copied()
                .unwrap_or_default();
            queue_state.ready = durable_count.saturating_sub(pending_count);
        }

        // A library can have only durable leases and no eligible ready rows.
        // Ensure it is represented so its authoritative in-flight count is not
        // lost merely because every queued item is currently leased.
        for (library_id, leased) in durable_inflight {
            state.ensure_library(library_id, &self.defaults).inflight = leased;
        }
    }

    pub async fn record_enqueued(
        &self,
        kind: JobKind,
        library_id: LibraryId,
        priority: JobPriority,
    ) {
        self.record_ready(kind, library_id, priority).await;
    }

    pub async fn reserve(
        &self,
        kind: JobKind,
    ) -> Option<SchedulingReservation> {
        if self.priority_ring.is_empty() {
            return None;
        }

        let mut state = self.state.lock().await;
        for _ in 0..self.priority_ring.len() {
            let priority = self.priority_ring[state.next_priority_index];
            state.next_priority_index =
                (state.next_priority_index + 1) % self.priority_ring.len();

            if let Some((library_id, weight_debt)) =
                state.select_for_priority(kind, priority)
            {
                let reservation_id = Uuid::now_v7();
                state.reservations.insert(
                    reservation_id,
                    ReservationState {
                        kind,
                        library_id,
                        priority,
                        weight_debt,
                    },
                );
                return Some(SchedulingReservation {
                    id: reservation_id,
                    kind,
                    library_id,
                    priority,
                });
            }
        }
        None
    }

    pub async fn confirm(
        &self,
        reservation_id: Uuid,
    ) -> Option<SchedulingReservation> {
        let mut state = self.state.lock().await;
        let reservation = state.reservations.remove(&reservation_id)?;
        if let Some(library) = state.libraries.get_mut(&reservation.library_id)
        {
            library.pending = library.pending.saturating_sub(1);
            library.inflight += 1;
        }
        Some(SchedulingReservation {
            id: reservation_id,
            kind: reservation.kind,
            library_id: reservation.library_id,
            priority: reservation.priority,
        })
    }

    pub async fn cancel(&self, reservation_id: Uuid) {
        let mut state = self.state.lock().await;
        if let Some(reservation) = state.reservations.remove(&reservation_id)
            && let Some(library) =
                state.libraries.get_mut(&reservation.library_id)
        {
            library.pending = library.pending.saturating_sub(1);
            if let Some(priority_state) =
                library.queue_state(reservation.kind, reservation.priority)
            {
                priority_state.ready += 1;
                priority_state.current_weight += reservation.weight_debt;
            }
        }
    }

    /// Drop a reservation when persistence proves the ready count was stale.
    ///
    /// Unlike [`cancel`](Self::cancel), this does not restore the in-memory
    /// ready count. It lets the runtime self-heal after jobs are cancelled or
    /// purged directly in the durable queue without leaving phantom work that
    /// workers reserve forever.
    pub async fn discard_stale(&self, reservation_id: Uuid) {
        let mut state = self.state.lock().await;
        if let Some(reservation) = state.reservations.remove(&reservation_id)
            && let Some(library) =
                state.libraries.get_mut(&reservation.library_id)
        {
            library.pending = library.pending.saturating_sub(1);
        }
    }

    pub async fn release(&self, library_id: LibraryId) {
        let mut state = self.state.lock().await;
        if let Some(library) = state.libraries.get_mut(&library_id) {
            library.inflight = library.inflight.saturating_sub(1);
        }
    }

    pub async fn record_completed(&self, library_id: LibraryId) {
        self.release(library_id).await;
    }

    /// Forget all in-memory scheduling state for a deleted library.
    ///
    /// Durable queue rows are removed by the library foreign-key cascade. This
    /// companion cleanup prevents the scheduler's ready counts and outstanding
    /// reservations from continuing to advertise that deleted work.
    pub async fn forget_library(&self, library_id: LibraryId) {
        let mut state = self.state.lock().await;
        state.libraries.remove(&library_id);
        state
            .reservations
            .retain(|_, reservation| reservation.library_id != library_id);
    }

    #[cfg(test)]
    pub async fn snapshot(&self) -> HashMap<LibraryId, (usize, usize)> {
        let state = self.state.lock().await;
        state
            .libraries
            .iter()
            .map(|(id, lib)| {
                (
                    *id,
                    (
                        lib.inflight,
                        lib.queues.values().map(|p| p.ready).sum::<usize>(),
                    ),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reservation_accounting_tracks_ready_cancel_inflight_and_completion()
     {
        let scheduler = WeightedFairScheduler::new(
            &QueueConfig::default(),
            PriorityWeights::default(),
        );
        let library_id = LibraryId::new();

        scheduler
            .record_enqueued(JobKind::FolderScan, library_id, JobPriority::P1)
            .await;
        assert_eq!(scheduler.snapshot().await[&library_id], (0, 1));

        let canceled = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("ready job reserves capacity");
        assert_eq!(canceled.library_id, library_id);
        assert_eq!(canceled.kind, JobKind::FolderScan);
        assert_eq!(canceled.priority, JobPriority::P1);
        assert_eq!(scheduler.snapshot().await[&library_id], (0, 0));

        scheduler.cancel(canceled.id).await;
        assert_eq!(scheduler.snapshot().await[&library_id], (0, 1));

        let confirmed = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("restored ready job reserves again");
        scheduler
            .confirm(confirmed.id)
            .await
            .expect("reservation confirms");
        assert_eq!(scheduler.snapshot().await[&library_id], (1, 0));

        scheduler.record_completed(library_id).await;
        assert_eq!(scheduler.snapshot().await[&library_id], (0, 0));

        scheduler
            .record_enqueued(JobKind::FolderScan, library_id, JobPriority::P1)
            .await;
        let stale = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("stale ready count reserves capacity");
        scheduler.discard_stale(stale.id).await;
        assert_eq!(scheduler.snapshot().await[&library_id], (0, 0));
        assert!(scheduler.reserve(JobKind::FolderScan).await.is_none());
    }

    #[tokio::test]
    async fn forget_library_drops_counts_and_pending_reservations_idempotently()
    {
        let scheduler = WeightedFairScheduler::new(
            &QueueConfig::default(),
            PriorityWeights::default(),
        );
        let deleted_library = LibraryId::new();
        let retained_library = LibraryId::new();

        scheduler
            .reconcile_ready_absolute([ReadyCountEntry {
                kind: JobKind::FolderScan,
                library_id: deleted_library,
                priority: JobPriority::P1,
                count: 3,
                leased: 0,
            }])
            .await;

        let inflight = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("deleted library has ready work");
        assert_eq!(inflight.library_id, deleted_library);
        scheduler
            .confirm(inflight.id)
            .await
            .expect("first reservation confirms");

        let pending = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("deleted library has another ready job");
        assert_eq!(pending.library_id, deleted_library);
        scheduler
            .record_enqueued(
                JobKind::FolderScan,
                retained_library,
                JobPriority::P2,
            )
            .await;

        scheduler.forget_library(deleted_library).await;
        scheduler.forget_library(deleted_library).await;

        let snapshot = scheduler.snapshot().await;
        assert!(!snapshot.contains_key(&deleted_library));
        assert_eq!(snapshot[&retained_library], (0, 1));
        assert!(scheduler.confirm(pending.id).await.is_none());

        let retained = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("retained library remains schedulable");
        assert_eq!(retained.library_id, retained_library);
    }

    #[tokio::test]
    async fn workers_cannot_reserve_ready_work_for_another_kind() {
        let scheduler = WeightedFairScheduler::new(
            &QueueConfig::default(),
            PriorityWeights::default(),
        );
        let library_id = LibraryId::new();

        scheduler
            .record_enqueued(JobKind::FolderScan, library_id, JobPriority::P1)
            .await;

        assert!(scheduler.reserve(JobKind::ImageFetch).await.is_none());
        let reservation = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("the matching worker kind reserves the ready job");
        assert_eq!(reservation.kind, JobKind::FolderScan);
        assert_eq!(reservation.library_id, library_id);
    }

    #[tokio::test]
    async fn absolute_reconciliation_replaces_stale_counts_and_preserves_pending()
     {
        let scheduler = WeightedFairScheduler::new(
            &QueueConfig::default(),
            PriorityWeights::default(),
        );
        let library_id = LibraryId::new();

        scheduler
            .record_enqueued(JobKind::ImageFetch, library_id, JobPriority::P1)
            .await;
        scheduler
            .reconcile_ready_absolute([ReadyCountEntry {
                kind: JobKind::FolderScan,
                library_id,
                priority: JobPriority::P1,
                count: 3,
                leased: 0,
            }])
            .await;

        assert!(scheduler.reserve(JobKind::ImageFetch).await.is_none());
        let pending = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("durable folder work is schedulable");

        scheduler
            .reconcile_ready_absolute([ReadyCountEntry {
                kind: JobKind::FolderScan,
                library_id,
                priority: JobPriority::P1,
                count: 3,
                leased: 0,
            }])
            .await;
        assert_eq!(scheduler.snapshot().await[&library_id], (0, 2));

        scheduler.cancel(pending.id).await;
        assert_eq!(scheduler.snapshot().await[&library_id], (0, 3));

        scheduler.reconcile_ready_absolute(std::iter::empty()).await;
        assert_eq!(scheduler.snapshot().await[&library_id], (0, 0));
    }

    #[tokio::test]
    async fn absolute_reconciliation_repairs_stale_inflight_at_library_cap() {
        let mut config = QueueConfig::default();
        config.default_library_cap = 1;
        let scheduler =
            WeightedFairScheduler::new(&config, PriorityWeights::default());
        let library_id = LibraryId::new();

        scheduler
            .record_enqueued(JobKind::FolderScan, library_id, JobPriority::P1)
            .await;
        let leaked = scheduler
            .reserve(JobKind::FolderScan)
            .await
            .expect("ready job reserves capacity");
        scheduler
            .confirm(leaked.id)
            .await
            .expect("reservation confirms");
        assert_eq!(scheduler.snapshot().await[&library_id], (1, 0));
        assert!(scheduler.reserve(JobKind::FolderScan).await.is_none());

        scheduler
            .reconcile_ready_absolute([ReadyCountEntry {
                kind: JobKind::FolderScan,
                library_id,
                priority: JobPriority::P1,
                count: 1,
                leased: 0,
            }])
            .await;

        assert_eq!(scheduler.snapshot().await[&library_id], (0, 1));
        assert!(
            scheduler.reserve(JobKind::FolderScan).await.is_some(),
            "PostgreSQL snapshot clears leaked in-flight capacity"
        );
    }
}
