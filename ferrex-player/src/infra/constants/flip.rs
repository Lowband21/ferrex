use std::f32::consts::PI;

pub const HOLD_THRESHOLD: f32 = 1.0; // Threshold for "long hold" vs "click"

// Acceleration and velocity constants
/// Base acceleration while holding
pub const BASE_ACCEL: f32 = PI * 1.0;
/// Max acceleration while holding
pub const MAX_ACCEL: f32 = PI * 8.0;
/// Scaling factor for accel ramp rate
pub const JERK_FACTOR: f32 = 1.5;
/// Impulse applied on successive clicks
pub const CLICK_IMPULSE: f32 = PI * 4.0;
/// Impulse to ensure landing on opposite face
pub const NUDGE_IMPULSE: f32 = PI * 3.0;
/// Velocity cap
pub const MAX_VELOCITY: f32 = PI * 10.0;

// Spring-damper physics constants
/// Spring stiffness for click animation
pub const SPRING_K: f32 = 80.0;
/// Damping coefficient for click animation
pub const DAMPING_B: f32 = 20.0;
/// Damping coefficient for settling
pub const SETTLE_DAMPING_B: f32 = 6.0;
/// Periodic snap strength (sine based gravity wells)
pub const SNAP_K: f32 = 30.0;

// Settling constants
/// rad/s - when to consider snapping
pub const SETTLE_VELOCITY_THRESHOLD: f32 = 0.01;
/// ~0.1 radians - proximity to stable point
pub const SETTLE_ANGLE_THRESHOLD: f32 = 0.0314;
/// rad/s - when to nudge toward opposite
pub const NUDGE_VELOCITY_THRESHOLD: f32 = 2.0;
