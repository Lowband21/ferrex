use anyhow::{Context, Result, anyhow};
use reqwest::Url;

pub const DEMO_DATABASE_NAME: &str = "ferrex_demo";

pub fn validate_primary_database_url(base: &str) -> Result<()> {
    let url = Url::parse(base).context("invalid PostgreSQL URL")?;
    ensure_not_demo_database(&url)
}

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
