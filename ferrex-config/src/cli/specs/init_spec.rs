use std::fs;

use tokio::process::Command;

use anyhow::{Context, Result, bail};

use regex::Regex;

use crate::cli::{options, stack::StackMode};

/// Detect whether the env contents contain placeholders that warrant re-running init.
pub fn env_contents_have_placeholders(contents: &str) -> bool {
    let re = Regex::new(r"(changeme_)|(/change/me)").unwrap();
    contents.trim().is_empty() || re.is_match(contents)
}

/// Run the config init tool if the env file is missing, empty, or placeholders are present,
/// or when the caller explicitly requests it.
pub async fn ensure_env_initialized(
    opts: &options::StackOptions,
) -> Result<()> {
    let need_force = opts.force_init || opts.reset_db;

    let existing = fs::read_to_string(&opts.env_file).unwrap_or_default();
    let needs_init =
        existing.is_empty() || env_contents_have_placeholders(&existing);
    if !needs_init && !need_force {
        return Ok(());
    }

    // Run `ferrex-init init` via the current binary to reuse the existing CLI surface.
    let exe = std::env::current_exe()
        .context("failed to locate ferrex-init binary")?;
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
    if need_force || needs_init {
        cmd.arg("--force");
    }

    let status = cmd
        .status()
        .await
        .context("failed to run ferrex-init init")?;
    if !status.success() {
        bail!("ferrex-init init exited with {}", status);
    }
    Ok(())
}
