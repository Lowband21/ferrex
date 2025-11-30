//! Docker compose orchestration used by `ferrex-init stack`.
//!
//! This module builds command specs for compose up/down flows, derives project
//! names, and can optionally start the server in host mode while still using
//! docker for dependencies.

use std::path::PathBuf;

/// How the stack should run the Ferrex server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMode {
    Docker,
    Host,
}

/// Which docker-compose overlays to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackMode {
    Local,
    Tailscale,
}

#[derive(Debug, Clone)]
/// Summary emitted after stack actions.
pub struct StackOutcome {
    pub project_name: String,
    pub compose_files: Vec<PathBuf>,
    pub server_mode: ServerMode,
    pub tailscale: bool,
    pub reset_db: bool,
    pub host_server_pid: Option<u32>,
    pub host_server_pid_file: Option<PathBuf>,
    pub stopped_host_server_pid: Option<u32>,
    pub tailscale_serve_ran: bool,
}

mod utils {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::cli::{
        options,
        specs::{
            compose_running_services_spec, compose_up_docker_spec,
            compose_up_services_spec, host_server_spec,
            init_spec::env_contents_have_placeholders, reset_db_volume_spec,
        },
        utils::derive_compose_project_name,
    };

    use super::*;

    #[test]
    fn project_name_falls_back_to_ferrex() {
        let p = PathBuf::from(".env");
        assert_eq!(derive_compose_project_name(&p), "ferrex");
    }

    #[test]
    fn project_name_slugifies_parent() {
        let p = PathBuf::from("/tmp/Dev Env/.env");
        assert_eq!(derive_compose_project_name(&p), "ferrex-dev-env");
    }

    #[test]
    fn placeholder_detection_matches_patterns() {
        // Placeholder in a recognized secret field should trigger
        let s = "DATABASE_APP_PASSWORD=changeme_pw";
        assert!(env_contents_have_placeholders(s));

        // Placeholder in AUTH fields should trigger
        let s = "AUTH_PASSWORD_PEPPER=changeme_test";
        assert!(env_contents_have_placeholders(s));

        // MEDIA_ROOT with /change/me placeholder should trigger
        let s = "MEDIA_ROOT=/change/me";
        assert!(env_contents_have_placeholders(s));

        // Non-placeholder values should not trigger
        let s = "MEDIA_ROOT=/data\nCUSTOM=ok";
        assert!(!env_contents_have_placeholders(s));

        // Placeholder-like values in non-secret fields should NOT trigger
        let s = "CUSTOM_FIELD=changeme_ignored";
        assert!(!env_contents_have_placeholders(s));

        // Comments with placeholder patterns should NOT trigger
        let s = "# changeme_comment\nDATABASE_APP_PASSWORD=real_password";
        assert!(!env_contents_have_placeholders(s));
    }

    fn sample_opts() -> options::StackOptions {
        options::StackOptions {
            env_file: PathBuf::from("/tmp/test/.env"),
            mode: StackMode::Local,
            profile: "release".into(),
            rust_log: Some("info".into()),
            wild: Some(true),
            server_mode: ServerMode::Docker,
            reset_db: false,
            clean: false,
            init_non_interactive: true,
            init_advanced: false,
            force_init: false,
            project_name_override: None,
            tailscale_serve: false,
            init_tui: false,
            skip_confirmation: false,
        }
    }

    #[test]
    fn compose_up_docker_spec_includes_files_and_envs() {
        let opts = sample_opts();
        let spec = compose_up_docker_spec(&opts, "ferrex-test", false);
        assert_eq!(spec.program, "docker");
        assert!(spec.args.starts_with(&["compose".into(), "-f".into(),]));
        assert!(
            spec.args.iter().any(|a| a.ends_with("docker-compose.yml")),
            "compose file args missing"
        );
        assert!(spec.args.contains(&"--env-file".into()));
        assert!(spec.args.contains(&"up".into()));
        assert!(
            !spec.args.contains(&"--build".into()),
            "--build should not be included when clean is false"
        );
        assert!(
            !spec.args.contains(&"--force-recreate".into()),
            "force-recreate should not be included when false"
        );

        let env: HashMap<_, _> = spec.env.iter().cloned().collect();
        assert_eq!(
            env.get("COMPOSE_PROJECT_NAME"),
            Some(&"ferrex-test".into())
        );
        assert_eq!(env.get("FERREX_BUILD_PROFILE"), Some(&"release".into()));
        assert_eq!(env.get("FERREX_ENABLE_WILD"), Some(&"1".into()));
        assert_eq!(env.get("RUST_LOG"), Some(&"info".into()));
    }

    #[test]
    fn compose_up_docker_spec_includes_build_when_clean() {
        let mut opts = sample_opts();
        opts.clean = true;
        let spec = compose_up_docker_spec(&opts, "ferrex-test", false);
        assert!(
            spec.args.contains(&"--build".into()),
            "--build should be included when clean is true"
        );
    }

    #[test]
    fn compose_up_docker_spec_force_recreate_when_reset_db() {
        let opts = sample_opts();
        let spec = compose_up_docker_spec(&opts, "ferrex-test", true);
        assert!(
            spec.args.contains(&"--force-recreate".into()),
            "force-recreate should be included when reset_db is true"
        );
    }

    #[test]
    fn compose_up_services_spec_targets_db_cache() {
        let opts = sample_opts();
        let spec =
            compose_up_services_spec(&opts, "ferrex-test", &["db", "cache"]);
        assert!(spec.args.ends_with(&["db".into(), "cache".into()]));
        assert!(spec.args.contains(&"up".into()));
    }

    #[test]
    fn compose_running_services_spec_filters_running() {
        let opts = sample_opts();
        let spec = compose_running_services_spec(
            opts.mode,
            &opts.env_file,
            &opts.profile,
            &opts.rust_log,
            opts.wild,
            "ferrex-test",
        );
        assert!(spec.args.contains(&"--services".into()));
        assert!(spec.args.contains(&"status=running".into()));
    }

    #[test]
    fn reset_db_volume_spec_uses_project_name() {
        let spec = reset_db_volume_spec("ferrex-dev");
        assert_eq!(spec.args.last().unwrap(), "ferrex-dev_postgres-data");
    }

    #[test]
    fn host_server_spec_includes_profile_and_envs() {
        let mut opts = sample_opts();
        opts.server_mode = ServerMode::Host;
        opts.profile = "dev".into();
        let mut env_map = HashMap::new();
        env_map.insert("DATABASE_URL".into(), "postgresql://x".into());
        let spec = host_server_spec(&opts, &env_map).expect("spec build");
        assert_eq!(spec.program, "cargo");
        assert!(spec.args.contains(&"ferrex-server".into()));
        assert!(spec.args.contains(&"--profile".into()));
        assert!(spec.args.contains(&"dev".into()));
        let env: HashMap<_, _> = spec.env.iter().cloned().collect();
        assert_eq!(
            env.get("FERREX_ENV_FILE"),
            Some(&opts.env_file.display().to_string())
        );
        assert!(env.contains_key("DATABASE_URL"));
        assert_eq!(env.get("RUST_LOG"), Some(&"info".into()));
        assert!(spec.inherit_stdio);
    }
}
