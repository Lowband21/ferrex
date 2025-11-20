//! Persistence traits for shared catalogs such as people, roles, and genres.

pub trait PeopleAndTagsRepository<'tx>: Send {
    type TransactionCtx;
}
