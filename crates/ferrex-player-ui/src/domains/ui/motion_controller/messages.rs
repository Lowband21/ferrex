use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
pub enum MotionMessage {
    Start(Direction),
    Stop(Direction),
    /// Frame-synchronized tick with timestamp from window::frames()
    Tick(Instant),
    SetBoost(bool),
}
