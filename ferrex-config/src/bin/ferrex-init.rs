use std::{collections::HashSet, fs, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use dialoguer::{Confirm, console::Term};
use ferrex_config::{
    cli::{
        self, CheckOptions, InitOptions, RotateTarget,
        db::{stack_db_migrate, stack_db_preflight},
        options::StackOptions,
        specs::{stack_down, stack_logs, stack_status, stack_up},
        stack::{ServerMode, StackMode},
    },
    constants::MANAGED_KEYS,
    env_writer::{
        merge_env_contents, merge_env_with_template, read_env_map,
        write_env_atomically,
    },
    runner::{self, Runner, RunnerChoice},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "ferrex-init", about = "Ferrex configuration bootstrapper")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate or refresh .env
    Init {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        advanced: bool,
        #[arg(long, help = "Use ratatui-based full-screen UI (experimental)")]
        tui: bool,
        #[arg(long)]
        tailscale: bool,
        #[arg(long, value_enum, default_value = "none")]
        rotate: RotateArg,
        #[arg(long)]
        force: bool,
        #[arg(long, value_enum, default_value = "auto")]
        runner: RunnerArg,
        #[arg(long, default_value = "ghcr.io/ferrex/init:latest")]
        docker_image: String,
        #[arg(long)]
        mount_suffix: Option<String>,
        /// Print the generated key/value pairs without writing .env
        #[arg(long)]
        print_only: bool,
    },
    /// Validate configuration and connectivity
    Check {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
        #[arg(long)]
        tls_cert: Option<PathBuf>,
        #[arg(long)]
        tls_key: Option<PathBuf>,
    },
    /// Show FERREX_SETUP_TOKEN from an env file
    ShowToken {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
    },
    /// Bring the stack up or down (docker compose with optional host server mode)
    Stack {
        #[command(subcommand)]
        action: StackAction,
    },
    /// Show stack status (docker compose ps)
    Status {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
        #[arg(long, value_enum, default_value = "local")]
        mode: StackModeArg,
        #[arg(long, default_value = "release")]
        profile: String,
        #[arg(long)]
        rust_log: Option<String>,
        #[arg(
            long,
            value_enum,
            default_value_t = WildChoice::Auto,
            help = "Wild linker toggle: on/off/auto (default: auto = leave env as-is)"
        )]
        wild: WildChoice,
        #[arg(long)]
        project: Option<String>,
    },
    /// Tail or fetch stack logs
    Logs {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
        #[arg(long, value_enum, default_value = "local")]
        mode: StackModeArg,
        #[arg(long, default_value = "release")]
        profile: String,
        #[arg(long)]
        rust_log: Option<String>,
        #[arg(
            long,
            value_enum,
            default_value_t = WildChoice::Auto,
            help = "Wild linker toggle: on/off/auto (default: auto = leave env as-is)"
        )]
        wild: WildChoice,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "ferrex")]
        service: String,
        #[arg(long)]
        follow: bool,
    },
    /// Database helpers (run in compose context)
    Db {
        #[command(subcommand)]
        action: DbAction,
    },
}

#[derive(Subcommand)]
enum DbAction {
    /// Run database preflight checks inside the stack
    Preflight {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
        #[arg(long, value_enum, default_value = "local")]
        mode: StackModeArg,
        #[arg(long, default_value = "release")]
        profile: String,
        #[arg(long)]
        rust_log: Option<String>,
        #[arg(
            long,
            value_enum,
            default_value_t = WildChoice::Auto,
            help = "Wild linker toggle: on/off/auto (default: auto = leave env as-is)"
        )]
        wild: WildChoice,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "")]
        args: String,
        #[arg(long, value_enum, default_value = "docker")]
        server: ServerModeArg,
    },
    /// Run database migrations inside the stack
    Migrate {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
        #[arg(long, default_value = "release")]
        profile: String,
        #[arg(long)]
        rust_log: Option<String>,
        #[arg(
            long,
            value_enum,
            default_value_t = WildChoice::Auto,
            help = "Wild linker toggle: on/off/auto (default: auto = leave env as-is)"
        )]
        wild: WildChoice,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "")]
        args: String,
        #[arg(long, value_enum, default_value = "docker")]
        server: ServerModeArg,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum RotateArg {
    None,
    Db,
    Auth,
    All,
}

#[derive(Clone, Copy, ValueEnum)]
enum RunnerArg {
    Auto,
    Host,
    Docker,
}

#[derive(Clone, Copy, ValueEnum)]
enum WildChoice {
    Auto,
    On,
    Off,
}

