use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum PosterMenuMessage {
    Toggle(Uuid),
    Close(Uuid),
    HoldStart(Uuid),
    HoldEnd(Uuid),
}
