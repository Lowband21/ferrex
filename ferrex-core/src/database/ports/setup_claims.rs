use std::net::IpAddr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct NewSetupClaim {
    pub code_hash: String,
    pub expires_at: DateTime<Utc>,
    pub client_name: Option<String>,
    pub client_ip: Option<IpAddr>,
}

#[derive(Debug, Clone)]
pub struct SetupClaimRecord {
    pub id: Uuid,
    pub code_hash: String,
    pub claim_token_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub client_name: Option<String>,
    pub client_ip: Option<IpAddr>,
    pub attempts: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
}

#[async_trait]
pub trait SetupClaimsRepository: Send + Sync {
    async fn create(&self, claim: NewSetupClaim) -> Result<SetupClaimRecord>;

    /// Return the most recent claim that is neither confirmed nor revoked and has
    /// not expired.
    async fn get_active(&self, now: DateTime<Utc>) -> Result<Option<SetupClaimRecord>>;

    /// Lookup a claim by matching the stored code hash. Expired or revoked
    /// claims should be ignored.
    async fn find_active_by_code_hash(
        &self,
        code_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SetupClaimRecord>>;

    /// Mark a claim as confirmed and persist the claim token hash. Implementations
    /// should return the updated record.
    async fn mark_confirmed(
        &self,
        id: Uuid,
        token_hash: String,
        now: DateTime<Utc>,
    ) -> Result<SetupClaimRecord>;

    /// Increment the attempt counter for diagnostic purposes.
    async fn increment_attempt(&self, id: Uuid, now: DateTime<Utc>) -> Result<()>;

    /// Find a confirmed claim by token hash (used when creating the admin after
    /// claim confirmation). Implementations should ensure revoked claims are
    /// not returned.
    async fn find_confirmed_by_token_hash(
        &self,
        token_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SetupClaimRecord>>;

    /// Revoke a specific claim (after token consumption) returning the updated
    /// record for auditing.
    async fn revoke_by_id(
        &self,
        id: Uuid,
        reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<SetupClaimRecord>;

    /// Revoke every claim (used by the CLI) returning the number of rows affected.
    async fn revoke_all(&self, reason: Option<&str>, now: DateTime<Utc>) -> Result<u64>;

    /// Remove stale revoked/expired rows to keep the table lean. Returns the
    /// number of rows removed.
    async fn purge_stale(&self, before: DateTime<Utc>) -> Result<u64>;
}
