use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cli::utils::workspace_root;
use crate::packaging_config::{PackagingConfig, PreflightConfig};

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
        let mut args = vec![
            "clippy",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ];
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
        println!("== preflight: tests ==");
        let mut args = vec!["test"];
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

    if preflight.run_audit {
        if which::which("cargo-audit").is_ok() {
            println!("== preflight: audit ==");
            let status = Command::new("cargo")
                .args(["audit"])
                .status()
                .context("failed to run cargo audit")?;
            if !status.success() {
                bail!("cargo audit failed");
            }
        } else {
            println!(
                "== preflight: audit skipped (cargo-audit not installed) =="
            );
        }
    }

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
