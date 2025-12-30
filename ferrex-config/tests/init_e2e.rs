//! Manual, ignored grey-box tests that exercise `ferrex-init` end-to-end.
//! They spawn the real binary, drive the TUI via a scripted adapter, and run
//! docker compose stacks against a throwaway project root. These are meant for
//! manual validation (run with `just init-e2e`) and are skipped in CI.

use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use once_cell::sync::Lazy;
use regex::Regex;
use tempfile::TempDir;

static SERIAL: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Copy)]
struct Ports {
    db: u16,
    redis: u16,
    app: u16,
}

struct Harness {
    root: TempDir,
    compose_root: PathBuf,
    env_file: PathBuf,
    ports: Ports,
}

impl Harness {
    fn new() -> anyhow::Result<Self> {
        let root = tempfile::tempdir()?;
        let compose_root = root.path().join("compose");
        fs::create_dir_all(&compose_root)?;

        let ports = Ports {
            db: find_free_port()?,
            redis: find_free_port()?,
            app: find_free_port()?,
        };

        copy_compose_files(&compose_root, ports)?;

        let env_file = root.path().join(".env");
        let cache_dir = root.path().join("cache");
        let image_cache_dir = cache_dir.join("images");
        let media_dir = root.path().join("media");
        fs::create_dir_all(&cache_dir)?;
        fs::create_dir_all(&image_cache_dir)?;
        fs::create_dir_all(&media_dir)?;

        // Seed an env with the chosen host repository_ports so init picks them up.
        fs::write(
            &env_file,
            format!(
                "\
SERVER_PORT={app}
FERREX_SERVER_URL=http://localhost:{app}
DATABASE_PORT={db}
DATABASE_HOST=localhost
                REDIS_URL=redis://127.0.0.1:{redis}
                MEDIA_ROOT={}
                CACHE_DIR={}
                IMAGE_CACHE_DIR={}
                ",
                media_dir.display(),
                cache_dir.display(),
                image_cache_dir.display(),
                app = ports.app,
                db = ports.db,
                redis = ports.redis
            ),
        )?;

        Ok(Self {
            root,
            compose_root,
            env_file,
            ports,
        })
    }

    fn bin(&self) -> Command {
        let bin = find_ferrex_init_binary().expect(
            "binary path not set (run via cargo test or set FERREX_INIT_E2E_BIN=/abs/path/to/ferrex-init)",
        );
        if !bin.exists() {
            panic!(
                "ferrex-init binary not found at: {} (current dir: {:?})",
                bin.display(),
                std::env::current_dir()
            );
        }
        eprintln!("using ferrex-init binary: {}", bin.display());
        let mut cmd = Command::new(bin);
        cmd.env("FERREX_COMPOSE_ROOT", &self.compose_root)
            .env("FERREX_INIT_AUTO_CONFIRM", "1");
        cmd
    }

    fn run_init_tui(&self, script_lines: &[&str]) -> anyhow::Result<()> {
        eprintln!("  → Running TUI init with scripted input...");
        let script_path = self.root.path().join("tui_script.txt");
        fs::write(
            &script_path,
            script_lines
                .iter()
                .map(|l| format!("{l}\n"))
                .collect::<String>(),
        )?;
        let trace_path = self.root.path().join("tui_trace.log");

        let status = self
            .bin()
            .arg("init")
            .arg("--env-file")
            .arg(&self.env_file)
            .arg("--tui")
            .env("FERREX_INIT_TUI_SCRIPT", &script_path)
            .env("FERREX_INIT_TUI_TRACE", &trace_path)
            .env("FERREX_INIT_TEST_SEED", "111")
            .status()?;

        if !status.success() {
            anyhow::bail!("tui init failed with exit code: {}", status);
        }
        eprintln!("  ✓ TUI init completed");
        Ok(())
    }

    fn run_init_noninteractive(&self) -> anyhow::Result<()> {
        eprintln!("  → Running non-interactive init...");
        let status = self
            .bin()
            .arg("init")
            .arg("--env-file")
            .arg(&self.env_file)
            .arg("--non-interactive")
            .env("FERREX_INIT_TEST_SEED", "222")
            .status()?;
        if !status.success() {
            anyhow::bail!(
                "non-interactive init failed with exit code: {}",
                status
            );
        }
        eprintln!("  ✓ Non-interactive init completed");
        Ok(())
    }

