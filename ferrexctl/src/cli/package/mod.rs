use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cli::utils::workspace_root;
use crate::packaging_config::{
    PackagingConfig, PreflightConfig, PreflightScope,
};

const REQUIRED_TOOLS: &[&str] = [
    "flatpak",
    "flatpak-builder",
    "python3",
    "tar",
    "appstreamcli",
]
.as_slice();

const STATE_MODULES: &[&str] = [
    "rust-toolchain",
    "gstreamer",
    "gst-plugins-base",
    "gst-plugins-good",
    "gst-plugins-bad",
    "ferrex-player",
]
.as_slice();

const TAR_EXCLUDES: &[&str] = [
    "--exclude=./.flatpak-builder",
    "--exclude=./target",
    "--exclude=./target-nix",
    "--exclude=./dist-release",
    "--exclude=./dist",
    "--exclude=./build",
    "--exclude=./out",
    "--exclude=./result",
    "--exclude=./.direnv",
    "--exclude=./.git",
    "--exclude=./pgsock",
]
.as_slice();

fn check_required_tools() -> Result<()> {
    for tool in REQUIRED_TOOLS {
        which::which(tool)
            .with_context(|| format!("missing required tool: {tool}"))?;
    }
    Ok(())
}

/// Build Flatpak bundle for ferrex-player
pub async fn package_flatpak(
    output: Option<&Path>,
    version: Option<&str>,
) -> Result<()> {
    check_required_tools()?;

    let config = PackagingConfig::load()?;
    let version = match version {
        Some(v) => v.to_string(),
        None => config.resolve_version()?,
    };

    let workspace = workspace_root();
    let output_path = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let output_dir = workspace.join(&config.flatpak.output_dir);
            output_dir
                .join(format!("ferrex-player_linux_x86_64_{version}.flatpak"))
        }
    };

    let manifest_path = workspace.join(&config.flatpak.manifest_path);
    let flathub_repo = "https://flathub.org/repo/flathub.flatpakrepo";

    let tmp_dir = tempfile::tempdir()?;
    let build_dir = tmp_dir.path().join("build-dir");
    let repo_dir = tmp_dir.path().join("repo");
    let state_dir = tmp_dir.path().join("state");
    let src_dir = tmp_dir.path().join("src");
    let manifest_tmp = tmp_dir.path().join("manifest.local.yml");

    println!("Building Flatpak bundle...");
    println!("  manifest:  {}", manifest_path.display());
    println!("  app-id:    {}", config.flatpak.app_id);
    println!("  build-dir: {}", build_dir.display());
    println!("  repo-dir:  {}", repo_dir.display());
    println!("  state-dir: {}", state_dir.display());
    println!("  src-dir:   {}", src_dir.display());

    fs::create_dir_all(&build_dir)?;
    fs::create_dir_all(&repo_dir)?;

    let state_build_dir = state_dir.join("build");
    fs::create_dir_all(&state_build_dir)?;

    for module in STATE_MODULES {
        let link_path = state_build_dir.join(module);
        if !link_path.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(
                format!("{module}-placeholder"),
                &link_path,
            )?;
        }
    }

    println!("Preparing sanitized source tree...");
    fs::create_dir_all(&src_dir)?;

    let workspace_str = workspace
        .to_str()
        .context("workspace path is not valid UTF-8")?;
    let src_dir_str = src_dir
        .to_str()
        .context("src_dir path is not valid UTF-8")?;

    let mut tar_args = vec!["-C", workspace_str];
    tar_args.extend(TAR_EXCLUDES.iter().copied());
    tar_args.extend(["-cf", "-", "."]);

    let tar_cmd = Command::new("tar")
        .current_dir(&workspace)
        .args(&tar_args)
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to spawn tar archive command")?;

    let tar_stdout = tar_cmd.stdout.context("tar stdout not captured")?;

    let untar_status = Command::new("tar")
        .current_dir(&src_dir)
        .args(["-C", src_dir_str, "-xf", "-"])
        .stdin(tar_stdout)
        .status()
        .context("failed to run tar extract command")?;

    if !untar_status.success() {
        bail!("failed to create sanitized source tree");
    }

    let manifest_path_str = manifest_path
        .to_str()
        .context("manifest path is not valid UTF-8")?;
    let manifest_tmp_str = manifest_tmp
        .to_str()
        .context("manifest_tmp path is not valid UTF-8")?;

    let python_script = r#"