#[allow(clippy::from_over_into)]
impl Into<Option<bool>> for WildChoice {
    fn into(self) -> Option<bool> {
        match self {
            WildChoice::Auto => None,
            WildChoice::On => Some(true),
            WildChoice::Off => Some(false),
        }
    }
}

#[derive(Subcommand)]
enum StackAction {
    Up {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
        #[arg(long, value_enum, default_value = "local")]
        mode: StackModeArg,
        #[arg(
            long,
            help = "Cargo/Docker build profile (applies to all stack actions)",
            default_value = "release"
        )]
        profile: String,
        #[arg(long)]
        rust_log: Option<String>,
        #[arg(
            long,
            value_enum,
            default_value_t = WildChoice::Auto,
            help = "Wild linker toggle: on/off/auto (default: auto = leave env as-is)"
        )]
        wild: WildChoice,
        #[arg(long, value_enum, default_value = "docker")]
        server: ServerModeArg,
        #[arg(long)]
        reset_db: bool,
        #[arg(long)]
        clean: bool,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long, default_value = "true")]
        tui: bool,
        #[arg(long)]
        advanced: bool,
        #[arg(long)]
        force_init: bool,
        #[arg(long)]
        project: Option<String>,
        #[arg(
            long,
            help = "Run tailscale serve after stack up (tailscale mode only)"
        )]
        tailscale_serve: Option<bool>,
    },
    Down {
        #[arg(long, default_value = ".env")]
        env_file: PathBuf,
        #[arg(long, value_enum, default_value = "local")]
        mode: StackModeArg,
        #[arg(long, default_value = "release")]
        profile: String,
        #[arg(long)]
        rust_log: Option<String>,
        #[arg(
            long,
            value_enum,
            default_value_t = WildChoice::Auto,
            help = "Wild linker toggle: on/off/auto (default: auto = leave env as-is)"
        )]
        wild: WildChoice,
        #[arg(long, value_enum, default_value = "docker")]
        server: ServerModeArg,
        #[arg(long)]
        clean: bool,
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum StackModeArg {
    Local,
    Tailscale,
}

