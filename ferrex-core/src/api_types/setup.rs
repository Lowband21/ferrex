//! API DTOs for secure setup claim workflow.
//! Shared between the server and client crates so the request/response
//! contracts stay in sync.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request payload for starting a secure setup claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartClaimRequest {
    #[serde(default)]
    pub device_name: Option<String>,
}

/// Response returned when a secure setup claim is started.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartClaimResponse {
    pub claim_id: Uuid,
    pub claim_code: String,
    pub expires_at: DateTime<Utc>,
    pub lan_only: bool,
}

/// Request payload for confirming a setup claim with the generated code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmClaimRequest {
    pub claim_code: String,
}

/// Response returned when a setup claim is confirmed and a token is issued.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmClaimResponse {
    pub claim_id: Uuid,
    pub claim_token: String,
    pub expires_at: DateTime<Utc>,
}
