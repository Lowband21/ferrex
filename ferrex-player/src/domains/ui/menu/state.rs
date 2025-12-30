use log::debug;

use crate::{
    domains::ui::menu::InteractionPhase,
    infra::{constants::flip::*, shader_widgets::poster::PosterFace},
};

use std::{
    f32::consts::{FRAC_PI_2, PI},
    time::Instant,
};

/// Per-poster flip state that drives the right-click menu animation.
#[derive(Debug, Clone)]
pub struct PosterMenuState {
    /// Current rotation in radians (0 = front, PI = back).
    /// Can exceed 2I during multi-rotation spin.
    pub angle: f32,

    /// Desired face to animate toward.
    pub target_face: PosterFace,

    /// Angular velocity in radians/sec (signed: positive toward PI, negative toward 0).
    pub velocity: f32,

    /// Last time the integrator ran.
    pub last_update: Instant,

    /// Current interaction phase - determines which physics mode to use.
    pub phase: InteractionPhase,

    /// When the current hold started - used to distinguish click vs hold.
    pub hold_start_time: Option<Instant>,
}

impl PosterMenuState {
    pub fn new(now: Instant) -> Self {
        Self {
            angle: 0.0,
            target_face: PosterFace::Front,
            velocity: 0.0,
            last_update: now,
            phase: InteractionPhase::Idle,
            hold_start_time: None,
        }
    }

    // ========== STATE TRANSITIONS ==========

    /// Begin holding right mouse button.
    /// Records start face/time and begins acceleration phase.
    pub fn mark_begin(&mut self, now: Instant) {
        // Determine if truly at rest vs animating
        let at_rest = self.velocity.abs() < SETTLE_VELOCITY_THRESHOLD
            && self.phase == InteractionPhase::Idle
            && self.hold_start_time.is_none();

        if at_rest {
            // Fresh start from rest
            debug!("Beginning hold from rest");
            self.target_face = self.face_from_angle().opposite();
        } else {
            // Click during animation - advance target face
            debug!("Click during animation - advancing target");
            self.target_face = self.target_face.opposite();
        }

        // Always add impulse (clamped), enter holding phase
        self.velocity = (self.velocity + CLICK_IMPULSE).min(MAX_VELOCITY);
        self.phase = InteractionPhase::Holding;
        self.hold_start_time = Some(now);
        self.last_update = now;
    }

    /// End hold - transition to settling or idle based on hold duration.
    /// Short hold (< 0.5s) → Idle (direct spring, smooth flip)
    /// Long hold (>= 0.5s) → Settling (periodic potential, multi-rotation)
    pub fn mark_end(&mut self, now: Instant) {
        if matches!(self.phase, InteractionPhase::Holding) {
            // Calculate hold duration
            let hold_duration = self
                .hold_start_time
                .map(|start| (now - start).as_secs_f32())
                .unwrap_or(0.0);

            if hold_duration >= HOLD_THRESHOLD {
                // Long hold: use settling with periodic potential
                self.phase = InteractionPhase::Settling {
                    must_reach_opposite: true,
                };
            } else {
                // Short hold (click): use direct spring for smooth flip
                self.phase = InteractionPhase::Idle;
            }

            // self.last_update = now;
        }
        self.hold_start_time = None;
    }

    /// Force close menu - animate to front face.
    pub fn force_to(&mut self, now: Instant, face: PosterFace) {
        self.target_face = face;
        self.velocity = 0.0;
        self.phase = InteractionPhase::Idle;
        self.hold_start_time = None;
        self.last_update = now;
    }

    // ========== PHYSICS STEP ==========

    /// Advance the state machine; returns true while motion is active.
    pub fn step(&mut self, now: Instant) -> bool {
        let dt = (now - self.last_update).as_secs_f32().min(0.05);
        self.last_update = now;

        match self.phase {
            InteractionPhase::Holding => self.step_holding(dt),
            InteractionPhase::Settling {
                must_reach_opposite,
            } => self.step_settling(dt, must_reach_opposite),
            InteractionPhase::Idle => self.step_idle(dt),
        }
    }

    /// Holding phase: continuous acceleration toward target.
    fn step_holding(&mut self, dt: f32) -> bool {
        let hold_duration = self
            .hold_start_time
            .unwrap_or(Instant::now())
            .elapsed()
            .as_secs_f32();

        if hold_duration >= HOLD_THRESHOLD {
            self.velocity +=
                (BASE_ACCEL * hold_duration.max(1.0) * JERK_FACTOR)
                    .min(MAX_ACCEL)
                    * dt;
            self.velocity = self.velocity.clamp(-MAX_VELOCITY, MAX_VELOCITY);
        }

        // Integrate position
        self.angle += self.velocity * dt;
        self.clamp_angle_lower();

        true // Always active while holding
    }

