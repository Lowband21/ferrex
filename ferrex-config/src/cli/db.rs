use crate::{
    cli::{
        options,
        specs::{
            compose_run_server_spec, compose_up_services_spec, host_db_spec,
            init_spec::ensure_env_initialized, run_spec, run_spec_inherit,
            wait_for_services,
        },
        stack::{ServerMode, StackMode},
        utils::resolve_project_name,
    },
    env_writer::read_env_map,
};

use tokio::time::Duration;

use anyhow::{Result, bail};

pub async fn stack_db_preflight(
    opts: &options::StackOptions,
    extra_args: &str,
) -> Result<()> {
    ensure_env_initialized(opts).await?;
    if !opts.env_file.exists() {
        bail!(
            "env file {} is missing even after init",
            opts.env_file.display()
        );
    }

    if matches!(opts.server_mode, ServerMode::Docker) {
        if matches!(opts.mode, StackMode::Tailscale) {
            bail!(
                "db preflight is not supported in tailscale mode (compose run conflicts with network_mode=service:tailscale)"
            );
        }

        let project_name = resolve_project_name(opts);
        let up = compose_up_services_spec(opts, &project_name, &["db"]);
        let status = run_spec(&up).await?;
        if !status.success() {
            bail!("docker compose up db exited with {}", status);
        }

        let _ = wait_for_services(
            opts,
            &project_name,
            &["db"],
            Duration::from_secs(90),
        )
        .await;

        let mut args: Vec<String> = vec!["db".into(), "preflight".into()];
        if !extra_args.trim().is_empty() {
            args.extend(extra_args.split_whitespace().map(|s| s.to_string()));
        }
        let run = compose_run_server_spec(opts, &project_name, &args);
        let status = run_spec_inherit(&run).await?;
        if !status.success() {
            bail!("db preflight exited with {}", status);
        }
        Ok(())
    } else {
        let env_map = read_env_map(&opts.env_file)?;
        let mut args: Vec<String> = Vec::new();
        if !extra_args.trim().is_empty() {
            args.extend(extra_args.split_whitespace().map(|s| s.to_string()));
        }
        let spec = host_db_spec(opts, &env_map, "preflight", &args)?;
        let status = run_spec_inherit(&spec).await?;
        if !status.success() {
            bail!("host db preflight exited with {}", status);
        }
        Ok(())
    }
}

pub async fn stack_db_migrate(
    opts: &options::StackOptions,
    extra_args: &str,
) -> Result<()> {
    ensure_env_initialized(opts).await?;
    if !opts.env_file.exists() {
        bail!(
            "env file {} is missing even after init",
            opts.env_file.display()
        );
    }

    if matches!(opts.server_mode, ServerMode::Docker) {
        let project_name = resolve_project_name(opts);
        let up = compose_up_services_spec(opts, &project_name, &["db"]);
        let status = run_spec(&up).await?;
        if !status.success() {
            bail!("docker compose up db exited with {}", status);
        }

        let _ = wait_for_services(
            opts,
            &project_name,
            &["db"],
            Duration::from_secs(90),
        )
        .await;

        let mut args: Vec<String> = vec!["db".into(), "migrate".into()];
        if !extra_args.trim().is_empty() {
            args.extend(extra_args.split_whitespace().map(|s| s.to_string()));
        }
        let run = compose_run_server_spec(opts, &project_name, &args);
        let status = run_spec_inherit(&run).await?;
        if !status.success() {
            bail!("db migrate exited with {}", status);
        }
        Ok(())
    } else {
        let env_map = read_env_map(&opts.env_file)?;
        let mut args: Vec<String> = Vec::new();
        if !extra_args.trim().is_empty() {
            args.extend(extra_args.split_whitespace().map(|s| s.to_string()));
        }
        let spec = host_db_spec(opts, &env_map, "migrate", &args)?;
        let status = run_spec_inherit(&spec).await?;
        if !status.success() {
            bail!("host db migrate exited with {}", status);
        }
        Ok(())
    }
}