    fn stack_up(&self, server_mode: &str, clean: bool) -> anyhow::Result<()> {
        eprintln!(
            "  → Bringing stack up (server: {}, clean: {})...",
            server_mode, clean
        );
        let mut cmd = self.bin();
        cmd.arg("stack")
            .arg("up")
            .arg("--env-file")
            .arg(&self.env_file)
            .arg("--mode")
            .arg("local")
            .arg("--profile")
            .arg("dev")
            .arg("--server")
            .arg(server_mode)
            .arg("--non-interactive")
            .arg("--advanced");
        if clean {
            cmd.arg("--clean");
        }

        // Show real-time output for long-running docker operations
        let status = cmd.status()?;
        if !status.success() {
            anyhow::bail!("stack up failed with exit code: {}", status);
        }
        eprintln!("  ✓ Stack up completed");
        Ok(())
    }

    fn stack_up_reset_db(&self, server_mode: &str) -> anyhow::Result<()> {
        eprintln!(
            "  → Bringing stack up with --reset-db (server: {})...",
            server_mode
        );
        let mut cmd = self.bin();
        cmd.arg("stack")
            .arg("up")
            .arg("--env-file")
            .arg(&self.env_file)
            .arg("--mode")
            .arg("local")
            .arg("--profile")
            .arg("dev")
            .arg("--server")
            .arg(server_mode)
            .arg("--non-interactive")
            .arg("--advanced")
            .arg("--reset-db");

        let status = cmd.status()?;
        if !status.success() {
            anyhow::bail!(
                "stack up --reset-db failed with exit code: {}",
                status
            );
        }
        eprintln!("  ✓ Stack up with --reset-db completed");
        Ok(())
    }

    fn stack_down(&self, clean: bool) -> anyhow::Result<()> {
        eprintln!("  → Bringing stack down (clean: {})...", clean);
        let mut cmd = self.bin();
        cmd.arg("stack")
            .arg("down")
            .arg("--env-file")
            .arg(&self.env_file)
            .arg("--mode")
            .arg("local")
            .arg("--profile")
            .arg("dev");
        if clean {
            cmd.arg("--clean");
        }

        // Show real-time output
        let status = cmd.status()?;
        if !status.success() {
            anyhow::bail!("stack down failed with exit code: {}", status);
        }
        eprintln!("  ✓ Stack down completed");
        Ok(())
    }

    fn status(&self) -> anyhow::Result<()> {
        eprintln!("  → Checking stack status...");
        let status = self
            .bin()
            .arg("status")
            .arg("--env-file")
            .arg(&self.env_file)
            .arg("--mode")
            .arg("local")
            .arg("--profile")
            .arg("dev")
            .status()?;
        if !status.success() {
            anyhow::bail!("status check failed with exit code: {}", status);
        }
        eprintln!("  ✓ Stack status check completed");
        Ok(())
    }

    fn check(&self) -> anyhow::Result<()> {
        eprintln!("  → Running connectivity checks (DB, Redis, etc.)...");
        let status = self
            .bin()
            .arg("check")
            .arg("--env-file")
            .arg(&self.env_file)
            .env("PGCONNECT_TIMEOUT", "20")
            .status()?;
        if !status.success() {
            anyhow::bail!(
                "connectivity check failed with exit code: {}",
                status
            );
        }
        eprintln!("  ✓ Connectivity checks passed");
        Ok(())
    }
}

fn find_free_port() -> anyhow::Result<u16> {
    let socket = TcpListener::bind("127.0.0.1:0")?;
    Ok(socket.local_addr()?.port())
}

fn copy_compose_files(target_root: &Path, ports: Ports) -> anyhow::Result<()> {
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();

    // Copy and sanitize compose files
    for name in ["docker-compose.yml", "docker-compose.tailscale.yml"] {
        let src = manifest_root.join(name);
        if !src.exists() {
            continue;
        }
        let dst = target_root.join(name);
        let contents = fs::read_to_string(&src)?;
        let sanitized = sanitize_compose(&contents, ports);
        fs::write(dst, sanitized)?;
    }

    // Copy seccomp profile file (required by docker-compose.yml)
    let seccomp_src = manifest_root.join("seccomp-allow-iouring.json");
    if seccomp_src.exists() {
        let seccomp_dst = target_root.join("seccomp-allow-iouring.json");
        fs::copy(&seccomp_src, &seccomp_dst)?;
    }

    Ok(())
}

