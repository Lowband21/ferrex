pub mod init_spec;

use crate::{
    cli::{
        fs, options,
        stack::{ServerMode, StackMode, StackOutcome},
        utils::{
            compose_root, host_pid_file_path, resolve_project_name,
            workspace_root,
        },
    },
    env_writer::read_env_map,
};

use std::{
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};

use tokio::{
    process::Command,
    time::{Duration, Instant, sleep},
};
use tracing::{debug, error, info, trace, warn};

/// Abstract command representation so we can test without spawning processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub inherit_stdio: bool,
}

/// Display raw command string
impl Display for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CMD:`{}{} {}`\nENV:\n{}",
            self.cwd
                .clone()
                .map(|dir| {
                    let mut dir_str = dir.to_string_lossy().to_string();
                    if !dir_str.ends_with("/") {
                        dir_str.push('/')
                    }
                    dir_str
                })
                .unwrap_or_default(),
            self.program,
            self.args.join(" "),
            self.env
                .clone()
                .into_iter()
                .map(|v| format!("{}: {}", v.0, v.1))
                .collect::<Vec<String>>()
                .join("\n"),
        )
    }
}

impl CommandSpec {
    pub(crate) fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            inherit_stdio: false,
        }
    }

    pub fn to_command(spec: &CommandSpec) -> Command {
        to_command(spec)
    }
}

pub fn to_command(spec: &CommandSpec) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args);
    if !spec.env.is_empty() {
        cmd.envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    if spec.inherit_stdio {
        cmd.stdin(std::process::Stdio::inherit());
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
    }
    cmd
}

pub async fn spawn_spec(spec: &CommandSpec) -> Result<Option<u32>> {
    let child = to_command(spec)
        .spawn()
        .with_context(|| format!("failed to spawn {}", spec.program))?;
    Ok(child.id())
}

pub async fn run_spec(spec: &CommandSpec) -> Result<std::process::ExitStatus> {
    let status = to_command(spec)
        .status()
        .await
        .with_context(|| format!("failed to run {}", spec.program))?;
    Ok(status)
}

pub async fn run_spec_inherit(
    spec: &CommandSpec,
) -> Result<std::process::ExitStatus> {
    let mut spec = spec.clone();
    spec.inherit_stdio = true;
    run_spec(&spec).await
}

pub async fn run_spec_with_output(
    spec: &CommandSpec,
) -> Result<(std::process::ExitStatus, String)> {
    let output = to_command(spec)
        .output()
        .await
        .with_context(|| format!("failed to run {}", spec.program))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok((output.status, stdout))
}

pub fn reset_db_volume_spec(project_name: &str) -> CommandSpec {
    let mut spec = CommandSpec::new("docker");
    spec.args = vec![
        "volume".into(),
        "rm".into(),
        "-f".into(),
        format!("{project_name}_postgres-data"),
    ];
    spec
}

pub fn compose_files(mode: StackMode, root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("docker-compose.yml")];
    if matches!(mode, StackMode::Tailscale) {
        files.push(root.join("docker-compose.tailscale.yml"));
    }
    files
}

pub fn compose_base_spec(
    mode: StackMode,
    env_file: &Path,
    profile: &str,
    rust_log: &Option<String>,
    wild: Option<bool>,
    project_name: &str,
    compose_root: &Path,
) -> CommandSpec {
    let mut spec = CommandSpec::new("docker");
    spec.args = vec!["compose".into()];
    spec.cwd = Some(compose_root.to_path_buf());
    for file in compose_files(mode, compose_root) {
        spec.args.push("-f".into());
        spec.args.push(file.display().to_string());
    }
    if env_file.exists() {
        spec.args.push("--env-file".into());
        spec.args.push(env_file.display().to_string());

        match read_env_map(env_file) {
            Ok(env_map) => {
                debug!(
                    env_file = %env_file.display(),
                    var_count = env_map.len(),
                    "Loading .env file into compose command environment"
                );
                for (key, value) in env_map {
                    spec.env.push((key, value));
                }
            }
            Err(err) => {
                warn!(
                    env_file = %env_file.display(),
                    error = %err,
                    "Failed to read .env file for environment override"
                );
            }
        }

        spec.env
            .push(("FERREX_ENV_FILE".into(), env_file.display().to_string()));
    } else {
        warn!(
            "Environment file could not be found, likely to cause issues due to being unable to update environment values, especially if they were regenerated since config start."
        )
    }

    spec.env
        .push(("COMPOSE_PROJECT_NAME".into(), project_name.into()));
    spec.env
        .push(("FERREX_BUILD_PROFILE".into(), profile.to_string()));
    if let Some(val) = wild {
        spec.env.push((
            "FERREX_ENABLE_WILD".into(),
            if val { "1".into() } else { "0".into() },
        ));
    }
    if let Some(log) = rust_log {
        spec.env.push(("RUST_LOG".into(), log.clone()));
    }
    spec
}

