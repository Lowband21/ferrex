//! Round-trip tests: ferrex-model → FlatBuffers bytes → read back correct values.

use ferrex_flatbuffers::fb;
use ferrex_flatbuffers::conversions::library::serialize_library_list;
use ferrex_flatbuffers::uuid_helpers::{uuid_to_fb, fb_to_uuid};
use ferrex_model::{Library, LibraryType, MovieReferenceBatchSize};
use chrono::Utc;
use std::path::PathBuf;
use uuid::Uuid;

fn make_test_library(name: &str, lib_type: LibraryType) -> Library {
    use ferrex_model::library::LibraryLikeMut;
    let mut lib = Library::new(
        name.to_string(),
        lib_type,
        vec![PathBuf::from("/media/movies")],
    );
    lib.scan_interval_minutes = 120;
    lib.enabled = true;
    lib.auto_scan = true;
    lib
}

#[test]
fn uuid_round_trip() {
    let original = Uuid::new_v4();
    let fb_uuid = uuid_to_fb(&original);
    let back = fb_to_uuid(&fb_uuid);
    assert_eq!(original, back, "UUID round-trip failed");
}

#[test]
fn uuid_v7_round_trip() {
    let original = Uuid::now_v7();
    let fb_uuid = uuid_to_fb(&original);
    let back = fb_to_uuid(&fb_uuid);
    assert_eq!(original, back, "UUID v7 round-trip failed");
}

#[test]
fn library_list_round_trip() {
    let libraries = vec![
        make_test_library("Movies", LibraryType::Movies),
        make_test_library("TV Shows", LibraryType::Series),
    ];

    // Serialize to FlatBuffers
    let bytes = serialize_library_list(&libraries);
    assert!(!bytes.is_empty(), "Serialized bytes should not be empty");

    // Deserialize and verify
    let list = flatbuffers::root::<fb::library::LibraryList>(&bytes)
        .expect("Failed to parse LibraryList");

    let items = list.items().expect("items should be present");
    assert_eq!(items.len(), 2, "Should have 2 libraries");

    // Verify first library
    let lib0 = items.get(0);
    assert_eq!(lib0.name(), "Movies");
    assert_eq!(lib0.library_type(), fb::common::LibraryType::Movies);
    assert!(lib0.enabled());
    assert!(lib0.auto_scan());
    assert_eq!(lib0.scan_interval_minutes(), 120);
    assert_eq!(lib0.movie_ref_batch_size(), 100);

    // Verify UUID round-trip
    let original_id = libraries[0].id;
    let fb_id = lib0.id();
    let recovered_id = fb_to_uuid(fb_id);
    assert_eq!(*original_id.as_uuid(), recovered_id, "Library ID round-trip failed");

    // Verify paths
    if let Some(paths) = lib0.paths() {
        assert_eq!(paths.len(), 1);
        assert_eq!(paths.get(0), "/media/movies");
    } else {
        panic!("paths should be present");
    }

    // Verify second library
    let lib1 = items.get(1);
    assert_eq!(lib1.name(), "TV Shows");
    assert_eq!(lib1.library_type(), fb::common::LibraryType::Series);

    // Verify timestamps are non-zero
    let created = lib0.created_at().unwrap();
    assert_ne!(created.millis(), 0, "created_at should not be epoch 0");
}

#[test]
fn empty_library_list_round_trip() {
    let bytes = serialize_library_list(&[]);
    let list = flatbuffers::root::<fb::library::LibraryList>(&bytes)
        .expect("Failed to parse empty LibraryList");

    let items = list.items().expect("items should be present");
    assert_eq!(items.len(), 0);
}

#[test]
fn library_list_size_reasonable() {
    // Verify the serialized size is reasonable
    let libraries = vec![make_test_library("Test", LibraryType::Movies)];
    let bytes = serialize_library_list(&libraries);

    // A single library should be well under 1KB
    assert!(
        bytes.len() < 1024,
        "Single library FlatBuffer is {} bytes — expected < 1KB",
        bytes.len()
    );
}
