use std::{any::type_name_of_val, fmt, net::IpAddr, sync::Arc};

use chrono::{DateTime, Duration, Utc};
use rand::{TryRngCore, rngs::OsRng};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    database::repository_ports::setup_claims::{
        NewSetupClaim, SetupClaimsRepository,
    },
    domain::users::auth::AuthCrypto,
    error::MediaError,
};

const DEFAULT_CLAIM_TTL_MINUTES: i64 = 10;
const CLAIM_CODE_LENGTH: usize = 6;
const CLAIM_TOKEN_LENGTH: usize = 32;

/// Provides business logic for the first-run setup claim workflow.
#[derive(Clone)]
pub struct SetupClaimService<R>
where
    R: SetupClaimsRepository + ?Sized,
{
    repository: Arc<R>,
    crypto: Arc<AuthCrypto>,
    claim_ttl: Duration,
}

impl<R> fmt::Debug for SetupClaimService<R>
where
    R: SetupClaimsRepository + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupClaimService")
            .field("repository", &type_name_of_val(self.repository.as_ref()))
            .field("crypto", &type_name_of_val(self.crypto.as_ref()))
            .field("claim_ttl", &self.claim_ttl)
            .finish()
    }
}

impl<R> SetupClaimService<R>
where
    R: SetupClaimsRepository + ?Sized,
{
    pub fn new(repository: Arc<R>, crypto: Arc<AuthCrypto>) -> Self {
        Self {
            repository,
            crypto,
            claim_ttl: Duration::minutes(DEFAULT_CLAIM_TTL_MINUTES),
        }
    }

    /// Override the default claim TTL (primarily for tests).
    pub fn with_claim_ttl(mut self, ttl: Duration) -> Self {
        self.claim_ttl = ttl;
        self
    }

    pub async fn start_claim(
        &self,
        client_name: Option<String>,
        client_ip: Option<IpAddr>,
    ) -> Result<StartedClaim, SetupClaimError> {
        let now = Utc::now();

        if let Some(active) = self
            .repository
            .get_active(now)
            .await
            .map_err(SetupClaimError::from)?
        {
            return Err(SetupClaimError::ActiveClaimPending {
                claim_id: active.id,
                expires_at: active.expires_at,
            });
        }

        let claim_code = generate_claim_code();
        let code_hash = self.crypto.hash_token(&claim_code);
        let expires_at = now + self.claim_ttl;

        let record = self
            .repository
            .create(NewSetupClaim {
                code_hash,
                expires_at,
                client_name,
                client_ip,
            })
            .await
            .map_err(SetupClaimError::from)?;

        Ok(StartedClaim {
            claim_id: record.id,
            claim_code,
            expires_at: record.expires_at,
        })
    }

    pub async fn confirm_claim(
        &self,
        code: &str,
    ) -> Result<ConfirmedClaim, SetupClaimError> {
        if code.trim().is_empty() {
            return Err(SetupClaimError::InvalidCode);
        }

        let now = Utc::now();
        let hashed = self.crypto.hash_token(code);

        let record = self
            .repository
            .find_active_by_code_hash(&hashed, now)
            .await
            .map_err(SetupClaimError::from)?
            .ok_or(SetupClaimError::InvalidCode)?;

        self.repository
            .increment_attempt(record.id, now)
            .await
            .map_err(SetupClaimError::from)?;

        if record.expires_at <= now {
            return Err(SetupClaimError::Expired {
                expired_at: record.expires_at,
            });
        }

        let claim_token = generate_claim_token();
        let token_hash = self.crypto.hash_token(&claim_token);
        let updated = self
            .repository
            .mark_confirmed(record.id, token_hash, now)
            .await
            .map_err(SetupClaimError::from)?;

        Ok(ConfirmedClaim {
            claim_id: updated.id,
            claim_token,
            expires_at: updated.expires_at,
        })
    }

    pub async fn validate_claim_token(
        &self,
        token: &str,
    ) -> Result<ValidatedClaimToken, SetupClaimError> {
        if token.trim().is_empty() {
            return Err(SetupClaimError::InvalidToken);
        }

        let now = Utc::now();
        let hashed = self.crypto.hash_token(token);
        let record = self
            .repository
            .find_confirmed_by_token_hash(&hashed, now)
            .await
            .map_err(SetupClaimError::from)?
            .ok_or(SetupClaimError::InvalidToken)?;

        if record.expires_at <= now {
            return Err(SetupClaimError::Expired {
                expired_at: record.expires_at,
            });
        }

        Ok(ValidatedClaimToken {
            claim_id: record.id,
            expires_at: record.expires_at,
        })
    }

    pub async fn consume_claim_token(
        &self,
        token: &str,
    ) -> Result<ConsumedClaim, SetupClaimError> {
        let validated = self.validate_claim_token(token).await?;
        let now = Utc::now();

        let revoked = self
            .repository
            .revoke_by_id(validated.claim_id, Some("claim token consumed"), now)
            .await
            .map_err(SetupClaimError::from)?;

        Ok(ConsumedClaim {
            claim_id: revoked.id,
            expires_at: revoked.expires_at,
        })
    }

    pub async fn revoke_all(
        &self,
        reason: Option<&str>,
    ) -> Result<u64, SetupClaimError> {
        let now = Utc::now();
        self.repository
            .revoke_all(reason, now)
            .await
            .map_err(SetupClaimError::from)
    }

    pub async fn purge_stale(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<u64, SetupClaimError> {
        self.repository
            .purge_stale(older_than)
            .await
            .map_err(SetupClaimError::from)
    }
}

#[derive(Debug, Error)]
pub enum SetupClaimError {
    #[error("a claim is already pending until {expires_at}")]
    ActiveClaimPending {
        claim_id: Uuid,
        expires_at: DateTime<Utc>,
    },
    #[error("claim code is invalid or expired")]
    InvalidCode,
    #[error("claim token is invalid or expired")]
    InvalidToken,
    #[error("claim expired at {expired_at}")]
    Expired { expired_at: DateTime<Utc> },
    #[error(transparent)]
    Storage(#[from] MediaError),
}

#[derive(Debug, Clone)]
pub struct StartedClaim {
    pub claim_id: Uuid,
    pub claim_code: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ConfirmedClaim {
    pub claim_id: Uuid,
    pub claim_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ConsumedClaim {
    pub claim_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ValidatedClaimToken {
    pub claim_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

fn generate_claim_code() -> String {
    random_string(CLAIM_CODE_LENGTH)
}

fn generate_claim_token() -> String {
    random_string(CLAIM_TOKEN_LENGTH)
}

fn random_string(length: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut bytes = vec![0u8; length];
    let mut rng = OsRng;
    rng.try_fill_bytes(&mut bytes)
        .expect("secure random generation available");

    let mut out = String::with_capacity(length);
    for byte in bytes {
        let idx = (byte as usize) % ALPHABET.len();
        out.push(ALPHABET[idx] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repository_ports::setup_claims::{
        SetupClaimRecord, SetupClaimsRepository,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    struct InMemoryRepo {
        claims: Mutex<HashMap<Uuid, SetupClaimRecord>>,
    }

    impl InMemoryRepo {
        fn new() -> Self {
            Self {
                claims: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl SetupClaimsRepository for InMemoryRepo {
        async fn create(
            &self,
            claim: NewSetupClaim,
        ) -> Result<SetupClaimRecord, MediaError> {
            let record = SetupClaimRecord {
                id: Uuid::new_v4(),
                code_hash: claim.code_hash,
                claim_token_hash: None,
                created_at: Utc::now(),
                expires_at: claim.expires_at,
                confirmed_at: None,
                client_name: claim.client_name,
                client_ip: claim.client_ip,
                attempts: 0,
                last_attempt_at: None,
                revoked_at: None,
                revoked_reason: None,
            };
            self.claims.lock().await.insert(record.id, record.clone());
            Ok(record)
        }

        async fn get_active(
            &self,
            now: DateTime<Utc>,
        ) -> Result<Option<SetupClaimRecord>, MediaError> {
            let claims = self.claims.lock().await;
            Ok(claims
                .values()
                .find(|record| {
                    record.confirmed_at.is_none()
                        && record.revoked_at.is_none()
                        && record.expires_at > now
                })
                .cloned())
        }

        async fn find_active_by_code_hash(
            &self,
            code_hash: &str,
            now: DateTime<Utc>,
        ) -> Result<Option<SetupClaimRecord>, MediaError> {
            let claims = self.claims.lock().await;
            Ok(claims
                .values()
                .filter(|record| record.code_hash == code_hash)
                .filter(|record| record.confirmed_at.is_none())
                .filter(|record| record.revoked_at.is_none())
                .find(|record| record.expires_at > now)
                .cloned())
        }

        async fn mark_confirmed(
            &self,
            id: Uuid,
            token_hash: String,
            now: DateTime<Utc>,
        ) -> Result<SetupClaimRecord, MediaError> {
            let mut claims = self.claims.lock().await;
            let record = claims.get_mut(&id).unwrap();
            record.claim_token_hash = Some(token_hash);
            record.confirmed_at = Some(now);
            record.last_attempt_at = Some(now);
            Ok(record.clone())
        }

        async fn increment_attempt(
            &self,
            id: Uuid,
            now: DateTime<Utc>,
        ) -> Result<(), MediaError> {
            let mut claims = self.claims.lock().await;
            let record = claims.get_mut(&id).unwrap();
            record.attempts += 1;
            record.last_attempt_at = Some(now);
            Ok(())
        }

        async fn find_confirmed_by_token_hash(
            &self,
            token_hash: &str,
            now: DateTime<Utc>,
        ) -> Result<Option<SetupClaimRecord>, MediaError> {
            let claims = self.claims.lock().await;
            Ok(claims
                .values()
                .filter(|record| {
                    record.claim_token_hash.as_deref() == Some(token_hash)
                })
                .filter(|record| record.revoked_at.is_none())
                .filter(|record| record.confirmed_at.is_some())
                .find(|record| record.expires_at > now)
                .cloned())
        }

        async fn revoke_by_id(
            &self,
            id: Uuid,
            reason: Option<&str>,
            now: DateTime<Utc>,
        ) -> Result<SetupClaimRecord, MediaError> {
            let mut claims = self.claims.lock().await;
            let record = claims.get_mut(&id).unwrap();
            record.revoked_at = Some(now);
            record.revoked_reason = reason.map(|s| s.to_string());
            Ok(record.clone())
        }

        async fn revoke_all(
            &self,
            reason: Option<&str>,
            now: DateTime<Utc>,
        ) -> Result<u64, MediaError> {
            let mut claims = self.claims.lock().await;
            let mut count = 0;
            for record in claims.values_mut() {
                if record.revoked_at.is_none() {
                    record.revoked_at = Some(now);
                    record.revoked_reason = reason.map(|s| s.to_string());
                    count += 1;
                }
            }
            Ok(count)
        }

        async fn purge_stale(
            &self,
            before: DateTime<Utc>,
        ) -> Result<u64, MediaError> {
            let mut claims = self.claims.lock().await;
            let initial = claims.len();
            claims.retain(|_, record| {
                if let Some(revoked) = record.revoked_at {
                    return revoked >= before;
                }
                record.confirmed_at.is_some() || record.expires_at >= before
            });
            Ok((initial - claims.len()) as u64)
        }
    }

    fn build_crypto() -> Arc<AuthCrypto> {
        Arc::new(AuthCrypto::new("pepper", "token-key").unwrap())
    }

    #[tokio::test]
    async fn starting_claim_generates_code() {
        let repo = Arc::new(InMemoryRepo::new());
        let service = SetupClaimService::new(repo, build_crypto());

        let result = service
            .start_claim(Some("Player".into()), None)
            .await
            .expect("start claim");

        assert_eq!(result.claim_code.len(), CLAIM_CODE_LENGTH);
    }

    #[tokio::test]
    async fn cannot_start_two_claims_simultaneously() {
        let repo = Arc::new(InMemoryRepo::new());
        let service = SetupClaimService::new(repo.clone(), build_crypto());

        let first = service
            .start_claim(Some("Player".into()), None)
            .await
            .expect("first claim");

        let second = service.start_claim(None, None).await;
        assert!(matches!(
            second,
            Err(SetupClaimError::ActiveClaimPending { claim_id, .. }) if claim_id == first.claim_id
        ));
    }
}
