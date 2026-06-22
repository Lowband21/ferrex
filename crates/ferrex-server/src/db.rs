//! Database URL guard rails for server startup.
//!
//! The server uses a dedicated demo database when demo mode is enabled. These
//! helpers prevent operators from accidentally pointing the primary production
//! connection at the reserved demo database name.

use anyhow::{Context, Result, anyhow};
use reqwest::Url;

/// Reserved database name used for demo-mode data.
pub const DEMO_DATABASE_NAME: &str = "ferrex_demo";

/// Validate that the primary database URL is syntactically valid and is not the demo database.
pub fn validate_primary_database_url(base: &str) -> Result<()> {
    let url = Url::parse(base).context("invalid PostgreSQL URL")?;
    ensure_not_demo_database(&url)
}

/// Derive the demo-mode database URL from the primary URL and reserved demo name.
#[cfg(feature = "demo")]
pub fn derive_demo_database_url(base: &str) -> Result<String> {
    let mut url = Url::parse(base).context("invalid PostgreSQL URL")?;
    ensure_not_demo_database(&url)?;
    let name = std::env::var("DEMO_DATABASE_NAME")
        .unwrap_or(DEMO_DATABASE_NAME.to_string());
    url.set_path(&format!("/{}", name));
    Ok(url.into())
}

fn ensure_not_demo_database(url: &Url) -> Result<()> {
    let name = url.path().trim_start_matches('/');
    if name.is_empty() {
        return Err(anyhow!("database URL must include database name"));
    }

    let demo_name = std::env::var("DEMO_DATABASE_NAME")
        .unwrap_or_else(|_| DEMO_DATABASE_NAME.to_string());
    if name.eq_ignore_ascii_case(&demo_name) {
        return Err(anyhow!(
            "Primary database name `{}` is reserved for demo mode. Choose a different database for production runs.",
            demo_name
        ));
    }
    Ok(())
}