import sys
from pathlib import Path

src = Path(sys.argv[1]).read_text(encoding="utf-8")
out_path = Path(sys.argv[2])
src_dir = sys.argv[3]

lines = src.splitlines(True)
replaced = False
for i, line in enumerate(lines):
    if line.strip() == "path: ..":
        indent = line[: len(line) - len(line.lstrip())]
        lines[i] = f'{indent}path: "{src_dir}"\n'
        replaced = True
        break

if not replaced:
    raise SystemExit("error: could not find `path: ..` in flatpak manifest to rewrite")

out_path.write_text("".join(lines), encoding="utf-8")
"#;

    let python_status = Command::new("python3")
        .args([
            "-c",
            python_script,
            manifest_path_str,
            manifest_tmp_str,
            src_dir_str,
        ])
        .status()
        .context("failed to run python3 manifest rewrite")?;

    if !python_status.success() {
        bail!("failed to rewrite manifest path");
    }

    let remotes_output = Command::new("flatpak")
        .args(["remotes", "--user", "--columns=name"])
        .output()
        .context("failed to query flatpak remotes")?;

    let has_flathub = String::from_utf8_lossy(&remotes_output.stdout)
        .lines()
        .any(|line| line.trim() == "flathub");

    if !has_flathub {
        println!("Adding flathub remote (user)...");
        let add_remote_status = Command::new("flatpak")
            .args([
                "remote-add",
                "--user",
                "--if-not-exists",
                "flathub",
                flathub_repo,
            ])
            .status()
            .context("failed to run flatpak remote-add")?;

        if !add_remote_status.success() {
            bail!("failed to add flathub remote");
        }
    }

    println!("Running flatpak-builder...");
    let build_dir_str = build_dir
        .to_str()
        .context("build_dir path is not valid UTF-8")?;
    let repo_dir_str = repo_dir
        .to_str()
        .context("repo_dir path is not valid UTF-8")?;
    let state_dir_str = state_dir
        .to_str()
        .context("state_dir path is not valid UTF-8")?;

    let builder_status = Command::new("flatpak-builder")
        .args([
            "--user",
            "--install-deps-from=flathub",
            "--force-clean",
            &format!("--state-dir={state_dir_str}"),
            &format!("--repo={repo_dir_str}"),
            build_dir_str,
            manifest_tmp_str,
        ])
        .status()
        .context("failed to run flatpak-builder")?;

    if !builder_status.success() {
        bail!("flatpak-builder failed");
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let output_path_str = output_path
        .to_str()
        .context("output path is not valid UTF-8")?;

    let bundle_status = Command::new("flatpak")
        .args([
            "build-bundle",
            repo_dir_str,
            output_path_str,
            &config.flatpak.app_id,
            &format!("--runtime-repo={flathub_repo}"),
        ])
        .status()
        .context("failed to run flatpak build-bundle")?;

    if !bundle_status.success() {
        bail!("flatpak build-bundle failed");
    }

    println!("Wrote: {}", output_path.display());
    Ok(())
}

#[derive(Debug, Serialize)]
struct ReleaseManifest {
    schema: String,
    scope: String,
    tag: String,
    version: String,
    commit: String,
    created_utc: String,
    artifacts: Vec<ArtifactEntry>,
}

#[derive(Debug, Serialize)]
struct ArtifactEntry {
    name: String,
    sha256: String,
    size: u64,
}

fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| {
        format!("failed to open file for hashing: {}", path.display())
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn generate_manifest(
    tag: &str,
    version: &str,
    artifacts: &[PathBuf],
) -> Result<ReleaseManifest> {
    let commit_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("failed to get git commit")?;

    if !commit_output.status.success() {
        bail!("git rev-parse HEAD failed");
    }

    let commit = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let created_utc =
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut artifact_entries = Vec::new();
    for path in artifacts {
        let name = path
            .file_name()
            .context("artifact has no filename")?
            .to_str()
            .context("artifact filename not UTF-8")?
            .to_string();

        let sha256 = compute_sha256(path)?;
        let size = fs::metadata(path)
            .with_context(|| {
                format!("failed to get metadata for: {}", path.display())
            })?
            .len();

        artifact_entries.push(ArtifactEntry { name, sha256, size });
    }

    Ok(ReleaseManifest {
        schema: "ferrex.release-manifest.v1".to_string(),
        scope: "workspace".to_string(),
        tag: tag.to_string(),
        version: version.to_string(),
        commit,
        created_utc,
        artifacts: artifact_entries,
    })
}

fn run_preflight_checks(preflight: &PreflightConfig) -> Result<()> {
    if preflight.run_fmt {
        println!("== preflight: fmt ==");
        let status = Command::new("cargo")
            .args(["fmt", "--all", "--check"])
            .status()
            .context("failed to run cargo fmt")?;
        if !status.success() {
            bail!("cargo fmt check failed");
        }
    }

    if preflight.run_clippy {
        println!("== preflight: clippy ==");
        let mut args = vec!["clippy"];
        if preflight.scope == PreflightScope::Init {
            args.extend(["-p", "ferrexctl"]);
        } else {
            args.extend([
                "-p",
                "ferrex-server",
                "-p",
                "ferrex-player",
                "-p",
                "ferrexctl",
            ]);
        }
        args.extend([
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ]);
        if preflight.offline {
            args.insert(1, "--offline");
        }
        let status = Command::new("cargo")
            .args(&args)
            .status()
            .context("failed to run cargo clippy")?;
        if !status.success() {
            bail!("cargo clippy failed");
        }
    }

    if preflight.run_tests {
        println!("== preflight: tests (ferrexctl) ==");
        let mut args = vec!["test", "-p", "ferrexctl"];
        if preflight.offline {
            args.push("--offline");
        }
        let status = Command::new("cargo")
            .args(&args)
            .status()
            .context("failed to run cargo test")?;
        if !status.success() {
            bail!("cargo test failed");
        }
    }

    if preflight.run_deny && !preflight.offline {
        if which::which("cargo-deny").is_ok() {
            println!("== preflight: cargo deny ==");
            let status = Command::new("cargo")
                .args(["deny", "check"])
                .status()
                .context("failed to run cargo deny")?;
            if !status.success() {
                bail!("cargo deny check failed");
            }
        } else {
            println!(
                "== preflight: cargo deny skipped (cargo-deny not installed) =="
            );
        }
    }

    if preflight.run_audit {
        if which::which("cargo-audit").is_ok() {
            if preflight.offline {
                let cargo_home =
                    std::env::var("CARGO_HOME").unwrap_or_else(|_| {
                        format!(
                            "{}/.cargo",
                            std::env::var("HOME").unwrap_or_default()
                        )
                    });
                let advisory_db =
                    std::path::Path::new(&cargo_home).join("advisory-db");
                if advisory_db.exists() {
                    println!("== preflight: cargo audit (offline) ==");
                    let status = Command::new("cargo")
                        .args(["audit", "--no-fetch"])
                        .status()
                        .context("failed to run cargo audit")?;
                    if !status.success() {
                        bail!("cargo audit failed");
                    }
                } else {
                    println!(
                        "== preflight: cargo audit (offline) skipped: advisory DB not present =="
                    );
                }
            } else {
                println!("== preflight: cargo audit ==");
                let status = Command::new("cargo")
                    .args(["audit"])
                    .status()
                    .context("failed to run cargo audit")?;
                if !status.success() {
                    bail!("cargo audit failed");
                }
            }
        } else {
            println!(
                "== preflight: cargo audit skipped (cargo-audit not installed) =="
            );
        }
    }

    Ok(())
}

/// Run preflight checks for packaging
pub async fn package_preflight(
    scope: &str,
    offline: bool,
    dry_run: bool,
    skip: &[String],
) -> Result<()> {
    let scope_enum = match scope {
        "init" => PreflightScope::Init,
        _ => PreflightScope::Workspace,
    };

    let mut config = PreflightConfig {
        offline,
        scope: scope_enum,
        ..PreflightConfig::default()
    };

    for check in skip {
        match check.as_str() {
            "fmt" => config.run_fmt = false,
            "clippy" => config.run_clippy = false,
            "test" => config.run_tests = false,
            "deny" => config.run_deny = false,
            "audit" => config.run_audit = false,
            _ => {}
        }
    }

    if dry_run {
        println!("Preflight checks (dry-run):");
        println!("  Scope: {scope}");
        println!("  Offline: {offline}");
        println!();
        println!("Checks that would run:");
        if config.run_fmt {
            println!("  - cargo fmt --all --check");
        }
        if config.run_clippy {
            if config.scope == PreflightScope::Init {
                println!(
                    "  - cargo clippy -p ferrexctl --all-targets --all-features -- -D warnings"
                );
            } else {
                println!(
                    "  - cargo clippy -p ferrex-server -p ferrex-player -p ferrexctl --all-targets --all-features -- -D warnings"
                );
            }
        }
        if config.run_tests {
            println!("  - cargo test -p ferrexctl");
        }
        if config.run_deny && !config.offline {
            println!("  - cargo deny check");
        }
        if config.run_audit {
            if config.offline {
                println!("  - cargo audit --no-fetch (if advisory DB present)");
            } else {
                println!("  - cargo audit");
            }
        }
        let skipped: Vec<&str> = skip.iter().map(|s| s.as_str()).collect();
        if !skipped.is_empty() {
            println!();
            println!("Skipped checks: {}", skipped.join(", "));
        }
        return Ok(());
    }

    run_preflight_checks(&config)?;

    println!("== preflight: ok ==");
    Ok(())
}

/// Generate release artifacts (Flatpak + manifest + checksums)
pub async fn package_release(
    version: Option<&str>,
    output_dir: Option<&Path>,
    skip_preflight: bool,
    dry_run: bool,
) -> Result<()> {
    let config = PackagingConfig::load()?;

    let version = match version {
        Some(v) => v.to_string(),
        None => config.resolve_version()?,
    };
    let tag = format!("v{version}");

    let workspace = workspace_root();
    let output_dir = match output_dir {
        Some(d) => d.to_path_buf(),
        None => workspace.join(&config.release.output_dir).join(&tag),
    };

    println!("Release configuration:");
    println!("  Tag:     {tag}");
    println!("  Version: {version}");
    println!("  Output:  {}", output_dir.display());
    println!();

    if !skip_preflight {
        run_preflight_checks(&config.preflight)?;
        println!();
    } else {
        println!("Skipping preflight checks (--skip-preflight)");
        println!();
    }

    if dry_run {
        println!("[dry-run] would create: {}", output_dir.display());
    } else {
        fs::create_dir_all(&output_dir).with_context(|| {
            format!(
                "failed to create output directory: {}",
                output_dir.display()
            )
        })?;
    }

    let flatpak_filename =
        format!("ferrex-player_linux_x86_64_{version}.flatpak");
    let flatpak_path = output_dir.join(&flatpak_filename);

    if dry_run {
        println!("[dry-run] would build: {}", flatpak_path.display());
    } else {
        println!("Building Flatpak bundle...");
        package_flatpak(Some(&flatpak_path), Some(&version)).await?;
    }

    let manifest_path = output_dir.join("manifest.json");
    if dry_run {
        println!("[dry-run] would write: {}", manifest_path.display());
    } else {
        let manifest = generate_manifest(
            &tag,
            &version,
            std::slice::from_ref(&flatpak_path),
        )?;
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .context("failed to serialize manifest")?;
        fs::write(&manifest_path, manifest_json).with_context(|| {
            format!("failed to write manifest: {}", manifest_path.display())
        })?;
        println!("Wrote: {}", manifest_path.display());
    }

    let sha256sums_path = output_dir.join("SHA256SUMS");
    if dry_run {
        println!("[dry-run] would write: {}", sha256sums_path.display());
    } else {
        let mut entries = Vec::new();

        for entry in fs::read_dir(&output_dir).with_context(|| {
            format!("failed to read output directory: {}", output_dir.display())
        })? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file()
                && path.file_name() != Some(OsStr::new("SHA256SUMS"))
            {
                let filename = path
                    .file_name()
                    .context("file has no name")?
                    .to_str()
                    .context("filename not UTF-8")?
                    .to_string();
                let sha256 = compute_sha256(&path)?;
                entries.push((filename, sha256));
            }
        }

        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut content = String::new();
        for (filename, sha256) in &entries {
            content.push_str(&format!("{sha256}  {filename}\n"));
        }

        fs::write(&sha256sums_path, &content).with_context(|| {
            format!("failed to write SHA256SUMS: {}", sha256sums_path.display())
        })?;
        println!("Wrote: {}", sha256sums_path.display());
    }

    println!();
    println!("Release artifacts generated:");
    println!("  Tag: {tag}");
    println!("  Version: {version}");
    println!("  Output: {}", output_dir.display());
    println!();
    println!("Artifacts:");
    println!("  - {flatpak_filename}");
    println!("  - manifest.json");
    println!("  - SHA256SUMS");

    Ok(())
}