pub fn compose_down_spec(
    mode: StackMode,
    env_file: &Path,
    profile: &str,
    rust_log: &Option<String>,
    wild: Option<bool>,
    project_name: &str,
    compose_root: &Path,
) -> CommandSpec {
    let mut spec = compose_base_spec(
        mode,
        env_file,
        profile,
        rust_log,
        wild,
        project_name,
        compose_root,
    );
    spec.args.extend(["down".into(), "--remove-orphans".into()]);
    spec
}

pub fn compose_up_docker_spec(
    opts: &options::StackOptions,
    project_name: &str,
    force_recreate: bool,
    compose_root: &Path,
) -> CommandSpec {
    let mut spec = compose_base_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        project_name,
        compose_root,
    );
    spec.args.extend(["up".into(), "-d".into()]);
    if opts.clean {
        spec.args.push("--build".into());
    }
    if force_recreate {
        spec.args.push("--force-recreate".into());
    }
    spec
}

pub fn tailscale_serve_spec(
    opts: &options::StackOptions,
    project_name: &str,
    compose_root: &Path,
) -> CommandSpec {
    let mut spec = compose_base_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        project_name,
        compose_root,
    );
    spec.args.extend([
        "exec".into(),
        "-T".into(),
        "tailscale".into(),
        "tailscale".into(),
        "serve".into(),
        "--bg".into(),
        "http://127.0.0.1:3000".into(),
    ]);
    spec.inherit_stdio = true;
    spec
}

pub fn compose_down_services_spec(
    opts: &options::StackOptions,
    project_name: &str,
    services: &[&str],
    compose_root: &Path,
) -> CommandSpec {
    let mut spec = compose_base_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        project_name,
        compose_root,
    );
    spec.args.push("down".into());
    for svc in services {
        spec.args.push((*svc).into());
    }
    spec
}

pub fn compose_up_services_spec(
    opts: &options::StackOptions,
    project_name: &str,
    services: &[&str],
    compose_root: &Path,
) -> CommandSpec {
    let mut spec = compose_base_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        project_name,
        compose_root,
    );
    spec.args.push("up".into());
    spec.args.push("-d".into());
    for svc in services {
        spec.args.push((*svc).into());
    }
    spec
}

pub fn compose_running_services_spec(
    mode: StackMode,
    env_file: &Path,
    profile: &str,
    rust_log: &Option<String>,
    wild: Option<bool>,
    project_name: &str,
    compose_root: &Path,
) -> CommandSpec {
    let mut spec = compose_base_spec(
        mode,
        env_file,
        profile,
        rust_log,
        wild,
        project_name,
        compose_root,
    );
    spec.args.extend([
        "ps".into(),
        "--services".into(),
        "--filter".into(),
        "status=running".into(),
    ]);
    spec
}

pub async fn wait_for_services(
    opts: &options::StackOptions,
    project_name: &str,
    services: &[&str],
    timeout: Duration,
    compose_root: &Path,
) -> Result<()> {
    let spec = compose_running_services_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        project_name,
        compose_root,
    );
    let deadline = Instant::now() + timeout;
    loop {
        let (status, out) = run_spec_with_output(&spec).await?;
        if status.success() {
            trace!("Services status from spec:\n`{}`\nOutput:\n{}", spec, out);
            let all_running = services.iter().all(|svc| out.contains(svc));
            if all_running {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for services to be running");
        }
        sleep(Duration::from_secs(1)).await;
    }
}

