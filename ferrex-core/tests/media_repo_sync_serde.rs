//! Serde roundtrip tests for media repository sync types.

use ferrex_core::api::types::MovieBatchVersionManifestEntry;
use ferrex_core::types::ids::MovieBatchId;

#[test]
fn movie_batch_manifest_entry_deserializes_without_content_hash() {
    let json = r#"{"batch_id": 7, "version": 42}"#;
    let parsed: MovieBatchVersionManifestEntry =
        serde_json::from_str(json).expect("deserialize");

    assert_eq!(parsed.batch_id, MovieBatchId(7));
    assert_eq!(parsed.version, 42);
    assert_eq!(parsed.content_hash, None);
}

#[test]
fn movie_batch_manifest_entry_serializes_with_content_hash_when_present() {
    let entry = MovieBatchVersionManifestEntry {
        batch_id: MovieBatchId(7),
        version: 42,
        content_hash: Some(123),
    };

    let value = serde_json::to_value(entry).expect("serialize");
    assert_eq!(value["batch_id"], 7);
    assert_eq!(value["version"], 42);
    assert_eq!(value["content_hash"], 123);
}
