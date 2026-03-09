use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_preflight_dry_run_lists_expected_checks() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args(["package", "preflight", "--scope=init", "--dry-run"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Preflight checks (dry-run)"))
        .stdout(predicates::str::contains("cargo fmt --all --check"))
        .stdout(predicates::str::contains("cargo clippy"))
        .stdout(predicates::str::contains("cargo test -p ferrexctl"));
}

#[test]
fn test_preflight_dry_run_workspace_scope() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args(["package", "preflight", "--scope=workspace", "--dry-run"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Scope: workspace"))
        .stdout(predicates::str::contains("ferrex-server"))
        .stdout(predicates::str::contains("ferrex-player"));
}

#[test]
fn test_preflight_dry_run_with_offline() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "package",
        "preflight",
        "--scope=init",
        "--dry-run",
        "--offline",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Offline: true"));
}

#[test]
#[ignore = "requires workspace root detection fix"]
fn test_release_init_dry_run_shows_expected_steps() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "package",
        "release-init",
        "0.1.0-alpha",
        "--dry-run",
        "--skip-preflight",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Release init configuration"))
        .stdout(predicates::str::contains("Tag:     ferrexctl-v0.1.0-alpha"))
        .stdout(predicates::str::contains("Version: 0.1.0-alpha"))
        .stdout(predicates::str::contains("Dry run: true"))
        .stdout(predicates::str::contains("[dry-run] would build tarball"))
        .stdout(predicates::str::contains("[dry-run] would write:"));
}

#[test]
#[ignore = "requires workspace root detection fix"]
fn test_release_init_dry_run_with_no_image() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "package",
        "release-init",
        "0.1.0-alpha",
        "--dry-run",
        "--skip-preflight",
        "--no-image",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Skipping Docker image build"));
}

#[test]
#[ignore = "requires built Windows binary"]
fn test_windows_dry_run_shows_cross_compile_config() {
    let temp_dir = TempDir::new().unwrap();
    let gst_root = temp_dir.path();

    fs::create_dir_all(gst_root.join("bin")).unwrap();
    fs::create_dir_all(gst_root.join("lib").join("gstreamer-1.0")).unwrap();

    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "package",
        "windows",
        "--dry-run",
        "--gst-root",
        &gst_root.to_string_lossy(),
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Windows packaging configuration"))
        .stdout(predicates::str::contains("Target:"))
        .stdout(predicates::str::contains("Profile:"))
        .stdout(predicates::str::contains("Flavor:"))
        .stdout(predicates::str::contains("GStreamer root:"))
        .stdout(predicates::str::contains("Mode:    dry-run"))
        .stdout(predicates::str::contains(
            "[dry-run] would stage distribution",
        ))
        .stdout(predicates::str::contains("[dry-run] would copy:"))
        .stdout(predicates::str::contains("[dry-run] would create zip:"));
}

#[test]
#[ignore = "requires built Windows binary"]
fn test_windows_dry_run_with_target() {
    let temp_dir = TempDir::new().unwrap();
    let gst_root = temp_dir.path();

    fs::create_dir_all(gst_root.join("bin")).unwrap();
    fs::create_dir_all(gst_root.join("lib").join("gstreamer-1.0")).unwrap();

    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "package",
        "windows",
        "--dry-run",
        "--target",
        "x86_64-pc-windows-gnu",
        "--gst-root",
        &gst_root.to_string_lossy(),
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Target:  x86_64-pc-windows-gnu"))
        .stdout(predicates::str::contains("Flavor:  gnu"));
}

#[test]
fn test_flatpak_dry_run_shows_manifest_preparation() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args(["package", "flatpak", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("flatpak"))
        .stdout(predicates::str::contains("--output"))
        .stdout(predicates::str::contains("--version"));
}

#[test]
fn test_postgres_preset_applies_correctly_in_env() {
    let temp_dir = TempDir::new().unwrap();
    let env_file = temp_dir.path().join(".env");

    fs::write(&env_file, "MEDIA_ROOT=/tmp/media\nCACHE_DIR=/tmp/cache\n")
        .unwrap();

    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "init",
        "--env-file",
        &env_file.to_string_lossy(),
        "--non-interactive",
        "--postgres-preset",
        "small",
    ]);
    cmd.assert().success();

    let env_content = fs::read_to_string(&env_file).unwrap();
    assert!(
        env_content.contains("FERREX_POSTGRES_PRESET=small"),
        "Expected FERREX_POSTGRES_PRESET=small in env file"
    );
    assert!(
        env_content.contains("FERREX_POSTGRES_SHARED_BUFFERS=512MB"),
        "Expected shared_buffers preset value"
    );
    assert!(
        env_content.contains("FERREX_POSTGRES_EFFECTIVE_CACHE_SIZE=2GB"),
        "Expected effective_cache_size preset value"
    );
    assert!(
        env_content.contains("FERREX_POSTGRES_WORK_MEM=16MB"),
        "Expected work_mem preset value"
    );
    assert!(
        env_content.contains("FERREX_POSTGRES_MAX_CONNECTIONS=50"),
        "Expected max_connections preset value"
    );
    assert!(
        env_content.contains("FERREX_POSTGRES_SHM_SIZE=2g"),
        "Expected shm_size preset value"
    );
    assert!(
        env_content.contains("FERREX_POSTGRES_MAINTENANCE_WORK_MEM=256MB"),
        "Expected maintenance_work_mem preset value"
    );
}

