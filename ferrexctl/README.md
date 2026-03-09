# ferrexctl

Configuration bootstrapper and packaging tool for the Ferrex media platform.

This crate provides both a library and CLI for managing Ferrex deployments:
- Environment configuration generation
- Docker Compose stack orchestration
- Database migrations and preflight checks
- Configuration validation
- Flatpak packaging and release artifact generation

## Installation

```bash
cargo install ferrexctl
```

Or via GitHub releases for pre-built binaries.

## CLI Usage

### Configuration & Stack Management

```bash
# Generate .env configuration
ferrexctl init

# Validate configuration and connectivity
ferrexctl check

# Start the Ferrex stack (Postgres, Redis, server)
ferrexctl stack up

# Stop the Ferrex stack
ferrexctl stack down

# Run database preflight checks
ferrexctl db preflight

# Run database migrations
ferrexctl db migrate

# View stack status
ferrexctl status

# Tail logs
ferrexctl logs --follow
```

### Packaging & Release

```bash
# Build Flatpak bundle (version from workspace Cargo.toml)
ferrexctl package flatpak

# Build with custom output path
ferrexctl package flatpak --output /tmp/ferrex-player.flatpak

# Build with custom version
ferrexctl package flatpak --version 0.2.0-beta

# Generate release artifacts (Flatpak + manifest + checksums)
ferrexctl package release

# Skip preflight checks (fmt, clippy, tests)
ferrexctl package release --skip-preflight

# Dry run (preview without writing)
ferrexctl package release --dry-run
```

## Options

```
--env-file <PATH>      Path to .env file (default: .env)
--mode <MODE>          Stack mode: local or tailscale
--profile <PROFILE>    Cargo/Docker profile (default: release)
--non-interactive      Skip interactive prompts
--advanced             Show advanced configuration options
--tui                  Use TUI interface for configuration
```

## Library Usage

```rust
use ferrexctl::cli::{InitOptions, gen_init_merge_env};

let opts = InitOptions {
    env_path: ".env".into(),
    non_interactive: true,
    ..Default::default()
};

let outcome = gen_init_merge_env(&opts).await?;
```

## License

Licensed under MIT OR Apache-2.0.