    /// Settling phase: periodic potential (sin-based gravity wells) with friction.
    fn step_settling(&mut self, dt: f32, must_reach_opposite: bool) -> bool {
        let target_angle = self.nearest_stable();
        if (self.angle - target_angle).abs() > STOPPED_ANGLE_THRESHOLD {
            // Periodic potential creates stable points at 0, PI, 2*PI...
            // sin(2*angle) has zeros at 0, PI/2, PI, 3PI/2, 2PI...
            // -sin(2*angle) pulls toward 0, PI, 2*PI (stable) and repels from PI/2, 3PI/2
            let snap_force = -SNAP_K * (2.0 * self.angle).sin();
            let damping_force = -SETTLE_DAMPING_B * self.velocity;
            self.velocity += (snap_force + damping_force) * dt;

            // Check if we need to ensure landing on opposite face
            if must_reach_opposite {
                let current_face = self.face_from_angle();
                // If we're slowing down on the same face we started, nudge
                if current_face != self.target_face
                    && self.velocity.abs() < NUDGE_VELOCITY_THRESHOLD
                {
                    debug!("Applying nudge impulse to reach target face");
                    self.velocity += NUDGE_IMPULSE * dt;
                }
            }

            // Integrate position
            self.angle += self.velocity * dt;
            self.clamp_angle_lower();

            // Check for settling
            self.check_and_snap_settling()
        } else {
            self.phase = InteractionPhase::Idle;
            false
        }
    }

    /// Idle phase: direct spring toward target face.
    fn step_idle(&mut self, dt: f32) -> bool {
        let target_angle = self.next_stable();
        if (self.angle - target_angle).abs() > STOPPED_ANGLE_THRESHOLD {
            let spring_force = SPRING_K * (target_angle - self.angle);
            let damping_force = -DAMPING_B * self.velocity;
            self.velocity += (spring_force + damping_force) * dt;

            // Integrate position
            self.angle += self.velocity * dt;
            self.clamp_angle_lower();

            self.check_and_snap_settling()
        } else {
            false
        }
    }

    /// Check if settled during settling phase and snap to stable point.
    fn check_and_snap_settling(&mut self) -> bool {
        if self.velocity.abs() < SETTLE_VELOCITY_THRESHOLD {
            let nearest_stable = self.nearest_stable();

            if (self.angle - nearest_stable).abs() < SETTLE_ANGLE_THRESHOLD {
                // If we must reach opposite and we're not there yet, don't snap
                if self.face_from_angle() != self.target_face {
                    // Wrong face - don't snap yet, let nudge push us over
                    return true;
                }

                // debug!(
                //     "Settling with velocity {} and angle delta {} less than thresholds {} and {}",
                //     self.velocity,
                //     (self.angle - nearest_stable),
                //     SETTLE_VELOCITY_THRESHOLD,
                //     SETTLE_ANGLE_THRESHOLD
                // );

                self.phase = InteractionPhase::Idle;
                self.velocity = 0.0;
                self.angle = nearest_stable;

                return false; // No longer active
            }
        }

        self.velocity.abs() > SETTLE_VELOCITY_THRESHOLD
    }

    /// Prevent angle from going negative (bounce at 0).
    fn clamp_angle_lower(&mut self) {
        if self.angle < 0.0 {
            self.angle = 0.0;
        }
    }

    // ========== QUERIES ==========

    /// Which face to render.
    /// During Holding phase, returns target_face to ensure texture stays loaded.
    /// Otherwise calculates from angle for correct visuals.
    pub fn face_from_angle(&self) -> PosterFace {
        let position = self.angle.rem_euclid(PI * 2.0);
        if (FRAC_PI_2..PI + FRAC_PI_2).contains(&position) {
            PosterFace::Back
        } else {
            PosterFace::Front
        }
    }

    /// Whether animation has settled at target.
    pub fn is_settled(&self) -> bool {
        let target = match self.target_face {
            PosterFace::Back => PI,
            PosterFace::Front => 0.0,
        };
        matches!(self.phase, InteractionPhase::Idle)
            && self.velocity.abs() < SETTLE_VELOCITY_THRESHOLD
            && (self.angle - target).abs() < SETTLE_ANGLE_THRESHOLD
            && self.hold_start_time.is_none()
    }

    pub fn next_stable(&self) -> f32 {
        let n_rotate = (self.angle / PI).ceil().max(1.0);

        let offset = (n_rotate % 2.0) * PI;

        match self.target_face {
            PosterFace::Front => n_rotate * PI + offset,
            PosterFace::Back => n_rotate * PI,
        }
    }

    pub fn nearest_stable(&self) -> f32 {
        let n = (self.angle / PI).round();
        n * PI
    }
}