#[test]
fn test_postgres_preset_medium() {
    let temp_dir = TempDir::new().unwrap();
    let env_file = temp_dir.path().join(".env");

    fs::write(&env_file, "MEDIA_ROOT=/tmp/media\nCACHE_DIR=/tmp/cache\n")
        .unwrap();

    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "init",
        "--env-file",
        &env_file.to_string_lossy(),
        "--non-interactive",
        "--postgres-preset",
        "medium",
    ]);
    cmd.assert().success();

    let env_content = fs::read_to_string(&env_file).unwrap();
    assert!(env_content.contains("FERREX_POSTGRES_PRESET=medium"));
    assert!(env_content.contains("FERREX_POSTGRES_SHARED_BUFFERS=4GB"));
    assert!(env_content.contains("FERREX_POSTGRES_EFFECTIVE_CACHE_SIZE=12GB"));
    assert!(env_content.contains("FERREX_POSTGRES_WORK_MEM=64MB"));
    assert!(env_content.contains("FERREX_POSTGRES_MAX_CONNECTIONS=100"));
    assert!(env_content.contains("FERREX_POSTGRES_SHM_SIZE=8g"));
    assert!(env_content.contains("FERREX_POSTGRES_MAINTENANCE_WORK_MEM=1GB"));
}

#[test]
fn test_postgres_preset_large() {
    let temp_dir = TempDir::new().unwrap();
    let env_file = temp_dir.path().join(".env");

    fs::write(&env_file, "MEDIA_ROOT=/tmp/media\nCACHE_DIR=/tmp/cache\n")
        .unwrap();

    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "init",
        "--env-file",
        &env_file.to_string_lossy(),
        "--non-interactive",
        "--postgres-preset",
        "large",
    ]);
    cmd.assert().success();

    let env_content = fs::read_to_string(&env_file).unwrap();
    assert!(env_content.contains("FERREX_POSTGRES_PRESET=large"));
    assert!(env_content.contains("FERREX_POSTGRES_SHARED_BUFFERS=16GB"));
    assert!(env_content.contains("FERREX_POSTGRES_EFFECTIVE_CACHE_SIZE=48GB"));
    assert!(env_content.contains("FERREX_POSTGRES_WORK_MEM=256MB"));
    assert!(env_content.contains("FERREX_POSTGRES_MAX_CONNECTIONS=200"));
    assert!(env_content.contains("FERREX_POSTGRES_SHM_SIZE=32g"));
    assert!(env_content.contains("FERREX_POSTGRES_MAINTENANCE_WORK_MEM=2GB"));
}

#[test]
fn test_postgres_preset_custom() {
    let temp_dir = TempDir::new().unwrap();
    let env_file = temp_dir.path().join(".env");

    fs::write(&env_file, "MEDIA_ROOT=/tmp/media\nCACHE_DIR=/tmp/cache\n")
        .unwrap();

    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "init",
        "--env-file",
        &env_file.to_string_lossy(),
        "--non-interactive",
        "--postgres-preset",
        "custom",
    ]);
    cmd.assert().success();

    let env_content = fs::read_to_string(&env_file).unwrap();
    assert!(env_content.contains("FERREX_POSTGRES_PRESET=custom"));
    assert!(!env_content.contains("FERREX_POSTGRES_SHARED_BUFFERS=512MB"));
}

#[test]
fn test_postgres_override_takes_precedence_over_preset() {
    let temp_dir = TempDir::new().unwrap();
    let env_file = temp_dir.path().join(".env");

    fs::write(
        &env_file,
        "MEDIA_ROOT=/tmp/media\nCACHE_DIR=/tmp/cache\nFERREX_POSTGRES_SHARED_BUFFERS=8GB\n",
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args([
        "init",
        "--env-file",
        &env_file.to_string_lossy(),
        "--non-interactive",
        "--postgres-preset",
        "small",
    ]);
    cmd.assert().success();

    let env_content = fs::read_to_string(&env_file).unwrap();
    assert!(env_content.contains("FERREX_POSTGRES_PRESET=small"));
    assert!(
        env_content.contains("FERREX_POSTGRES_SHARED_BUFFERS=8GB"),
        "Existing FERREX_POSTGRES_SHARED_BUFFERS should take precedence over preset"
    );
    assert!(
        env_content.contains("FERREX_POSTGRES_WORK_MEM=16MB"),
        "Other preset values should still be applied"
    );
}

#[test]
fn test_package_subcommand_help() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args(["package", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("preflight"))
        .stdout(predicates::str::contains("release-init"))
        .stdout(predicates::str::contains("windows"))
        .stdout(predicates::str::contains("flatpak"));
}

#[test]
fn test_preflight_help() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args(["package", "preflight", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--scope"))
        .stdout(predicates::str::contains("--dry-run"))
        .stdout(predicates::str::contains("--offline"))
        .stdout(predicates::str::contains("--skip"));
}

#[test]
fn test_release_init_help() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args(["package", "release-init", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--dry-run"))
        .stdout(predicates::str::contains("--skip-preflight"))
        .stdout(predicates::str::contains("--no-image"))
        .stdout(predicates::str::contains("--tag-latest"));
}

#[test]
fn test_windows_help() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    cmd.args(["package", "windows", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--target"))
        .stdout(predicates::str::contains("--profile"))
        .stdout(predicates::str::contains("--gst-root"))
        .stdout(predicates::str::contains("--out"))
        .stdout(predicates::str::contains("--dry-run"));
}
