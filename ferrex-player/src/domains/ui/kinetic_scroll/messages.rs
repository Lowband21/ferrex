#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
pub enum KineticMessage {
    Start(Direction),
    Stop(Direction),
    Tick,
    SetBoost(bool),
}