fn sanitize_compose(contents: &str, ports: Ports) -> String {
    let mut out = contents.to_string();

    // For the ferrex service, try to use an existing image if available to speed up tests
    // Check if we should use a pre-built image for faster testing
    let use_prebuilt =
        std::env::var("FERREX_TEST_USE_PREBUILT").unwrap_or_default() == "1";

    if use_prebuilt {
        // Replace the build section with a pre-built image reference
        // This regex matches the ferrex service block with its build section
        let re_ferrex_build = Regex::new(
            r"(?ms)(ferrex:\s*\n)(\s+build:.*?\n(?:\s+\w+:.*?\n)*?)(\s+image:.*?\n)"
        ).unwrap();
        if re_ferrex_build.is_match(&out) {
            out = re_ferrex_build.replace(&out, "$1$3").to_string();
        }
    } else {
        // Ensure build context points to the real workspace so Dockerfile and sources exist.
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .display()
            .to_string();
        let re_context = Regex::new(r"(?m)^(\s*)context:\s*\.?\s*$").unwrap();
        out = re_context
            .replace_all(&out, format!("${{1}}context: {}", workspace))
            .to_string();
    }

    // Remove container_name to avoid collisions across runs.
    let re_container = Regex::new(r"(?m)^\s*container_name:.*\n").unwrap();
    out = re_container.replace_all(&out, "").to_string();

    // Fix the initdb volume path to point to the workspace
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .display()
        .to_string();
    let re_initdb = Regex::new(r"(?m)(\s+-)(\s+)\./docker/initdb:").unwrap();
    out = re_initdb
        .replace_all(&out, format!("${{1}}${{2}}{}/docker/initdb:", workspace))
        .to_string();

    // Rebind host repository_ports to the randomly assigned ones.
    out = replace_port(out, 5432, ports.db);
    out = replace_port(out, 6379, ports.redis);
    out = replace_port(out, 3000, ports.app);
    out
}

