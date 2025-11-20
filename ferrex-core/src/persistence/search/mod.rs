//! Persistence contracts for search indexing and lookup facilities.

pub trait SearchRepository<'tx>: Send {
    type TransactionCtx;
}
