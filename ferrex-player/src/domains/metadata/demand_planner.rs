//! Poster Demand Engine (Planner)
//!
//! This module defines types and a minimal launcher for the demand planner.
//! Coalesces UI snapshots, computes desired requests, and pushes them to the
//! image service with appropriate priorities.

use ferrex_core::player_prelude::{
    EpisodeStillSize, ImageRequest, PosterKind, PosterSize, Priority,
};
use uuid::Uuid;

use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc;

/// Optional context for non-grid imagery (e.g., hero/backdrop/cast).
#[derive(Debug, Clone, Default)]
pub struct DemandContext {
    overrides: HashMap<Uuid, DemandRequestKind>,
}

impl DemandContext {
    /// Record an explicit image request mapping for a given media id.
    pub fn override_request(&mut self, id: Uuid, kind: DemandRequestKind) {
        self.overrides.insert(id, kind);
    }

    /// Look up an explicit request override.
    pub fn request_kind(&self, id: &Uuid) -> Option<&DemandRequestKind> {
        self.overrides.get(id)
    }
}

/// Specific image request instructions used to override the default poster mapping.
#[derive(Debug, Clone)]
pub enum DemandRequestKind {
    Poster { kind: PosterKind, size: PosterSize },
    EpisodeStill { size: EpisodeStillSize },
}

/// Snapshot of current demand produced by UI layer.
#[derive(Debug, Clone)]
pub struct DemandSnapshot {
    pub visible_ids: Vec<Uuid>,
    pub prefetch_ids: Vec<Uuid>,
    pub background_ids: Vec<Uuid>,
    pub timestamp: std::time::Instant,
    pub context: Option<DemandContext>,
    pub poster_kind: Option<PosterKind>,
}

/// Handle to send snapshots to the planner.
#[derive(Debug, Clone)]
pub struct PlannerHandle {
    tx: mpsc::UnboundedSender<DemandSnapshot>,
}

impl PlannerHandle {
    pub fn send(&self, snapshot: DemandSnapshot) {
        let _ = self.tx.send(snapshot);
    }
}

/// Start the planner task.
pub fn start_planner(
    image_service: crate::domains::metadata::image_service::UnifiedImageService,
) -> (PlannerHandle, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<DemandSnapshot>();
    let handle = PlannerHandle { tx };

    let join = tokio::spawn(async move {
        loop {
            // Drain all pending snapshots and process them as a single union
            let first = rx.recv().await;
            let Some(first_snap) = first else { break };
            let mut snapshots = Vec::with_capacity(4);
            snapshots.push(first_snap);
            while let Ok(next) = rx.try_recv() {
                snapshots.push(next);
            }

            // Build a union of desired requests across all drained snapshots,
            // preserving insertion order and upgrading priorities when repeated.
            let mut union_desired: Vec<(ImageRequest, Priority)> = Vec::new();
            let mut positions: HashMap<ImageRequest, usize> = HashMap::new();
            let mut total_visible = 0usize;
            let mut total_prefetch = 0usize;
            let mut total_background = 0usize;

            // Helper to insert/upgrade priority while preserving order
            let mut push_or_update =
                |req: ImageRequest,
                 prio: Priority,
                 out: &mut Vec<(ImageRequest, Priority)>,
                 pos: &mut HashMap<ImageRequest, usize>| {
                    if let Some(i) = pos.get(&req) {
                        if prio.weight() > out[*i].1.weight() {
                            out[*i].1 = prio;
                        }
                    } else {
                        pos.insert(req.clone(), out.len());
                        out.push((req, prio));
                    }
                };

            for snap in snapshots.iter() {
                total_visible += snap.visible_ids.len();
                total_prefetch += snap.prefetch_ids.len();
                total_background += snap.background_ids.len();

                let desired = build_desired_set(
                    snap.visible_ids.iter().copied(),
                    snap.prefetch_ids.iter().copied(),
                    snap.background_ids.iter().copied(),
                    snap.poster_kind,
                    snap.context.as_ref(),
                );

                for (req, prio) in desired.into_iter() {
                    push_or_update(
                        req,
                        prio,
                        &mut union_desired,
                        &mut positions,
                    );
                }
            }

            // Proceed with existing planner logic against the union of desired requests
            let state = image_service.snapshot_state();
            let mut desired_requests: HashSet<ImageRequest> =
                HashSet::with_capacity(union_desired.len());
            for (req, prio) in union_desired.iter() {
                desired_requests.insert(req.clone());
                if state.loaded.contains(req) {
                    continue;
                }
                image_service.request_image(req.clone().with_priority(*prio));
            }

            const CANCELLATION_GRACE_MS: u64 = 75;
            const MAX_CANCEL_PER_TICK: usize = 6;

            if state.in_flight >= image_service.max_concurrent() {
                let mut cancelled = 0usize;
                for req in &state.loading {
                    if desired_requests.contains(req) {
                        continue;
                    }
                    if let Some(started) = image_service.loading_started_at(req)
                        && started.elapsed()
                            >= Duration::from_millis(CANCELLATION_GRACE_MS)
                    {
                        if image_service.cancel_inflight(req) {
                            cancelled += 1;
                            log::trace!(
                                "Planner cancelled in-flight request {:?}",
                                req.media_id
                            );
                            if cancelled >= MAX_CANCEL_PER_TICK {
                                break;
                            }
                        }
                    }
                }
            }

            let mut trimmed = 0usize;
            for req in &state.queued {
                if desired_requests.contains(req) {
                    continue;
                }
                if image_service.remove_from_queue(req) {
                    trimmed += 1;
                }
            }
            if trimmed > 0 {
                log::trace!("Planner trimmed {} queued requests", trimmed);
            }

            log::trace!(
                "Planner snapshots processed (union) (visible_total={}, prefetch_total={}, background_total={}, desired_unique={})",
                total_visible,
                total_prefetch,
                total_background,
                desired_requests.len()
            );
        }
    });

    (handle, join)
}

