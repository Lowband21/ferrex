use std::{panic, path::PathBuf};

use ferrex_player_app::app::{self, bootstrap::AppConfig};

#[test]
fn ui_end_to_end() -> Result<(), iced_test::Error> {
    let config = AppConfig::new("https://localhost:3000").with_test_stubs(true);
    let program = app::application(config);

    let tests_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ui");

    match panic::catch_unwind(panic::AssertUnwindSafe(|| {
        iced_test::run(program, tests_dir)
    })) {
        Ok(result) => result,
        Err(payload)
            if panic_message(&payload).contains("Create emulator renderer") =>
        {
            eprintln!(
                "skipping iced UI replay because the test renderer is unavailable in this environment"
            );
            Ok(())
        }
        Err(payload) => panic::resume_unwind(payload),
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        String::new()
    }
}