#[derive(Clone, Copy, ValueEnum)]
enum ServerModeArg {
    Docker,
    Host,
}
impl From<RotateArg> for RotateTarget {
    fn from(val: RotateArg) -> Self {
        match val {
            RotateArg::None => RotateTarget::None,
            RotateArg::Db => RotateTarget::Db,
            RotateArg::Auth => RotateTarget::Auth,
            RotateArg::All => RotateTarget::All,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Init {
            env_file,
            non_interactive,
            advanced,
            tui,
            tailscale,
            rotate,
            force,
            runner,
            docker_image,
            mount_suffix,
            print_only,
        } => {
            let opts = InitOptions {
                env_path: env_file.clone(),
                non_interactive,
                advanced,
                tailscale,
                rotate: rotate.into(),
                force,
                tui,
            };
            let auto_confirm =
                std::env::var("FERREX_INIT_AUTO_CONFIRM").is_ok();

            let runner_choice = match runner {
                RunnerArg::Auto => RunnerChoice::Auto,
                RunnerArg::Host => RunnerChoice::Host,
                RunnerArg::Docker => RunnerChoice::Docker,
            };
            let selected = runner::choose_runner(runner_choice);

            let outcome = match selected {
                Runner::Host => cli::gen_init_merge_env(&opts).await?,
                Runner::Docker => {
                    let kv = runner::run_docker_init(
                        &docker_image,
                        &env_file,
                        tailscale,
                        advanced,
                        non_interactive,
                        match rotate {
                            RotateArg::None => None,
                            RotateArg::Db => Some("db"),
                            RotateArg::Auth => Some("auth"),
                            RotateArg::All => Some("all"),
                        },
                        force,
                        mount_suffix.as_deref(),
                    )?;
                    cli::InitOutcome {
                        kv,
                        rotated_keys: Vec::new(),
                    }
                }
            };

            if print_only {
                for (key, value) in &outcome.kv {
                    println!("{key}={value}");
                }
                return Ok(());
            }

            let managed: HashSet<String> =
                MANAGED_KEYS.iter().map(|s: &&str| s.to_string()).collect();
            let existing_raw =
                fs::read_to_string(&opts.env_path).unwrap_or_default();
            let template = opts
                .env_path
                .parent()
                .map(|dir| dir.join(".env.example"))
                .and_then(|path| fs::read_to_string(path).ok());

            let merged = if let Some(template) = template {
                merge_env_with_template(
                    &existing_raw,
                    &outcome.kv,
                    &managed,
                    &template,
                )
            } else {
                merge_env_contents(&existing_raw, &outcome.kv, &managed)
            };

            // Show a diff and ask for confirmation in interactive modes.
            if !non_interactive && !auto_confirm {
                let before_map =
                    read_env_map(&opts.env_path).unwrap_or_default();
                let mut after_map: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for (k, v) in &outcome.kv {
                    after_map.insert(k.clone(), v.clone());
                }

                let mut additions = Vec::new();
                let mut updates = Vec::new();
                let mut removals = Vec::new();

                for key in MANAGED_KEYS {
                    let key = *key;
                    let before = before_map.get(key);
                    let after = after_map.get(key);
                    match (before, after) {
                        (None, Some(new)) => additions.push((key, new.clone())),
                        (Some(old), Some(new)) if old != new => {
                            updates.push((key, old.clone(), new.clone()))
                        }
                        (Some(_), None) => removals.push(key),
                        _ => {}
                    }
                }

                if !additions.is_empty()
                    || !updates.is_empty()
                    || !removals.is_empty()
                {
                    println!();
                    println!(
                        "Proposed changes to {}:",
                        opts.env_path.display()
                    );

                    if !additions.is_empty() {
                        println!();
                        println!("  Added managed keys:");
                        for (k, v) in &additions {
                            println!("    + {k}={v}");
                        }
                    }
                    if !updates.is_empty() {
                        println!();
                        println!("  Updated managed keys:");
                        for (k, old, new) in &updates {
                            println!("    ~ {k}: {old} -> {new}");
                        }
                    }
                    if !removals.is_empty() {
                        println!();
                        println!("  Removed managed keys:");
                        for k in &removals {
                            println!("    - {k}");
                        }
                    }

                    println!();
                    println!(
                        "Apply these changes to {}?",
                        opts.env_path.display()
                    );

                    let confirm_prompt = if tui {
                        "Confirm and write .env from TUI session?"
                    } else {
                        "Confirm and write .env?"
                    };
                    let confirmed = Confirm::new()
                        .with_prompt(confirm_prompt)
                        .default(true)
                        .interact_on(&Term::stderr())?;
                    if !confirmed {
                        println!("Aborted; .env was not modified.");
                        return Ok(());
                    }
                } else {
                    println!(
                        "No changes to managed keys; .env will remain unchanged."
                    );
                    // Still write merged to keep layout aligned with template, if any.
                }
            }

            write_env_atomically(&opts.env_path, &merged)?;

            println!(
                "Wrote {} ({} managed keys, {} rotated) via {:?} runner",
                opts.env_path.display(),
                outcome.kv.len(),
                outcome.rotated_keys.len(),
                selected,
            );
            if !outcome.rotated_keys.is_empty() {
                println!("Rotated: {}", outcome.rotated_keys.join(", "));
            }
        }
        Command::Check {
            env_file,
            tls_cert,
            tls_key,
        } => {
            let opts = CheckOptions {
                config_path: None,
                env_file: Some(env_file),
                tls_cert_path: tls_cert,
                tls_key_path: tls_key,
            };
            cli::run_config_check(&opts).await?;
        }
        Command::ShowToken { env_file } => {
            match load_env_value(&env_file, "FERREX_SETUP_TOKEN")? {
                Some(token) if !token.trim().is_empty() => println!("{token}"),
                _ => println!(
                    "FERREX_SETUP_TOKEN not found in {}",
                    env_file.display()
                ),
            }
        }
        Command::Stack { action } => match action {
            StackAction::Up {
                env_file,
                mode,
                profile,
                rust_log,
                wild,
                server,
                reset_db,
                clean,
                non_interactive,
                tui,
                advanced,
                force_init,
                project,
                tailscale_serve,
            } => {
                let tailscale_serve = tailscale_serve.unwrap_or(match mode {
                    StackModeArg::Tailscale => true,
                    StackModeArg::Local => false,
                });

                let opts = StackOptions {
                    env_file,
                    mode: mode.into(),
                    profile,
                    rust_log,
                    wild: wild.into(),
                    server_mode: server.into(),
                    project_name_override: project,
                    reset_db,
                    clean,
                    init_non_interactive: non_interactive,
                    init_tui: tui,
                    init_advanced: advanced,
                    force_init,
                    tailscale_serve,
                };
                let outcome = stack_up(&opts).await?;
                print_stack_outcome("up", &outcome);
            }
            StackAction::Down {
                env_file,
                mode,
                profile,
                rust_log,
                wild,
                server,
                clean,
                project,
            } => {
                let options = stack_opts_from_args(
                    env_file, mode, profile, rust_log, wild, server, false,
                    clean, false, false, false, project, false,
                );
                let outcome = stack_down(&options).await?;
                print_stack_outcome("down", &outcome);
            }
        },
        Command::Status {
            env_file,
            mode,
            profile,
            rust_log,
            wild,
            project,
        } => {
            let opts = stack_opts_from_args(
                env_file,
                mode,
                profile,
                rust_log,
                wild,
                ServerModeArg::Docker,
                false,
                false,
                false,
                false,
                false,
                project,
                false,
            );
            stack_status(&opts).await?;
        }
        Command::Logs {
            env_file,
            mode,
            profile,
            rust_log,
            wild,
            project,
            service,
            follow,
        } => {
            let opts = stack_opts_from_args(
                env_file,
                mode,
                profile,
                rust_log,
                wild,
                ServerModeArg::Docker,
                false,
                false,
                false,
                false,
                false,
                project,
                false,
            );
            let svc = if service.trim().is_empty() {
                None
            } else {
                Some(service.as_str())
            };
            stack_logs(&opts, svc, follow).await?;
        }
        Command::Db { action } => match action {
            DbAction::Preflight {
                env_file,
                mode,
                profile,
                rust_log,
                wild,
                project,
                args,
                server,
            } => {
                let opts = stack_opts_from_args(
                    env_file, mode, profile, rust_log, wild, server, false,
                    false, true, false, false, project, false,
                );
                stack_db_preflight(&opts, &args).await?;
            }
            DbAction::Migrate {
                env_file,
                profile,
                rust_log,
                wild,
                project,
                args,
                server,
            } => {
                let wild_opt = match wild {
                    WildChoice::On => Some(true),
                    WildChoice::Off => Some(false),
                    WildChoice::Auto => None,
                };

                let opts = StackOptions {
                    env_file,
                    profile,
                    rust_log,
                    wild: wild_opt,
                    server_mode: server.into(),
                    project_name_override: project,
                    ..Default::default()
                };
                stack_db_migrate(&opts, &args).await?;
            }
        },
    }

    Ok(())
}

fn load_env_value(path: &PathBuf, key: &str) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    for entry in dotenvy::from_path_iter(path)? {
        let (k, v) = entry?;
        if k == key {
            return Ok(Some(v));
        }
    }