pub fn host_server_spec(
    opts: &options::StackOptions,
    env_map: &HashMap<String, String>,
) -> Result<CommandSpec> {
    if matches!(opts.mode, StackMode::Tailscale) {
        bail!(
            "Host server mode is not supported with tailscale compose overlay"
        );
    }

    let mut bin_path = compose_root();
    let path_ext = PathBuf::from(
        format!("target/{}/ferrex-server", opts.profile).as_str(),
    );
    bin_path.push(path_ext);

    let mut spec = if !opts.clean
        && let Ok(path_norm) = std::fs::canonicalize(&bin_path)
        && let Some(path_norm_str) = path_norm.to_str()
    {
        CommandSpec::new(path_norm_str)
    } else {
        let mut spec = CommandSpec::new("cargo");
        spec.cwd = Some(workspace_root());
        spec.args = vec!["run".into(), "-p".into(), "ferrex-server".into()];
        if opts.profile == "release" {
            spec.args.push("--release".into());
        } else {
            spec.args.push("--profile".into());
            spec.args.push(opts.profile.clone());
        }

        spec
    };

    for (k, v) in env_map {
        spec.env.push((k.clone(), v.clone()));
    }
    spec.env.push((
        "FERREX_ENV_FILE".into(),
        opts.env_file.display().to_string(),
    ));
    if let Some(log) = &opts.rust_log {
        spec.env.push(("RUST_LOG".into(), log.clone()));
    }

    spec.inherit_stdio = true;
    Ok(spec)
}

pub fn host_db_spec(
    opts: &options::StackOptions,
    env_map: &HashMap<String, String>,
    subcommand: &str,
    extra_args: &[String],
) -> Result<CommandSpec> {
    let mut spec = CommandSpec::new("cargo");
    spec.cwd = Some(workspace_root());

    spec.args = vec!["run".into(), "-p".into(), "ferrex-server".into()];

    if opts.profile == "release" {
        spec.args.push("--release".into());
    } else {
        spec.args.push("--profile".into());
        spec.args.push(opts.profile.clone());
    }

    spec.args.push("--".into());
    spec.args.push("db".into());

    spec.args.push(subcommand.into());

    for a in extra_args {
        spec.args.push(a.clone());
    }

    for (k, v) in env_map {
        spec.env.push((k.clone(), v.clone()));
    }

    spec.env.push((
        "FERREX_ENV_FILE".into(),
        opts.env_file.display().to_string(),
    ));

    spec.env.push(("SQLX_OFFLINE".into(), "true".into()));

    if let Some(log) = &opts.rust_log {
        spec.env.push(("RUST_LOG".into(), log.clone()));
    }

    spec.inherit_stdio = true;
    Ok(spec)
}

pub async fn kill_pid(pid: u32) -> Result<()> {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .await
        .with_context(|| format!("failed to send TERM to pid {pid}"))?;
    if !status.success() {
        bail!("kill exited with {status}");
    }

    // Wait briefly; if still alive, escalate to KILL.
    let mut attempts = 0;
    loop {
        attempts += 1;
        let alive = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        if !alive {
            return Ok(());
        }
        if attempts >= 3 {
            let _ = Command::new("kill")
                .arg("-KILL")
                .arg(pid.to_string())
                .status()
                .await;
            return Ok(());
        }
        sleep(Duration::from_millis(200)).await;
    }
}

pub async fn stop_host_server(pid_path: &Path) -> Result<Option<u32>> {
    if !pid_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(pid_path).with_context(|| {
        format!("failed to read pid file {}", pid_path.display())
    })?;
    let pid: u32 = contents
        .trim()
        .parse()
        .with_context(|| format!("invalid pid in {}", pid_path.display()))?;

    // Best-effort terminate; ignore errors but report pid stopped.
    let _ = kill_pid(pid).await;
    let _ = fs::remove_file(pid_path);
    Ok(Some(pid))
}

pub async fn hard_cleanup(project_name: &str, tailscale: bool) -> Result<()> {
    let mut containers = vec![
        "ferrex_media_db",
        "ferrex_media_cache",
        "ferrex_media_server",
    ];
    if tailscale {
        containers.push("ferrex_tailscale");
    }
    let mut spec = CommandSpec::new("docker");
    spec.args.push("rm".into());
    spec.args.push("-f".into());
    spec.args.extend(containers.iter().map(|c| (*c).into()));
    let _ = run_spec(&spec).await;

    let network = format!("{project_name}_default");
    let mut net = CommandSpec::new("docker");
    net.args = vec!["network".into(), "rm".into(), network];
    let _ = run_spec(&net).await;
    Ok(())
}