fn read_ferrexctl_version() -> Result<String> {
    let workspace = workspace_root();
    let cargo_toml_path = workspace.join("ferrexctl").join("Cargo.toml");

    let content = fs::read_to_string(&cargo_toml_path).with_context(|| {
        format!(
            "failed to read ferrexctl Cargo.toml at {}",
            cargo_toml_path.display()
        )
    })?;

    match parse_ferrexctl_version(&content) {
        Some(version) => Ok(version),
        None => {
            let workspace_cargo_path = workspace.join("Cargo.toml");
            let workspace_content = fs::read_to_string(&workspace_cargo_path)
                .with_context(|| {
                format!(
                    "failed to read workspace Cargo.toml at {}",
                    workspace_cargo_path.display()
                )
            })?;
            parse_workspace_version(&workspace_content).with_context(
                || "failed to parse version from workspace Cargo.toml",
            )
        }
    }
}

fn parse_ferrexctl_version(cargo_toml: &str) -> Option<String> {
    let mut in_package = false;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();

        if trimmed == "[package]" {
            in_package = true;
            continue;
        }

        if in_package {
            if trimmed.starts_with('[') && trimmed != "[package]" {
                in_package = false;
                continue;
            }

            if trimmed.starts_with("version") {
                if let Some(eq_pos) = trimmed.find('=') {
                    let value = trimmed[eq_pos + 1..].trim();
                    if value.starts_with('"') {
                        let version = value.trim_matches('"').trim();
                        return Some(version.to_string());
                    }
                }
            }
        }
    }

    None
}

