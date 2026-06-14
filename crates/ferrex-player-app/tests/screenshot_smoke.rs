use std::{fs, panic};

use ferrex_player_app::screenshot::{
    self, ScreenshotError, ScreenshotPreset, ScreenshotSpec, Viewport,
};
use iced_test::emulator::Mode;

#[test]
fn screenshot_smoke_captures_small_png_or_skips_without_renderer() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let output = temp_dir.path().join("ferrex-screenshot-smoke.png");
    let spec = ScreenshotSpec {
        preset: ScreenshotPreset::FirstRunAuth,
        viewport: Viewport {
            width: 320,
            height: 180,
        },
        scale_factor: 1.0,
        mode: Mode::Immediate,
        settle_ms: 0,
        output: output.clone(),
        ice: None,
        ice_metadata: None,
    };

    match panic::catch_unwind(panic::AssertUnwindSafe(|| {
        screenshot::capture(&spec)
    })) {
        Ok(Ok(capture)) => {
            assert_eq!(capture.png_path, output);
            assert!(capture.png_path.exists(), "PNG should be written");
            assert!(
                capture.metadata_path.exists(),
                "metadata sidecar should be written"
            );

            let png = fs::read(&capture.png_path).expect("read smoke PNG");
            assert!(
                png.starts_with(&[0x89, b'P', b'N', b'G']),
                "smoke output should be a PNG"
            );
        }
        Ok(Err(ScreenshotError::RendererInit { .. })) => {
            eprintln!("{}", screenshot::renderer_unavailable_skip_reason());
        }
        Ok(Err(error)) => panic!("screenshot smoke capture failed: {error}"),
        Err(payload) if is_renderer_init_panic(&payload) => {
            eprintln!("{}", screenshot::renderer_unavailable_skip_reason());
        }
        Err(payload) => panic::resume_unwind(payload),
    }
}

fn is_renderer_init_panic(payload: &Box<dyn std::any::Any + Send>) -> bool {
    let message = if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        String::new()
    };

    message.contains("Create emulator renderer")
}
