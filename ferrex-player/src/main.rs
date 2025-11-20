use ferrex_player::app::{self, AppConfig};

use env_logger::{Builder, Target};
use log::LevelFilter;

#[cfg(feature = "profile-with-tracy")]
use tracy_client;

fn init_logger() {
    Builder::new()
        .target(Target::Stdout)
        .filter_level(LevelFilter::Warn)
        .filter_module("ferrex-player", LevelFilter::Debug)
        .init();
}

fn main() -> iced::Result {
    if std::env::var("RUST_LOG").is_err() {
        log::warn!("Failed to initialize logger from env, falling back to default");
        init_logger();
    } else {
        log::warn!("Initializing logger from env");
        env_logger::init();
    }

    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    ferrex_player::infrastructure::profiling::init();

    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    log::info!("Profiling system initialized");

    #[cfg(feature = "profile-with-puffin")]
    log::info!(
        "Puffin server listening on 127.0.0.1:8585 - connect with: puffin_viewer --url 127.0.0.1:8585"
    );

    #[cfg(feature = "profile-with-tracy")]
    tracy_client::Client::start();

    let config = AppConfig::from_environment();

    app::application(config).run()
}