fn parse_workspace_version(cargo_toml: &str) -> Option<String> {
    let mut in_workspace_package = false;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();

        if trimmed == "[workspace.package]" {
            in_workspace_package = true;
            continue;
        }

        if in_workspace_package {
            if trimmed.starts_with('[') {
                in_workspace_package = false;
                continue;
            }

            if trimmed.starts_with("version") {
                if let Some(eq_pos) = trimmed.find('=') {
                    let value = trimmed[eq_pos + 1..].trim();
                    let version = value.trim_matches('"').trim();
                    return Some(version.to_string());
                }
            }
        }
    }

    None
}

fn get_git_repo_slug() -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .context("failed to run git remote get-url origin")?;

    if !output.status.success() {
        bail!("no git remote named 'origin' found");
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let https_re =
        regex::Regex::new(r"^https://github\.com/([^/]+)/([^/]+)(\.git)?$")
            .map_err(|e| anyhow::anyhow!("invalid regex: {}", e))?;
    if let Some(caps) = https_re.captures(&url) {
        let owner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let repo = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        return Ok(format!("{}/{}", owner, repo.trim_end_matches(".git")));
    }

    let ssh_re =
        regex::Regex::new(r"^git@github\.com:([^/]+)/([^/]+)(\.git)?$")
            .map_err(|e| anyhow::anyhow!("invalid regex: {}", e))?;
    if let Some(caps) = ssh_re.captures(&url) {
        let owner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let repo = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        return Ok(format!("{}/{}", owner, repo.trim_end_matches(".git")));
    }

    bail!("unsupported origin url (expected github): {}", url)
}

