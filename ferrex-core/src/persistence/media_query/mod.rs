//! Read-focused persistence traits for media discovery and playback APIs.

pub trait MediaQueryRepository<'tx>: Send {
    type TransactionCtx;
}