pub async fn stack_up(opts: &options::StackOptions) -> Result<StackOutcome> {
    // Ensure env directory exists.
    if let Some(parent) = opts.env_file.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create env file parent {}", parent.display())
        })?;
    }

    let compose_root = &compose_root();

    if matches!(opts.mode, StackMode::Tailscale)
        && matches!(opts.server_mode, ServerMode::Host)
    {
        bail!("Host server mode is not supported with tailscale overlay");
    }

    init_spec::ensure_env_initialized(opts).await?;

    if !opts.env_file.exists() {
        bail!(
            "env file {} is missing even after init",
            opts.env_file.display()
        );
    }

    let project_name = resolve_project_name(opts);
    info!("Bringing up {} compose project", project_name);

    if opts.clean || opts.reset_db {
        info!("Cleaning up old containers");

        let down = compose_down_spec(
            opts.mode,
            &opts.env_file,
            &opts.profile,
            &opts.rust_log,
            opts.wild,
            &project_name,
            compose_root,
        );
        let status = run_spec_inherit(&down).await?;
        if !status.success() {
            warn!("docker compose down exited with {:?}", status);
        }
    }

    if opts.reset_db {
        // Confirmation prompt for destructive operation
        if !opts.init_non_interactive && !opts.skip_confirmation {
            let auto_confirm = std::env::var("FERREXCTL_AUTO_CONFIRM").is_ok();
            if !auto_confirm {
                use dialoguer::{Confirm, console::Term};
                let confirmed = Confirm::new()
                    .with_prompt(
                        "WARNING: This will delete the database volume and all data. Continue?"
                    )
                    .default(false)
                    .interact_on(&Term::stderr())?;
                if !confirmed {
                    bail!("Database reset cancelled by user");
                }
            }
        }
        info!("Resetting Database Volume!");
        let reset = reset_db_volume_spec(&project_name);
        let _ = run_spec(&reset).await?;
    }

    let (host_server_pid, pid_file) = match opts.server_mode {
        ServerMode::Docker => {
            let up = compose_up_docker_spec(
                opts,
                &project_name,
                opts.reset_db,
                compose_root,
            );
            // Capture output for better error reporting
            let status = run_spec_inherit(&up).await?;
            if !status.success() {
                eprintln!("Docker compose up failed with status: {}", status);
                bail!(
                    "docker compose up failed - see output above for details"
                );
            }
            (None, None)
        }
        ServerMode::Host => {
            // Bring up only db/cache.
            if opts.clean || opts.reset_db {
                let down = compose_down_services_spec(
                    opts,
                    &project_name,
                    &["db", "cache"],
                    compose_root,
                );
                let _ = run_spec_inherit(&down).await?;
            }
            let up = compose_up_services_spec(
                opts,
                &project_name,
                &["db", "cache"],
                compose_root,
            );
            // Capture output for better error reporting
            let status = run_spec_inherit(&up).await?;
            if !status.success() {
                error!("Docker compose up failed with status: {}", status);
                bail!(
                    "docker compose up failed - see output above for details"
                );
            }
            // Wait briefly for db/cache to reach running state; warn on timeout but continue.
            if let Err(err) = wait_for_services(
                opts,
                &project_name,
                &["postgres", "redis"],
                Duration::from_secs(5),
                compose_root,
            )
            .await
            {
                warn!(
                    "Warning: db/cache may not be ready yet ({err}). \
             Check status with: docker compose --project-name {project_name} ps"
                );
            }

            let env_map = read_env_map(&opts.env_file)?;
            let spec = host_server_spec(opts, &env_map)?;
            if opts.host_attach {
                // Foreground mode: behave like `docker compose up` so the user sees
                // compile/runtime output immediately and can Ctrl-C to stop.
                let status = run_spec_inherit(&spec)
                    .await
                    .context("failed to run host server in foreground")?;
                if !status.success() {
                    bail!(
                        "host ferrex-server exited with non-zero status {}",
                        status
                    );
                }
                (None, None)
            } else {
                // Detached mode: spawn and write PID file for later `stack down`.
                let pid = spawn_spec(&spec)
                    .await
                    .context("failed to spawn host server task")?
                    .ok_or_else(|| {
                        anyhow!("failed to obtain host server pid")
                    })?;
                let pid_path =
                    host_pid_file_path(&opts.env_file, &project_name);
                if let Some(parent) = pid_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                fs::write(&pid_path, pid.to_string()).with_context(|| {
                    format!(
                        "failed to write host server pid to {}",
                        pid_path.display()
                    )
                })?;
                (Some(pid), Some(pid_path))
            }
        }
    };

    let mut tailscale_serve_ran = false;
    if matches!(opts.mode, StackMode::Tailscale)
        && matches!(opts.server_mode, ServerMode::Docker)
        && opts.tailscale_serve
    {
        // ensure tailscale container is up
        let _ = wait_for_services(
            opts,
            &project_name,
            &["tailscale"],
            Duration::from_secs(90),
            compose_root,
        )
        .await;
        let spec = tailscale_serve_spec(opts, &project_name, compose_root);
        match run_spec_inherit(&spec).await {
            Ok(status) if status.success() => {
                tailscale_serve_ran = true;
            }
            Ok(status) => {
                eprintln!(
                    "Warning: tailscale serve returned non-zero status {}. You may need to authenticate the tailscale sidecar and rerun serve manually.",
                    status
                );
            }
            Err(err) => {
                eprintln!(
                    "Warning: failed to run tailscale serve: {err}. Authenticate the sidecar then run: docker compose exec tailscale tailscale serve --bg http://127.0.0.1:3000"
                );
            }
        }
    }

    Ok(StackOutcome {
        project_name,
        compose_files: compose_files(opts.mode, compose_root),
        server_mode: opts.server_mode,
        tailscale: matches!(opts.mode, StackMode::Tailscale),
        reset_db: opts.reset_db,
        host_server_pid,
        host_server_pid_file: pid_file,
        stopped_host_server_pid: None,
        tailscale_serve_ran,
    })
}