fn ensure_clean_tree() -> Result<()> {
    let diff_output = Command::new("git")
        .args(["diff", "--quiet"])
        .output()
        .context("failed to run git diff")?;

    let cached_output = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .output()
        .context("failed to run git diff --cached")?;

    if !diff_output.status.success() || !cached_output.status.success() {
        bail!(
            "working tree not clean; commit or stash changes before releasing"
        );
    }

    Ok(())
}

fn ensure_gh() -> Result<()> {
    which::which("gh").context("missing required command: gh")?;

    let status = Command::new("gh")
        .args(["auth", "status"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run gh auth status")?;

    if !status.success() {
        bail!("gh not authenticated; run: gh auth login");
    }

    Ok(())
}

fn ensure_docker() -> Result<()> {
    which::which("docker").context("missing required command: docker")?;

    let status = Command::new("docker")
        .args(["info"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run docker info")?;

    if !status.success() {
        bail!("docker daemon not available");
    }

    Ok(())
}

fn ghcr_login() -> Result<()> {
    let user_output = Command::new("gh")
        .args(["api", "user", "-q", ".login"])
        .output()
        .context("failed to get gh user")?;

    if !user_output.status.success() {
        bail!("unable to get gh user");
    }
    let user = String::from_utf8_lossy(&user_output.stdout)
        .trim()
        .to_string();

    let token_output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .context("failed to get gh auth token")?;

    if !token_output.status.success() {
        bail!("unable to get gh auth token");
    }
    let token = String::from_utf8_lossy(&token_output.stdout)
        .trim()
        .to_string();

    if user.is_empty() || token.is_empty() {
        bail!("unable to get gh auth token/user");
    }

    let mut child = Command::new("docker")
        .args(["login", "ghcr.io", "-u", &user, "--password-stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn docker login")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(token.as_bytes())
            .context("failed to write token to docker login")?;
    }

    let status = child.wait().context("failed to wait for docker login")?;

    if !status.success() {
        bail!("docker login to ghcr.io failed");
    }

    Ok(())
}

fn build_ferrexctl_binary(version: &str, output_path: &Path) -> Result<()> {
    let workspace = workspace_root();
    let script_path = workspace.join("scripts/build/ferrexctl-binary.sh");

    let status = Command::new("bash")
        .arg(&script_path)
        .args([version, output_path.to_str().unwrap_or("")])
        .current_dir(&workspace)
        .status()
        .with_context(|| {
            format!(
                "failed to run ferrexctl-binary.sh script at {}",
                script_path.display()
            )
        })?;

    if !status.success() {
        bail!("ferrexctl-binary.sh build failed");
    }

    Ok(())
}

fn build_and_push_docker_image(
    repo: &str,
    version: &str,
    tag_latest: bool,
    dry_run: bool,
) -> Result<()> {
    let image = format!(
        "ghcr.io/{}/ferrexctl:{}",
        repo.split('/').next().unwrap_or(repo),
        version
    );

    if dry_run {
        println!("[dry-run] would build/push image: {}", image);
        if tag_latest {
            let latest_image = format!(
                "ghcr.io/{}/ferrexctl:latest",
                repo.split('/').next().unwrap_or(repo)
            );
            println!("[dry-run] would also tag as: {}", latest_image);
        }
        return Ok(());
    }

    ensure_docker()?;
    ghcr_login()?;

    println!("Building/pushing image: {}", image);

    let workspace = workspace_root();
    let mut args = vec![
        "buildx".to_string(),
        "build".to_string(),
        "--platform".to_string(),
        "linux/amd64".to_string(),
        "-f".to_string(),
        "docker/Dockerfile.init".to_string(),
        "-t".to_string(),
        image.clone(),
        "--push".to_string(),
        ".".to_string(),
    ];

    if tag_latest {
        let latest_image = format!(
            "ghcr.io/{}/ferrexctl:latest",
            repo.split('/').next().unwrap_or(repo)
        );
        args.push("-t".to_string());
        args.push(latest_image);
    }

    let status = Command::new("docker")
        .args(&args)
        .current_dir(&workspace)
        .status()
        .context("failed to run docker buildx build")?;

    if !status.success() {
        bail!("docker buildx build failed");
    }

    println!("Successfully pushed: {}", image);

    Ok(())
}

fn generate_sha256sums(dir: &Path) -> Result<()> {
    let entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| {
            format!("failed to read directory: {}", dir.display())
        })?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            path.is_file() && path.file_name() != Some(OsStr::new("SHA256SUMS"))
        })
        .map(|e| e.path())
        .collect();

    let mut checksums = Vec::new();
    for path in entries {
        let filename = path
            .file_name()
            .context("file has no name")?
            .to_str()
            .context("filename not UTF-8")?
            .to_string();
        let sha256 = compute_sha256(&path)?;
        checksums.push((filename, sha256));
    }

    checksums.sort_by(|a, b| a.0.cmp(&b.0));

    let mut content = String::new();
    for (filename, sha256) in &checksums {
        content.push_str(&format!("{}  {}\n", sha256, filename));
    }

    let sha256sums_path = dir.join("SHA256SUMS");
    fs::write(&sha256sums_path, content).with_context(|| {
        format!("failed to write SHA256SUMS: {}", sha256sums_path.display())
    })?;

    println!("Wrote: {}", sha256sums_path.display());

    Ok(())
}

fn create_github_release(
    repo: &str,
    tag: &str,
    assets: &[PathBuf],
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        println!(
            "[dry-run] would create draft GitHub Release: {} (repo={})",
            tag, repo
        );
        println!("[dry-run] with assets:");
        for asset in assets {
            println!("  - {}", asset.display());
        }
        println!("[dry-run] would rely on CI (tag push) to verify/publish");
        return Ok(());
    }

    ensure_gh()?;

    // Check if release already exists
    let check_output = Command::new("gh")
        .args(["release", "view", tag])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("failed to check if release exists")?;

    if check_output.status.success() {
        bail!(
            "release/tag already exists: {} (refusing to overwrite)",
            tag
        );
    }

    println!("Creating draft GitHub Release with tag {} in {}", tag, repo);

    let mut args = vec![
        "release".to_string(),
        "create".to_string(),
        tag.to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--draft".to_string(),
        "--title".to_string(),
        format!("ferrexctl {}", tag),
        "--notes".to_string(),
        "".to_string(),
        "--target".to_string(),
        "HEAD".to_string(),
    ];

    for asset in assets {
        args.push(asset.to_str().unwrap_or("").to_string());
    }

    let status = Command::new("gh")
        .args(&args)
        .status()
        .context("failed to run gh release create")?;

    if !status.success() {
        bail!("gh release create failed");
    }

    println!("Done. Draft release created for tag: {}", tag);

    Ok(())
}