    Ok(None)
}

impl From<StackModeArg> for StackMode {
    fn from(val: StackModeArg) -> Self {
        match val {
            StackModeArg::Local => StackMode::Local,
            StackModeArg::Tailscale => StackMode::Tailscale,
        }
    }
}

impl From<ServerModeArg> for ServerMode {
    fn from(val: ServerModeArg) -> Self {
        match val {
            ServerModeArg::Docker => ServerMode::Docker,
            ServerModeArg::Host => ServerMode::Host,
        }
    }
}

fn stack_opts_from_args(
    env_file: PathBuf,
    mode: StackModeArg,
    profile: String,
    rust_log: Option<String>,
    wild: WildChoice,
    server: ServerModeArg,
    reset_db: bool,
    clean: bool,
    non_interactive: bool,
    advanced: bool,
    force_init: bool,
    project: Option<String>,
    tailscale_serve: bool,
) -> StackOptions {
    let wild_opt = match wild {
        WildChoice::On => Some(true),
        WildChoice::Off => Some(false),
        WildChoice::Auto => None,
    };
    let stack_mode: StackMode = mode.into();

    StackOptions {
        env_file,
        mode: stack_mode,
        profile,
        rust_log,
        wild: wild_opt,
        server_mode: server.into(),
        reset_db,
        clean,
        init_non_interactive: non_interactive,
        init_advanced: advanced,
        force_init,
        project_name_override: project,
        tailscale_serve,
        init_tui: true,
    }
}

fn print_stack_outcome(
    action: &str,
    outcome: &ferrex_config::cli::stack::StackOutcome,
) {
    let files: Vec<String> = outcome
        .compose_files
        .iter()
        .map(|p: &PathBuf| p.display().to_string())
        .collect();
    println!(
        "Stack {} complete: server={:?}, tailscale={}, project={}, files={}",
        action,
        outcome.server_mode,
        outcome.tailscale,
        outcome.project_name,
        files.join(", ")
    );
    if outcome.reset_db {
        println!("Database volume reset requested (postgres-data)");
    }
    if let Some(pid) = outcome.host_server_pid {
        println!("Host ferrex-server started with pid {}", pid);
    }
    if let Some(pid) = outcome.stopped_host_server_pid {
        println!("Stopped host ferrex-server pid {}", pid);
    }
    if outcome.tailscale && outcome.tailscale_serve_ran {
        println!("tailscale serve configured for http://127.0.0.1:3000");
    }
}