pub async fn stack_down(opts: &options::StackOptions) -> Result<StackOutcome> {
    let project_name = resolve_project_name(opts);
    let compose_root = &compose_root();

    let pid_file = host_pid_file_path(&opts.env_file, &project_name);
    let stopped_pid = stop_host_server(&pid_file).await?;

    let down = compose_down_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        &project_name,
        compose_root,
    );
    let status = run_spec(&down).await?;
    if !status.success() {
        bail!("docker compose down exited with {}", status);
    }

    if opts.clean {
        hard_cleanup(&project_name, matches!(opts.mode, StackMode::Tailscale))
            .await
            .ok();
    }
    Ok(StackOutcome {
        project_name,
        compose_files: compose_files(opts.mode, compose_root),
        server_mode: opts.server_mode,
        tailscale: matches!(opts.mode, StackMode::Tailscale),
        reset_db: false,
        host_server_pid: None,
        host_server_pid_file: Some(pid_file),
        stopped_host_server_pid: stopped_pid,
        tailscale_serve_ran: false,
    })
}

pub fn compose_run_server_spec(
    opts: &options::StackOptions,
    project_name: &str,
    args: &[String],
    compose_root: &Path,
) -> CommandSpec {
    let mut spec = compose_base_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        project_name,
        compose_root,
    );
    spec.args.push("run".into());
    if opts.clean {
        spec.args.push("--rm".into());
        spec.args.push("--build".into());
    }
    spec.args.push("ferrex".into());
    spec.args.extend(args.iter().cloned());
    spec.inherit_stdio = true;
    spec
}

pub async fn stack_status(opts: &options::StackOptions) -> Result<()> {
    let project_name = resolve_project_name(opts);
    let compose_root = &compose_root();

    let mut spec = compose_base_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        &project_name,
        compose_root,
    );
    spec.args.push("ps".into());
    run_spec_inherit(&spec).await?;
    Ok(())
}

pub async fn stack_logs(
    opts: &options::StackOptions,
    service: Option<&str>,
    follow: bool,
) -> Result<()> {
    let project_name = resolve_project_name(opts);
    let compose_root = &compose_root();

    let mut spec = compose_base_spec(
        opts.mode,
        &opts.env_file,
        &opts.profile,
        &opts.rust_log,
        opts.wild,
        &project_name,
        compose_root,
    );

    spec.args.push("logs".into());
    if follow {
        spec.args.push("-f".into());
    }
    if let Some(svc) = service {
        spec.args.push(svc.into());
    }

    run_spec_inherit(&spec).await?;
    Ok(())
}