/// Initialize a draft GitHub Release for ferrexctl
pub async fn package_release_init(
    version: &str,
    no_image: bool,
    tag_latest: bool,
    dry_run: bool,
    skip_preflight: bool,
    offline_preflight: bool,
    skip_build: bool,
) -> Result<()> {
    let cargo_version = read_ferrexctl_version()?;
    if cargo_version != version {
        bail!(
            "ferrexctl version mismatch: ferrexctl/Cargo.toml={} requested={}",
            cargo_version,
            version
        );
    }

    let tag = format!("ferrexctl-v{}", version);

    println!("Release init configuration:");
    println!("  Tag:     {}", tag);
    println!("  Version: {}", version);
    println!("  Dry run: {}", dry_run);
    println!();

    if !dry_run {
        ensure_clean_tree()?;
    }

    if !skip_preflight {
        let scope = if offline_preflight { "init" } else { "init" };
        let skip_checks: Vec<String> = vec![];
        package_preflight(scope, offline_preflight, dry_run, &skip_checks)
            .await?;
        println!();
    } else {
        println!("Skipping preflight checks (--skip-preflight)");
        println!();
    }

    if !dry_run {
        ensure_gh()?;
    }

    let repo = get_git_repo_slug()?;
    let workspace = workspace_root();
    let dist_dir = workspace.join("dist-release").join(&tag);

    if dry_run {
        println!("[dry-run] would create: {}", dist_dir.display());
    } else {
        fs::create_dir_all(&dist_dir).with_context(|| {
            format!("failed to create dist directory: {}", dist_dir.display())
        })?;
    }

    let mut assets: Vec<PathBuf> = vec![];

    let tarball_name = format!("ferrexctl_linux_x86_64_{}.tar.gz", version);
    let tarball_path = dist_dir.join(&tarball_name);

    println!(
        "Building ferrexctl binary tarball → {}",
        tarball_path.display()
    );

    if skip_build {
        println!(
            "[skip-build] would build tarball → {}",
            tarball_path.display()
        );
    } else if dry_run {
        println!("[dry-run] would build tarball → {}", tarball_path.display());
        assets.push(tarball_path.clone());
    } else {
        build_ferrexctl_binary(version, &tarball_path)?;
        assets.push(tarball_path.clone());
    }

    if !no_image {
        build_and_push_docker_image(&repo, version, tag_latest, dry_run)?;
    } else {
        println!("Skipping Docker image build (--no-image)");
    }

    let manifest_path = dist_dir.join("manifest.json");
    if dry_run {
        println!("[dry-run] would write: {}", manifest_path.display());
        assets.push(manifest_path.clone());
    } else {
        let manifest_artifacts: Vec<PathBuf> =
            assets.iter().filter(|p| p.exists()).cloned().collect();
        let manifest = generate_manifest(&tag, version, &manifest_artifacts)?;
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .context("failed to serialize manifest")?;
        fs::write(&manifest_path, manifest_json).with_context(|| {
            format!("failed to write manifest: {}", manifest_path.display())
        })?;
        println!("Wrote: {}", manifest_path.display());
        assets.push(manifest_path.clone());
    }

    if dry_run {
        let sha256sums_path = dist_dir.join("SHA256SUMS");
        println!("[dry-run] would write: {}", sha256sums_path.display());
        assets.push(sha256sums_path);
    } else {
        generate_sha256sums(&dist_dir)?;
        let sha256sums_path = dist_dir.join("SHA256SUMS");
        assets.push(sha256sums_path);
    }

    println!();
    create_github_release(&repo, &tag, &assets, dry_run)?;

    Ok(())
}