/// Helper to map media IDs to ImageRequests with the desired priority.
/// Note: This does not interact with services; it only computes the set.
pub fn build_desired_set(
    visible_ids: impl IntoIterator<Item = Uuid>,
    prefetch_ids: impl IntoIterator<Item = Uuid>,
    background_ids: impl IntoIterator<Item = Uuid>,
    poster_kind: Option<PosterKind>,
    context: Option<&DemandContext>,
) -> Vec<(ImageRequest, Priority)> {
    let mut out: Vec<(ImageRequest, Priority)> = Vec::new();
    let mut positions: HashMap<ImageRequest, usize> = HashMap::new();

    // Helper closure to insert or upgrade priority while preserving order.
    let mut push_or_update =
        |req: ImageRequest,
         prio: Priority,
         out: &mut Vec<(ImageRequest, Priority)>,
         positions: &mut HashMap<ImageRequest, usize>| {
            if let Some(existing) = positions.get(&req) {
                if prio.weight() > out[*existing].1.weight() {
                    out[*existing].1 = prio;
                }
            } else {
                positions.insert(req.clone(), out.len());
                out.push((req, prio));
            }
        };

    // Visible → Visible priority
    let visible_list: Vec<Uuid> = visible_ids.into_iter().collect();
    if !visible_list.is_empty() {
        let center = visible_list.len() / 2;
        let mut indexed: Vec<(usize, Uuid)> =
            visible_list.into_iter().enumerate().collect();
        indexed.sort_by_key(|(idx, _)| idx.abs_diff(center));
        for (_idx, id) in indexed {
            let req = resolve_image_request(id, poster_kind, context);
            push_or_update(req, Priority::Visible, &mut out, &mut positions);
        }
    }

    // Prefetch → Preload priority (do not override Visible if already set)
    for id in prefetch_ids {
        let req = resolve_image_request(id, poster_kind, context);
        push_or_update(req, Priority::Preload, &mut out, &mut positions);
    }

    // Background → Background priority (lowest tier)
    for id in background_ids {
        let req = resolve_image_request(id, poster_kind, context);
        push_or_update(req, Priority::Background, &mut out, &mut positions);
    }

    out
}

fn resolve_image_request(
    id: Uuid,
    fallback_kind: Option<PosterKind>,
    context: Option<&DemandContext>,
) -> ImageRequest {
    if let Some(ctx) = context {
        if let Some(kind) = ctx.request_kind(&id) {
            return match kind {
                DemandRequestKind::Poster { kind, size } => {
                    ImageRequest::poster(id, *kind, *size)
                }
                DemandRequestKind::EpisodeStill { size } => {
                    ImageRequest::episode_still(id, *size)
                }
            };
        }
    }

    let kind = fallback_kind.unwrap_or(PosterKind::Movie);
    ImageRequest::poster(id, kind, PosterSize::W300)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::player_prelude::PosterKind;

    fn uuid(idx: u128) -> Uuid {
        Uuid::from_u128(idx)
    }

    #[test]
    fn build_desired_set_prioritizes_center_first() {
        let visible = vec![uuid(1), uuid(2), uuid(3), uuid(4)];
        let desired = build_desired_set(
            visible.clone(),
            Vec::<Uuid>::new(),
            Vec::<Uuid>::new(),
            Some(PosterKind::Movie),
            None,
        );
        let order: Vec<Uuid> =
            desired.iter().map(|(req, _)| req.media_id).collect();
        assert_eq!(order, vec![uuid(3), uuid(2), uuid(4), uuid(1)]);
        assert!(
            desired
                .iter()
                .all(|(_req, prio)| matches!(prio, Priority::Visible))
        );
    }

    #[test]
    fn prefetch_duplicate_does_not_downgrade_visible() {
        let id = uuid(10);
        let desired = build_desired_set(
            vec![id],
            vec![id],
            Vec::<Uuid>::new(),
            Some(PosterKind::Series),
            None,
        );
        assert_eq!(desired.len(), 1);
        assert_eq!(desired[0].1, Priority::Visible);
    }

    #[test]
    fn context_override_supports_episode_stills() {
        let episode_id = uuid(42);
        let mut context = DemandContext::default();
        context.override_request(
            episode_id,
            DemandRequestKind::EpisodeStill {
                size: EpisodeStillSize::Standard,
            },
        );

        let desired = build_desired_set(
            Vec::<Uuid>::new(),
            vec![episode_id],
            Vec::<Uuid>::new(),
            None,
            Some(&context),
        );

        assert_eq!(desired.len(), 1);
        let request = &desired[0].0;
        assert_eq!(request.media_id, episode_id);
        assert_eq!(
            request.image_type,
            ferrex_core::player_prelude::ImageType::Episode
        );
        assert_eq!(desired[0].1, Priority::Preload);
    }

    #[test]
    fn background_ids_are_included_with_lowest_priority() {
        let visible = vec![uuid(1)];
        let prefetch = vec![uuid(2)];
        let background = vec![uuid(3), uuid(4)];

        let desired = build_desired_set(
            visible,
            prefetch,
            background.clone(),
            Some(PosterKind::Movie),
            None,
        );

        let background_entries: Vec<(ImageRequest, Priority)> = desired
            .into_iter()
            .filter(|(req, _)| background.contains(&req.media_id))
            .collect();

        assert_eq!(background_entries.len(), 2);
        assert!(
            background_entries
                .iter()
                .all(|(_, prio)| matches!(prio, Priority::Background))
        );
    }
}
