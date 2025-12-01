//! Registry for managing multiple carousel states keyed by CarouselKey

use std::collections::HashMap;

use super::animator::SnapAnimator;
use super::{
    state::VirtualCarouselState,
    types::{CarouselConfig, CarouselKey},
};
use crate::domains::ui::motion_controller::{
    MotionController, MotionControllerConfig,
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub enum MotionState {
    Idle,
    /// Kinetic scrolling with direction: +1 (right), -1 (left)
    Kinetic(i32),
    /// Snapping toward a target index position and pixel destination
    Snap {
        target_index: f32,
        target_x: f32,
    },
}

#[derive(Debug, Default)]
pub struct CarouselRegistry {
    pub(crate) states: HashMap<CarouselKey, VirtualCarouselState>,
    scrollers: HashMap<CarouselKey, MotionController>,
    animators: HashMap<CarouselKey, SnapAnimator>,
    holds: HashMap<CarouselKey, Instant>,
    hold_start_offsets: HashMap<CarouselKey, f32>,
    hold_start_indices: HashMap<CarouselKey, f32>,
    last_snapshot: HashMap<CarouselKey, Instant>,
    motion_states: HashMap<CarouselKey, MotionState>,
    // Mouse/trackpad settle detection for committing reference index
    mouse_settle_candidate: HashMap<CarouselKey, f32>,
    mouse_settle_started: HashMap<CarouselKey, Instant>,
}

impl CarouselRegistry {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            scrollers: HashMap::new(),
            animators: HashMap::new(),
            holds: HashMap::new(),
            hold_start_offsets: HashMap::new(),
            hold_start_indices: HashMap::new(),
            last_snapshot: HashMap::new(),
            motion_states: HashMap::new(),
            mouse_settle_candidate: HashMap::new(),
            mouse_settle_started: HashMap::new(),
        }
    }

    /// Get a mutable reference, creating a new state with the provided factory when absent.
    pub fn get_or_insert_with<F>(
        &mut self,
        key: CarouselKey,
        init: F,
    ) -> &mut VirtualCarouselState
    where
        F: FnOnce() -> VirtualCarouselState,
    {
        self.states.entry(key).or_insert_with(init)
    }

    pub fn get(&self, key: &CarouselKey) -> Option<&VirtualCarouselState> {
        self.states.get(key)
    }

    pub fn get_mut(
        &mut self,
        key: &CarouselKey,
    ) -> Option<&mut VirtualCarouselState> {
        self.states.get_mut(key)
    }

    pub fn remove(
        &mut self,
        key: &CarouselKey,
    ) -> Option<VirtualCarouselState> {
        self.scrollers.remove(key);
        self.states.remove(key)
    }

    /// Convenience helper for creating a default state given basic parameters.
    ///
    /// The `scale` parameter is used to scale card dimensions when the config
    /// specifies a `card_size`. Pass `1.0` for unscaled or use the effective
    /// scale from `ScalingContext`.
    pub fn ensure_default(
        &mut self,
        key: CarouselKey,
        total_items: usize,
        viewport_width: f32,
        config: CarouselConfig,
        scale: f32,
    ) -> &mut VirtualCarouselState {
        // Create if missing
        let state = self.states.entry(key).or_insert_with(|| {
            VirtualCarouselState::new(
                total_items,
                viewport_width,
                config,
                scale,
            )
        });

        // Always bring dynamic properties up to date even if the state already exists.
        // This ensures initial carousels reflect current counts and viewport without
        // waiting for a scroll event to trigger recalculation.
        if state.total_items != total_items {
            state.set_total_items(total_items);
        }

        if (state.viewport_width - viewport_width).abs() > 0.5 {
            state.update_dimensions(viewport_width);
        }

        state
    }

    /// Ensure a kinetic scroller exists for a key using the provided config.
    pub fn ensure_scroller_with_config(
        &mut self,
        key: &CarouselKey,
        cfg: MotionControllerConfig,
    ) -> &mut MotionController {
        self.scrollers
            .entry(key.clone())
            .or_insert_with(|| MotionController::new_with_config(cfg))
    }

    pub fn get_scroller(&self, key: &CarouselKey) -> Option<&MotionController> {
        self.scrollers.get(key)
    }

    pub fn get_scroller_mut(
        &mut self,
        key: &CarouselKey,
    ) -> Option<&mut MotionController> {
        self.scrollers.get_mut(key)
    }

    pub fn ensure_animator(&mut self, key: &CarouselKey) -> &mut SnapAnimator {
        self.animators
            .entry(key.clone())
            .or_insert_with(SnapAnimator::new)
    }

    pub fn get_animator(&self, key: &CarouselKey) -> Option<&SnapAnimator> {
        self.animators.get(key)
    }

    pub fn get_animator_mut(
        &mut self,
        key: &CarouselKey,
    ) -> Option<&mut SnapAnimator> {
        self.animators.get_mut(key)
    }

    pub fn begin_hold(&mut self, key: &CarouselKey) {
        self.holds.insert(key.clone(), Instant::now());
    }

    pub fn end_hold_elapsed_ms(&mut self, key: &CarouselKey) -> Option<u128> {
        self.holds
            .remove(key)
            .map(|inst| inst.elapsed().as_millis())
    }

    /// Begin a hold and record the starting absolute offset (for displacement-based heuristics).
    pub fn begin_hold_at(&mut self, key: &CarouselKey, start_offset: f32) {
        self.begin_hold(key);
        self.hold_start_offsets.insert(key.clone(), start_offset);
    }

    /// Finish a hold and return the moved units (in strides), removing the start offset.
    /// Caller must provide the current absolute offset and stride size.
    pub fn end_hold_moved_units(
        &mut self,
        key: &CarouselKey,
        current_offset: f32,
        stride: f32,
    ) -> Option<f32> {
        let start = self.hold_start_offsets.remove(key)?;
        if stride <= 0.0 {
            return Some(0.0);
        }
        Some((current_offset - start) / stride)
    }

    /// Begin a hold and record the starting index position.
    pub fn begin_hold_index_at(&mut self, key: &CarouselKey, start_index: f32) {
        self.begin_hold(key);
        self.hold_start_indices.insert(key.clone(), start_index);
    }

    /// Finish a hold and return the moved units (in index units), removing the start index.
    pub fn end_hold_moved_units_index(
        &mut self,
        key: &CarouselKey,
        current_index: f32,
    ) -> Option<f32> {
        let start = self.hold_start_indices.remove(key)?;
        Some(current_index - start)
    }

    pub fn set_motion_state(&mut self, key: &CarouselKey, state: MotionState) {
        self.motion_states.insert(key.clone(), state);
    }

    pub fn motion_state(&self, key: &CarouselKey) -> MotionState {
        self.motion_states
            .get(key)
            .copied()
            .unwrap_or(MotionState::Idle)
    }

    pub fn clear_motion_state(&mut self, key: &CarouselKey) {
        self.motion_states.insert(key.clone(), MotionState::Idle);
    }

    pub fn should_emit_snapshot(
        &mut self,
        key: &CarouselKey,
        debounce: Duration,
    ) -> bool {
        let now = Instant::now();
        match self.last_snapshot.get(key) {
            Some(last) if now.duration_since(*last) < debounce => false,
            _ => {
                self.last_snapshot.insert(key.clone(), now);
                true
            }
        }
    }

    /// Update or start a settle timer for a near-aligned candidate index.
    /// Returns the elapsed milliseconds since the current candidate was first seen.
    pub fn update_mouse_settle_candidate(
        &mut self,
        key: &CarouselKey,
        candidate_index: f32,
    ) -> u128 {
        let eps = 1e-4;
        let now = Instant::now();
        match self.mouse_settle_candidate.get(key).copied() {
            Some(prev) if (prev - candidate_index).abs() <= eps => {
                let start = self
                    .mouse_settle_started
                    .entry(key.clone())
                    .or_insert_with(Instant::now);
                start.elapsed().as_millis()
            }
            _ => {
                self.mouse_settle_candidate
                    .insert(key.clone(), candidate_index);
                self.mouse_settle_started.insert(key.clone(), now);
                0
            }
        }
    }

    /// Clear any settle tracking for this key.
    pub fn clear_mouse_settle(&mut self, key: &CarouselKey) {
        self.mouse_settle_candidate.remove(key);
        self.mouse_settle_started.remove(key);
    }

    /// Return a snapshot of all keys currently in the registry.
    pub fn keys(&self) -> Vec<CarouselKey> {
        self.states.keys().cloned().collect()
    }
}
