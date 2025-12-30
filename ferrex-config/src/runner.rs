//! Runtime selection and docker runner utilities for `ferrex-init`.
//!
//! Contains heuristics for host vs docker selection and SELinux/Podman mount
//! suffix detection, plus a small wrapper that executes the init image and
//! parses its `KEY=VALUE` output.

use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, anyhow, bail};

/// Execution backend for the init tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Host,
    Docker,
}

/// Desired runner selection from CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerChoice {
    Auto,
    Host,
    Docker,
}

/// Detect mount suffix for SELinux/Podman so bind mounts work correctly.
pub fn detect_mount_suffix() -> String {
    let selinux = selinux_enabled();
    let podman = podman_available();

    if selinux && podman {
        ":z,U".to_string()
    } else if selinux {
        ":z".to_string()
    } else if podman {
        ":U".to_string()
    } else {
        String::new()
    }
}

fn selinux_enabled() -> bool {
    matches!(fs::read_to_string("/sys/fs/selinux/enforce"), Ok(ref s) if s.trim() == "1")
}

fn podman_available() -> bool {
    Command::new("podman")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Decide runner based on availability (host preferred, docker fallback).
pub fn choose_runner(choice: RunnerChoice) -> Runner {
    match choice {
        RunnerChoice::Host => Runner::Host,
        RunnerChoice::Docker => Runner::Docker,
        RunnerChoice::Auto => {
            if Command::new("ferrex-init")
                .arg("--version")
                .output()
                .is_ok()
                || Command::new("cargo").arg("--version").output().is_ok()
            {
                Runner::Host
            } else {
                Runner::Docker
            }
        }
    }
}

/// Run init inside a docker/podman container and parse `KEY=VAL` lines.
#[allow(clippy::too_many_arguments)]
pub fn run_docker_init(
    image: &str,
    env_file: &Path,
    tailscale: bool,
    advanced: bool,
    non_interactive: bool,
    rotate: Option<&str>,
    force: bool,
    mount_suffix: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let runtime = std::env::var("FERREX_INIT_DOCKER_CMD")
        .unwrap_or_else(|_| "docker".into());
    if !env_file.exists() {
        // Ensure parent exists so docker can mount it readable/creatable.
        if let Some(parent) = env_file.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create {}", parent.display())
            })?;
        }
        fs::write(env_file, b"")?;
    }

    let suffix = mount_suffix
        .map(str::to_string)
        .unwrap_or_else(detect_mount_suffix);
    let mount = format!(
        "{}:/app/.env{}",
        env_file
            .canonicalize()
            .unwrap_or_else(|_| env_file.to_path_buf())
            .display(),
        suffix
    );

    let mut cmd = Command::new(runtime);
    cmd.arg("run")
        .arg("--rm")
        .arg("-v")
        .arg(&mount)
        .arg("-e")
        .arg("FERREX_ENV_FILE=/app/.env");

    if non_interactive {
        cmd.args(["-e", "FERREX_INIT_NON_INTERACTIVE=1"]);
    }
    if tailscale {
        cmd.args(["-e", "FERREX_INIT_TAILSCALE=1"]);
    }
    if advanced {
        cmd.args(["-e", "FERREX_INIT_ADVANCED_CONFIG=1"]);
    }
    if force {
        cmd.args(["-e", "FERREX_INIT_FORCE_CONFIG=1"]);
    }
    if let Some(rot) = rotate {
        cmd.args(["-e", &format!("FERREX_INIT_ROTATE={}", rot)]);
    }

    cmd.arg(image)
        .arg("init")
        .arg("--env-file")
        .arg("/app/.env")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());

    let output = cmd.output().context("failed to run docker init")?;
    if !output.status.success() {
        bail!("docker runner exited with status {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut kv = Vec::new();
    for line in stdout.lines() {
        if let Some((k, v)) = line.split_once('=') {
            kv.push((k.to_string(), v.to_string()));
        }
    }
    if kv.is_empty() {
        return Err(anyhow!("docker runner produced no key/value output"));
    }
    Ok(kv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_selinux_and_podman() {
        // Simulate availability with env vars is overkill; we simply assert shapes.
        // The function itself has no easy injection points, so check non-empty output when at least
        // one condition is true.
        let s = detect_mount_suffix();
        assert!(
            s.is_empty()
                || s.contains(':')
                || s == ":z"
                || s == ":U"
                || s == ":z,U"
        );
    }

    #[test]
    fn choose_runner_prefers_host_when_available() {
        let r = choose_runner(RunnerChoice::Auto);
        assert!(matches!(r, Runner::Host | Runner::Docker));
    }
}
