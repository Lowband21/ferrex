use crate::Result;

pub trait TransactionHandle: Sized {
    fn commit(self) -> Result<()>;
    fn rollback(self) -> Result<()>;
}

pub trait UnitOfWork: Send + Sync {
    type Transaction<'tx>: TransactionHandle + 'tx
    where
        Self: 'tx;

    fn begin(&self) -> Result<Self::Transaction<'_>>;
}
