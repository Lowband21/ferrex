use anyhow::{Context, Result, anyhow};
use reqwest::Url;
#[cfg(feature = "demo")]
use sqlx::{Connection, PgConnection};

pub const DEMO_DATABASE_NAME: &str = "ferrex_demo";

pub fn validate_primary_database_url(base: &str) -> Result<()> {
    let url = Url::parse(base).context("invalid PostgreSQL URL")?;
    ensure_not_demo_database(&url)
}

#[cfg(feature = "demo")]
pub fn derive_demo_database_url(base: &str) -> Result<String> {
    let mut url = Url::parse(base).context("invalid PostgreSQL URL")?;
    ensure_not_demo_database(&url)?;
    url.set_path(&format!("/{}", DEMO_DATABASE_NAME));
    Ok(url.into())
}

#[cfg(feature = "demo")]
pub async fn prepare_demo_database(base: &str) -> Result<String> {
    let base_url = Url::parse(base).context("invalid PostgreSQL URL")?;
    ensure_not_demo_database(&base_url)?;

    let demo_url = derive_demo_database_url(base)?;

    let mut admin_url = base_url.clone();
    admin_url.set_path("/postgres");
    let admin_url = admin_url.into_string();

    let mut connection =
        PgConnection::connect(&admin_url).await.with_context(|| {
            format!("failed to connect to admin database via {}", admin_url)
        })?;

    sqlx::query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1")
        .bind(DEMO_DATABASE_NAME)
        .execute(&mut connection)
        .await
        .context("failed to terminate active demo connections")?;

    let quoted_name = quote_ident(DEMO_DATABASE_NAME);
    let drop_stmt = format!("DROP DATABASE IF EXISTS {}", quoted_name);
    sqlx::query(&drop_stmt)
        .execute(&mut connection)
        .await
        .context("failed to drop existing demo database")?;

    let create_stmt = format!("CREATE DATABASE {}", quoted_name);
    sqlx::query(&create_stmt)
        .execute(&mut connection)
        .await
        .context("failed to create demo database")?;

    Ok(demo_url)
}

fn ensure_not_demo_database(url: &Url) -> Result<()> {
    let name = url.path().trim_start_matches('/');
    if name.is_empty() {
        return Err(anyhow!("database URL must include database name"));
    }
    if name.eq_ignore_ascii_case(DEMO_DATABASE_NAME) {
        return Err(anyhow!(
            "Primary database name `{}` is reserved for demo mode. Choose a different database for production runs.",
            DEMO_DATABASE_NAME
        ));
    }
    Ok(())
}

fn quote_ident(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}
