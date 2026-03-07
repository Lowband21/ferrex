use std::fs;

use tokio::process::Command;

use anyhow::{Context, Result, bail};
use tracing::warn;

use crate::cli::{options, stack::StackMode};

/// Detect whether the env contents contain placeholders that warrant re-running init.
///
/// This checks for:
/// - Empty file
/// - `changeme_` prefix in secret fields (not in comments or unrelated fields)
/// - `/change/me` placeholder for MEDIA_ROOT
///
/// It does NOT trigger on:
/// - Placeholder patterns in comments
/// - Valid paths or values that happen to contain these substrings
pub fn env_contents_have_placeholders(contents: &str) -> bool {
    if contents.trim().is_empty() {
        return true;
    }

    // Parse as key-value pairs to check only values, not comments
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            // Check for changeme_ in secret fields only
            let is_secret_field = matches!(
                key,
                "DATABASE_APP_PASSWORD"
                    | "DATABASE_ADMIN_PASSWORD"
                    | "AUTH_PASSWORD_PEPPER"
                    | "AUTH_TOKEN_KEY"
                    | "FERREX_SETUP_TOKEN"
            );

            if is_secret_field && value.starts_with("changeme_") {
                return true;
            }

            // Check for /change/me only in MEDIA_ROOT
            if key == "MEDIA_ROOT" && value == "/change/me" {
                return true;
            }
        }
    }

    false
}

/// Run the config init tool if the env file is missing, empty, or placeholders are present,
/// or when the caller explicitly requests it.
pub async fn ensure_env_initialized(
    opts: &options::StackOptions,
) -> Result<()> {
    // Determine if init needs to run at all
    let need_init_rerun = opts.force_init || opts.reset_db;

    let existing = fs::read_to_string(&opts.env_file).unwrap_or_default();
    let needs_init =
        existing.is_empty() || env_contents_have_placeholders(&existing);

    if !needs_init && !need_init_rerun {
        return Ok(());
    }

    // Log more context about why init is running
    if needs_init && !opts.reset_db {
        warn!(
            env_file = %opts.env_file.display(),
            "Init will run due to placeholder detection, but --reset-db is not set. \
             DB credentials will NOT be rotated to prevent auth mismatch with existing postgres data."
        );
    } else if need_init_rerun {
        warn!(
            env_file = %opts.env_file.display(),
            force_init = opts.force_init,
            reset_db = opts.reset_db,
            "Initialization explicitly requested"
        );
    } else {
        warn!("Initialization detected as required");
    }

    // Run `ferrexctl init` via the current binary to reuse the existing CLI surface.
    let exe =
        std::env::current_exe().context("failed to locate ferrexctl binary")?;
    let mut cmd = Command::new(exe);
    cmd.arg("init").arg("--env-file").arg(&opts.env_file);

    if opts.init_non_interactive {
        cmd.arg("--non-interactive");
    }
    if opts.init_advanced {
        cmd.arg("--advanced");
    }
    if opts.init_tui {
        cmd.arg("--tui");
    }
    if matches!(opts.mode, StackMode::Tailscale) {
        cmd.arg("--tailscale");
    }

    // --force: regenerate AUTH credentials only (when env is broken or user explicitly requests)
    // Note: --force no longer rotates DB credentials to prevent broken state.
    // DB rotation only happens via --rotate db with FERREX_INTERNAL_DB_RESET set.
    if opts.force_init || needs_init {
        cmd.arg("--force");
    }

    // --rotate db: rotate only DB credentials when resetting database
    // This is the ONLY path that rotates DB credentials, and it's safe because
    // the postgres volume will be deleted by --reset-db after init runs, and
    // --force-recreate ensures the db container is recreated with new credentials.
    // Set FERREX_INTERNAL_DB_RESET to signal that DB rotation is safe.
    if opts.reset_db {
        cmd.env("FERREX_INTERNAL_DB_RESET", "1");
        cmd.arg("--rotate").arg("db");
    }

    let status = cmd.status().await.context("failed to run ferrexctl init")?;
    if !status.success() {
        bail!("ferrexctl init exited with {}", status);
    }
    Ok(())
}
