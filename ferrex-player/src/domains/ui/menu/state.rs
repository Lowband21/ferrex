use crate::infra::widgets::poster::PosterFace;
use std::time::Instant;

const ANGULAR_IMPULSE: f32 = 5.0;
const MAX_ANGULAR_VELOCITY: f32 = std::f32::consts::PI * 16.0; // faster max spin
const DAMPING: f32 = 20.0;
const SETTLE_SPEED: f32 = std::f32::consts::PI * 2.2;
const HOLD_ACCEL: f32 = std::f32::consts::PI * 10.0; // stronger acceleration while holding
const HOLD_MAX_QUEUE: u32 = 8;
const HOLD_QUEUE_DELAY: f32 = 0.06; // seconds before queuing extra flips
const HOLD_QUEUE_INTERVAL: f32 = 0.06; // cadence for additional queued spins

/// Per-poster flip state that drives the right-click menu animation.
#[derive(Debug, Clone)]
pub struct PosterMenuState {
    /// Current rotation in radians (0 = front, PI = back).
    pub angle: f32,
    /// Desired face once queues drain.
    pub target_face: PosterFace,
    /// Spin velocity in radians/sec.
    pub angular_velocity: f32,
    /// Extra queued half-rotations (added by re-triggers).
    pub queued_flips: u32,
    /// Last time the integrator ran.
    pub last_update: Instant,
    /// Whether the right button is being held down on this poster.
    pub hold_active: bool,
    /// How long the current hold has been active (seconds).
    pub hold_time: f32,
    /// Accumulator for queuing additional flips during hold.
    pub hold_queue_accum: f32,
}

impl PosterMenuState {
    pub fn new(now: Instant) -> Self {
        Self {
            angle: 0.0,
            target_face: PosterFace::Front,
            angular_velocity: 0.0,
            queued_flips: 0,
            last_update: now,
            hold_active: false,
            hold_time: 0.0,
            hold_queue_accum: 0.0,
        }
    }

    /// Apply a click impulse: retarget face, queue a flip, and add angular energy.
    pub fn apply_impulse(&mut self, target_face: PosterFace, now: Instant) {
        let same_face = self.target_face == target_face;
        self.target_face = target_face;
        // Only queue an extra flip when re-triggering the same face; the first transition
        // relies on angular_velocity + settle to reach the target without double-flipping.
        if same_face {
            self.queued_flips = self.queued_flips.saturating_add(1);
        } else {
            self.queued_flips = 0;
        }
        self.angular_velocity =
            (self.angular_velocity + ANGULAR_IMPULSE).min(MAX_ANGULAR_VELOCITY);
        self.last_update = now;
    }

    /// Advance the state machine; returns true while motion is active.
    pub fn step(&mut self, now: Instant) -> bool {
        let dt = (now - self.last_update).as_secs_f32().min(0.05);
        self.last_update = now;

        let mut active = false;

        if self.hold_active {
            // Accelerate toward max while holding and queue periodic spins
            self.angular_velocity = (self.angular_velocity + HOLD_ACCEL * dt)
                .min(MAX_ANGULAR_VELOCITY);
            self.hold_time += dt;
            if self.hold_time >= HOLD_QUEUE_DELAY {
                self.hold_queue_accum += dt;
                while self.hold_queue_accum >= HOLD_QUEUE_INTERVAL
                    && self.queued_flips < HOLD_MAX_QUEUE
                {
                    self.queued_flips += 1;
                    self.hold_queue_accum -= HOLD_QUEUE_INTERVAL;
                }
            }
        } else {
            self.hold_time = 0.0;
            self.hold_queue_accum = 0.0;
        }

        let dir = if matches!(self.target_face, PosterFace::Back) {
            1.0
        } else {
            -1.0
        };

        if self.angular_velocity > 0.0 || self.queued_flips > 0 {
            self.angle += dir * self.angular_velocity * dt;
            active = true;

            let pi = std::f32::consts::PI;
            while self.angle > pi {
                if self.queued_flips > 0 {
                    self.angle -= pi;
                    self.queued_flips -= 1;
                    active = true;
                } else {
                    self.angle = pi;
                    break;
                }
            }
            while self.angle < 0.0 {
                if self.queued_flips > 0 {
                    self.angle += pi;
                    self.queued_flips -= 1;
                    active = true;
                } else {
                    self.angle = 0.0;
                    break;
                }
            }

            // Friction-like damping
            let damp = DAMPING * dt;
            if self.angular_velocity > damp {
                self.angular_velocity -= damp;
            } else {
                self.angular_velocity = 0.0;
            }
        }

        // Settle toward the target pose
        let target_angle = match self.target_face {
            PosterFace::Back => std::f32::consts::PI,
            PosterFace::Front => 0.0,
        };
        let delta = target_angle - self.angle;
        if delta.abs() > 0.001 {
            let settle_step =
                delta.clamp(-SETTLE_SPEED * dt, SETTLE_SPEED * dt);
            self.angle += settle_step;
            active = true;
        }

        active
    }

    pub fn face_for_render(&self) -> PosterFace {
        if self.angle >= std::f32::consts::FRAC_PI_2 {
            PosterFace::Back
        } else {
            PosterFace::Front
        }
    }

    pub fn progress(&self) -> f32 {
        (self.angle / std::f32::consts::PI).clamp(0.0, 1.0)
    }

    pub fn is_settled(&self) -> bool {
        let target_angle = match self.target_face {
            PosterFace::Back => std::f32::consts::PI,
            PosterFace::Front => 0.0,
        };
        self.queued_flips == 0
            && self.angular_velocity < 0.05
            && (self.angle - target_angle).abs() < 0.002
    }
}
