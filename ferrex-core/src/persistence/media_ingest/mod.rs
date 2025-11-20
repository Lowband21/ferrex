//! Persistence traits for write-heavy media ingestion flows.

pub trait MediaIngestRepository<'tx>: Send {
    type TransactionCtx;
}
