#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
pub enum MotionMessage {
    Start(Direction),
    Stop(Direction),
    Tick,
    SetBoost(bool),
}
