//! FlatBuffers → Rust request-struct conversions for mobile client POST bodies.
//!
//! Each implementation verifies the FlatBuffers buffer, then reads the typed
//! fields into the corresponding Rust struct used by the handler.

use crate::infra::content_negotiation::FromFlatbuffers;

// ── LoginRequest ────────────────────────────────────────────────────

impl FromFlatbuffers for ferrex_core::domain::users::user::LoginRequest {
    fn from_flatbuffers(bytes: &[u8]) -> Result<Self, String> {
        use ferrex_flatbuffers::fb::auth::LoginRequest as FbLogin;
        let fb = flatbuffers::root::<FbLogin>(bytes)
            .map_err(|e| format!("invalid LoginRequest FlatBuffer: {e}"))?;
        Ok(Self {
            username: fb.username().to_owned(),
            password: fb.password().to_owned(),
            device_name: fb.device_name().map(|s| s.to_owned()),
        })
    }
}

// ── RefreshRequest (server-side) ────────────────────────────────────

impl FromFlatbuffers for crate::handlers::users::auth::handlers::RefreshRequest {
    fn from_flatbuffers(bytes: &[u8]) -> Result<Self, String> {
        use ferrex_flatbuffers::fb::auth::RefreshRequest as FbRefresh;
        let fb = flatbuffers::root::<FbRefresh>(bytes)
            .map_err(|e| format!("invalid RefreshRequest FlatBuffer: {e}"))?;
        Ok(Self {
            refresh_token: fb.refresh_token().to_owned(),
        })
    }
}

// ── MovieBatchSyncRequest ───────────────────────────────────────────

impl FromFlatbuffers for ferrex_core::api::types::MovieBatchSyncRequest {
    fn from_flatbuffers(bytes: &[u8]) -> Result<Self, String> {
        use ferrex_flatbuffers::fb::library::BatchSyncRequest;
        let fb = flatbuffers::root::<BatchSyncRequest>(bytes)
            .map_err(|e| format!("invalid BatchSyncRequest FlatBuffer: {e}"))?;

        let mut batches = Vec::new();
        if let Some(versions) = fb.cached_versions() {
            for i in 0..versions.len() {
                let entry = versions.get(i);
                let batch_id = ferrex_core::types::MovieBatchId::new(entry.batch_id())
                    .map_err(|e| format!("invalid batch_id: {e}"))?;
                batches.push(ferrex_core::api::types::MovieBatchVersionManifestEntry {
                    batch_id,
                    version: entry.version(),
                    content_hash: None,
                });
            }
        }

        Ok(ferrex_core::api::types::MovieBatchSyncRequest { batches })
    }
}

// ── MovieBatchFetchRequest ──────────────────────────────────────────

impl FromFlatbuffers for ferrex_core::api::types::MovieBatchFetchRequest {
    fn from_flatbuffers(bytes: &[u8]) -> Result<Self, String> {
        use ferrex_flatbuffers::fb::library::BatchFetchRequest;
        let fb = flatbuffers::root::<BatchFetchRequest>(bytes)
            .map_err(|e| format!("invalid BatchFetchRequest FlatBuffer: {e}"))?;

        let mut batch_ids = Vec::new();
        if let Some(ids) = fb.batch_ids() {
            for i in 0..ids.len() {
                let id = ferrex_core::types::MovieBatchId::new(ids.get(i))
                    .map_err(|e| format!("invalid batch_id: {e}"))?;
                batch_ids.push(id);
            }
        }

        Ok(ferrex_core::api::types::MovieBatchFetchRequest { batch_ids })
    }
}

// ── MediaQuery ──────────────────────────────────────────────────────

impl FromFlatbuffers for ferrex_core::player_prelude::MediaQuery {
    fn from_flatbuffers(bytes: &[u8]) -> Result<Self, String> {
        // MediaQuery is complex — mobile clients still send it as JSON
        // embedded in the body. Fall back to JSON parsing.
        serde_json::from_slice(bytes)
            .map_err(|e| format!("MediaQuery FlatBuffers not supported, JSON parse failed: {e}"))
    }
}
