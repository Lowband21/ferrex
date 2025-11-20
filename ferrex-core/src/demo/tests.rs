use crate::demo::{
    DemoPolicy, allow_zero_length_for, clear_registered_libraries, init_demo_context,
    register_demo_library,
};
use crate::types::{
    ids::LibraryID,
    library::{Library, LibraryType},
};
use std::path::PathBuf;
use std::sync::Once;

#[test]
fn allow_zero_length_is_true_only_for_registered_demo_library() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        init_demo_context(
            PathBuf::from("/demo"),
            DemoPolicy {
                allow_zero_length_files: true,
                skip_metadata_probe: false,
            },
        )
        .expect("init demo context");
    });

    clear_registered_libraries();

    let library = Library {
        id: LibraryID::new(),
        name: "Demo Movies".into(),
        library_type: LibraryType::Movies,
        paths: vec![PathBuf::from("/demo")],
        scan_interval_minutes: 60,
        last_scan: None,
        enabled: true,
        auto_scan: true,
        watch_for_changes: false,
        analyze_on_scan: false,
        max_retry_attempts: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        media: None,
    };

    assert!(!allow_zero_length_for(&library.id));

    register_demo_library(&library);
    assert!(allow_zero_length_for(&library.id));

    let stranger = LibraryID::new();
    assert!(!allow_zero_length_for(&stranger));
}
