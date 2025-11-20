use std::path::PathBuf;

use ferrex_player::app::{self, bootstrap::AppConfig};

#[test]
fn ui_end_to_end() -> Result<(), iced_test::Error> {
    let config = AppConfig::new("https://localhost:3000").with_test_stubs(true);
    let program = app::application(config);

    let tests_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ui");

    iced_test::run(program, tests_dir)
}