fn replace_port(
    contents: String,
    container_port: u16,
    host_port: u16,
) -> String {
    let re = Regex::new(&format!(
        r#"(?m)^(\s*-\s*)"?(?P<host>\d+):{container_port}"?\s*$"#
    ))
    .unwrap();
    re.replace_all(&contents, format!(r#"$1"{host_port}:{container_port}""#))
        .to_string()
}

fn docker_available() -> bool {
    Command::new("docker")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn find_ferrex_init_binary() -> Option<PathBuf> {
    let mut discovered_paths = Vec::new();

    // 1. Check environment variable first (highest priority)
    if let Ok(p) = std::env::var("FERREX_INIT_E2E_BIN") {
        let path = PathBuf::from(p);
        // Convert relative paths to absolute paths based on current directory
        if path.is_relative()
            && let Ok(abs_path) =
                std::env::current_dir().map(|cwd| cwd.join(&path))
            && abs_path.exists()
        {
            discovered_paths.push(abs_path.clone());
            eprintln!(
                "  [binary discovery] Found via FERREX_INIT_E2E_BIN (resolved): {}",
                abs_path.display()
            );
            eprintln!(
                "  [binary discovery] Total discovered paths: {:?}",
                discovered_paths
            );
            return Some(abs_path);
        }
        if path.exists() {
            discovered_paths.push(path.clone());
            eprintln!(
                "  [binary discovery] Found via FERREX_INIT_E2E_BIN: {}",
                path.display()
            );
            eprintln!(
                "  [binary discovery] Total discovered paths: {:?}",
                discovered_paths
            );
            return Some(path);
        }
        eprintln!(
            "  [binary discovery] FERREX_INIT_E2E_BIN set but file not found: {}",
            path.display()
        );
    }

    // 2. Check PATH using which (second priority)
    if let Ok(path) = which::which("ferrex-init") {
        discovered_paths.push(path.clone());
        eprintln!(
            "  [binary discovery] Found in PATH via which: {}",
            path.display()
        );
        eprintln!(
            "  [binary discovery] Total discovered paths: {:?}",
            discovered_paths
        );
        return Some(path);
    }

    // 3. Check local build directories (fallback)
    let local_paths = [
        PathBuf::from("target/debug/ferrex-init"),
        PathBuf::from("target/release/ferrex-init"),
    ];

    for local_path in &local_paths {
        if let Ok(abs_path) =
            std::env::current_dir().map(|cwd| cwd.join(local_path))
            && abs_path.exists()
        {
            discovered_paths.push(abs_path.clone());
            eprintln!(
                "  [binary discovery] Found in local build directory: {}",
                abs_path.display()
            );
            eprintln!(
                "  [binary discovery] Total discovered paths: {:?}",
                discovered_paths
            );
            return Some(abs_path);
        }
    }

    // No binary found
    eprintln!("  [binary discovery] No ferrex-init binary found!");
    eprintln!("  [binary discovery] Attempted methods:");
    eprintln!("    1. FERREX_INIT_E2E_BIN environment variable");
    eprintln!("    2. PATH lookup via 'which ferrex-init'");
    eprintln!(
        "    3. Local build directories: target/debug/ and target/release/"
    );
    eprintln!(
        "  [binary discovery] All discovered paths: {:?}",
        discovered_paths
    );
    None
}

#[test]
#[ignore]
fn tui_init_writes_env_without_placeholders() -> anyhow::Result<()> {
    eprintln!("\n=== Test: tui_init_writes_env_without_placeholders ===");
    let _g = serial_guard();
    if !docker_available() {
        eprintln!("skipping: docker not available");
        return Ok(());
    }

    eprintln!(
        "→ Setting up test harness with temp directories and repository_ports..."
    );
    let harness = Harness::new()?;
    harness.run_init_tui(&["s"])?;

    eprintln!("→ Validating .env file contents...");
    let env = fs::read_to_string(&harness.env_file)?;
    assert!(
        !env.contains("changeme_"),
        "env still contains placeholder secrets"
    );
    assert!(
        env.contains(&format!("SERVER_PORT={}", harness.ports.app)),
        "server port should match injected port"
    );
    eprintln!("✓ Test passed\n");
    Ok(())
}

#[test]
#[ignore]
fn stack_docker_roundtrip_and_check() -> anyhow::Result<()> {
    eprintln!("\n=== Test: stack_docker_roundtrip_and_check ===");
    let _g = serial_guard();
    if !docker_available() {
        eprintln!("skipping: docker not available");
        return Ok(());
    }

    eprintln!(
        "→ Setting up test harness with temp directories and repository_ports..."
    );
    let harness = Harness::new()?;
    harness.run_init_noninteractive()?;
    harness.stack_up("docker", true)?;
    harness.status()?;
    harness.check()?;
    harness.stack_down(true)?;
    eprintln!("✓ Test passed\n");
    Ok(())
}

#[test]
#[ignore]
fn stack_host_mode_roundtrip() -> anyhow::Result<()> {
    eprintln!("\n=== Test: stack_host_mode_roundtrip ===");
    let _g = serial_guard();
    if !docker_available() {
        eprintln!("skipping: docker not available");
        return Ok(());
    }

    eprintln!(
        "→ Setting up test harness with temp directories and repository_ports..."
    );
    let harness = Harness::new()?;
    harness.run_init_noninteractive()?;
    harness.stack_up("host", true)?;
    harness.status()?;
    harness.check()?;
    harness.stack_down(true)?;
    eprintln!("✓ Test passed\n");
    Ok(())
}

/// Regression test for shell environment variable precedence bug.
///
/// This tests the scenario where:
/// 1. `stack up --reset-db` regenerates DB credentials and writes them to .env
/// 2. A subsequent `stack up --clean` should use the NEW credentials from .env
///
/// Previously, this would fail because:
/// - The justfile loads .env into shell BEFORE running commands
/// - Init subprocess writes new credentials to .env
/// - But the parent shell still has old credentials
/// - Docker-compose gives shell vars precedence over --env-file
/// - Result: postgres initialized with old creds, server tries new creds → auth failure
///
/// The fix ensures compose commands explicitly load the .env file into the command
/// environment, overriding any stale shell-inherited variables.
#[test]
#[ignore]
fn stack_reset_db_then_clean_succeeds() -> anyhow::Result<()> {
    eprintln!("\n=== Test: stack_reset_db_then_clean_succeeds ===");
    eprintln!("(Regression test for shell env var precedence bug)");
    let _g = serial_guard();
    if !docker_available() {
        eprintln!("skipping: docker not available");
        return Ok(());
    }

    eprintln!(
        "→ Setting up test harness with temp directories and repository_ports..."
    );
    let harness = Harness::new()?;

    // Step 1: Initial bring-up with --reset-db (generates new credentials)
    eprintln!("\n--- Phase 1: Initial stack up with --reset-db ---");
    harness.stack_up_reset_db("docker")?;
    harness.status()?;
    harness.check()?;

    // Step 2: Bring up again with just --clean (should use credentials from .env)
    // This is the scenario that was failing before the fix.
    eprintln!("\n--- Phase 2: Second stack up with --clean (no reset-db) ---");
    harness.stack_up("docker", true)?;
    harness.status()?;
    harness.check()?;

    // Cleanup
    harness.stack_down(true)?;
    eprintln!("✓ Test passed\n");
    Ok(())
}
