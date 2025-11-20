//! Persistence traits for artwork, images, and auxiliary media assets.

pub trait ArtworkRepository<'tx>: Send {
    type TransactionCtx;
}
