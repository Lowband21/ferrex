use std::cmp::max;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::types::ids::LibraryID;

use super::config::{LibraryQueuePolicy, PriorityWeights, QueueConfig};
use super::job::JobPriority;

/// Reservation handle returned by the scheduler when a worker is allowed to
/// attempt leasing work for a (library, priority) pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SchedulingReservation {
    pub id: Uuid,
    pub library_id: LibraryID,
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
    priorities: HashMap<JobPriority, PriorityLibraryState>,
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
            priorities: HashMap::new(),
        }
    }

    fn ensure_priority(&mut self, priority: JobPriority) -> &mut PriorityLibraryState {
        self.priorities.entry(priority).or_default()
    }

    fn priority_state(&mut self, priority: JobPriority) -> Option<&mut PriorityLibraryState> {
        self.priorities.get_mut(&priority)
    }
}

impl fmt::Debug for LibraryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibraryState")
            .field("cap", &self.cap)
            .field("weight", &self.weight)
            .field("inflight", &self.inflight)
            .field("pending", &self.pending)
            .field("priority_count", &self.priorities.len())
            .finish()
    }
}

struct QueueDefaults {
    default_cap: usize,
    default_weight: u32,
    overrides: HashMap<LibraryID, LibraryQueuePolicy>,
}

impl QueueDefaults {
    fn new(config: &QueueConfig) -> Self {
        Self {
            default_cap: max(1, config.default_library_cap),
            default_weight: max(1, config.default_library_weight),
            overrides: config.library_overrides.clone(),
        }
    }

    fn policy_for(&self, library_id: LibraryID) -> LibraryQueuePolicy {
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
    library_id: LibraryID,
    priority: JobPriority,
    weight_debt: i32,
}

impl fmt::Debug for ReservationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReservationState")
            .field("library_id", &self.library_id)
            .field("priority", &self.priority)
            .field("weight_debt", &self.weight_debt)
            .finish()
    }
}

struct SchedulerState {
    libraries: HashMap<LibraryID, LibraryState>,
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
        library_id: LibraryID,
        defaults: &QueueDefaults,
    ) -> &mut LibraryState {
        self.libraries
            .entry(library_id)
            .or_insert_with(|| LibraryState::new(&defaults.policy_for(library_id), (defaults,)))
    }

    fn select_for_priority(&mut self, priority: JobPriority) -> Option<(LibraryID, i32)> {
        let mut selected: Option<(LibraryID, i32)> = None;
        let mut total_weight = 0i32;

        for (library_id, state) in self.libraries.iter_mut() {
            if state.inflight + state.pending >= state.cap {
                continue;
            }
            let weight = state.weight as i32;
            if let Some(priority_state) = state.priority_state(priority) {
                if priority_state.ready == 0 {
                    continue;
                }
                priority_state.current_weight += weight;
                total_weight += weight;
                match selected {
                    Some((_, weight)) if priority_state.current_weight <= weight => {}
                    _ => {
                        selected = Some((*library_id, priority_state.current_weight));
                    }
                }
            }
        }

        if let Some((library_id, _)) = selected {
            if total_weight == 0 {
                return None;
            }
            if let Some(state) = self.libraries.get_mut(&library_id)
                && let Some(priority_state) = state.priority_state(priority)
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
/// minimal in-memory view of ready counts per (library, priority) and enforces
/// per-library in-flight caps when allocating leases.
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
    pub library_id: LibraryID,
    pub priority: JobPriority,
    pub count: usize,
}

impl WeightedFairScheduler {
    pub fn new(config: &QueueConfig, priority_weights: PriorityWeights) -> Self {
        let defaults = Arc::new(QueueDefaults::new(config));
        let ring = Arc::new(build_priority_ring(priority_weights));
        Self {
            defaults,
            priority_ring: ring,
            state: Arc::new(Mutex::new(SchedulerState::new())),
        }
    }

    pub async fn record_ready(&self, library_id: LibraryID, priority: JobPriority) {
        let mut state = self.state.lock().await;
        let library = state.ensure_library(library_id, &self.defaults);
        let priority_state = library.ensure_priority(priority);
        priority_state.ready += 1;
    }

    pub async fn record_ready_bulk<I>(&self, entries: I)
    where
        I: IntoIterator<Item = ReadyCountEntry>,
    {
        let mut state = self.state.lock().await;
        for entry in entries.into_iter() {
            if entry.count == 0 {
                continue;
            }
            let library = state.ensure_library(entry.library_id, &self.defaults);
            let priority_state = library.ensure_priority(entry.priority);
            priority_state.ready = priority_state.ready.saturating_add(entry.count);
        }
    }

    pub async fn record_enqueued(&self, library_id: LibraryID, priority: JobPriority) {
        self.record_ready(library_id, priority).await;
    }

    pub async fn reserve(&self) -> Option<SchedulingReservation> {
        if self.priority_ring.is_empty() {
            return None;
        }

        let mut state = self.state.lock().await;
        for _ in 0..self.priority_ring.len() {
            let priority = self.priority_ring[state.next_priority_index];
            state.next_priority_index = (state.next_priority_index + 1) % self.priority_ring.len();

            if let Some((library_id, weight_debt)) = state.select_for_priority(priority) {
                let reservation_id = Uuid::now_v7();
                state.reservations.insert(
                    reservation_id,
                    ReservationState {
                        library_id,
                        priority,
                        weight_debt,
                    },
                );
                return Some(SchedulingReservation {
                    id: reservation_id,
                    library_id,
                    priority,
                });
            }
        }
        None
    }

    pub async fn confirm(&self, reservation_id: Uuid) -> Option<SchedulingReservation> {
        let mut state = self.state.lock().await;
        let reservation = state.reservations.remove(&reservation_id)?;
        if let Some(library) = state.libraries.get_mut(&reservation.library_id) {
            library.pending = library.pending.saturating_sub(1);
            library.inflight += 1;
        }
        Some(SchedulingReservation {
            id: reservation_id,
            library_id: reservation.library_id,
            priority: reservation.priority,
        })
    }

    pub async fn cancel(&self, reservation_id: Uuid) {
        let mut state = self.state.lock().await;
        if let Some(reservation) = state.reservations.remove(&reservation_id)
            && let Some(library) = state.libraries.get_mut(&reservation.library_id)
        {
            library.pending = library.pending.saturating_sub(1);
            if let Some(priority_state) = library.priority_state(reservation.priority) {
                priority_state.ready += 1;
                priority_state.current_weight += reservation.weight_debt;
            }
        }
    }

    pub async fn release(&self, library_id: LibraryID) {
        let mut state = self.state.lock().await;
        if let Some(library) = state.libraries.get_mut(&library_id) {
            library.inflight = library.inflight.saturating_sub(1);
        }
    }

    pub async fn record_completed(&self, library_id: LibraryID) {
        self.release(library_id).await;
    }

    #[cfg(test)]
    pub async fn snapshot(&self) -> HashMap<LibraryID, (usize, usize)> {
        let state = self.state.lock().await;
        state
            .libraries
            .iter()
            .map(|(id, lib)| {
                (
                    *id,
                    (
                        lib.inflight,
                        lib.priorities
                            .values()
                            .map(|p| p.ready)
                            .fold(0usize, |acc, v| acc + v),
                    ),
                )
            })
            .collect()
    }
}
