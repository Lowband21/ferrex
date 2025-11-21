use anyhow::{Context, anyhow};
use ferrex_core::scan::{
    orchestration::config::OrchestratorConfig, scanner::settings,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn default_video_extensions() -> Vec<String> {
    settings::default_video_file_extensions_vec()
}

/// Source that produced the scanner configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ScannerConfigSource {
    #[default]
    Default,
    EnvPath(PathBuf),
    EnvInline,
    File(PathBuf),
}

/// Top-level scanner settings. Use these to tune how quickly new folders are
/// queued, how many workers run in parallel, and when a bulk scan is considered
/// finished.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ScannerConfig {
    /// Full orchestrator tuning: queue parallelism, priority weights, retry and
    /// lease policy, metadata throttles, maintenance scheduling, and filesystem
    /// watch debouncing. Raise the `queue` or `budget` limits to process more
    /// folders/files in parallel, but keep an eye on disk and network pressure.
    pub orchestrator: OrchestratorConfig,
    /// Per-library limit for queued maintenance jobs after the initial bulk
    /// sweep. Increase to let a library keep more follow-up scans pending; too
    /// high can starve other libraries on busy disks.
    pub library_actor_max_outstanding_jobs: usize,
    /// Idle window (ms) the aggregator waits after the queue drains before it
    /// declares the bulk scan complete. Shorter windows flip to maintenance
    /// faster; longer windows help when the filesystem reports changes slowly.
    pub quiescence_window_ms: u64,
    /// File extensions treated as video assets by the filesystem watcher.
    /// Defaults mirror the core's built-in allow-list so future user overrides
    /// can flow through without diverging behaviour.
    #[serde(default = "default_video_extensions")]
    pub video_extensions: Vec<String>,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        let mut orchestrator = OrchestratorConfig::default();
        if orchestrator.budget.image_fetch_limit
            < orchestrator.queue.max_parallel_image_fetch
        {
            orchestrator.budget.image_fetch_limit =
                orchestrator.queue.max_parallel_image_fetch;
        }

        Self {
            orchestrator,
            // Should make this num_cpus?
            library_actor_max_outstanding_jobs: 32,
            quiescence_window_ms: 5_000,
            video_extensions: default_video_extensions(),
        }
    }
}

impl ScannerConfig {
    /// Load scanner configuration overrides using environment variables.
    /// Evaluation order:
    /// 1) `$SCANNER_CONFIG_PATH` (TOML or JSON file),
    /// 2) `$SCANNER_CONFIG_JSON` (inline JSON),
    /// 3) defaults if neither is set.
    pub fn load_from_env() -> anyhow::Result<(Self, ScannerConfigSource)> {
        if let Ok(path_str) = env::var("SCANNER_CONFIG_PATH")
            && !path_str.trim().is_empty()
        {
            let path = PathBuf::from(path_str);
            let config = Self::load_from_file(&path)?;
            return Ok((config, ScannerConfigSource::EnvPath(path)));
        }

        if let Ok(raw) = env::var("SCANNER_CONFIG_JSON")
            && !raw.trim().is_empty()
        {
            let parsed = Self::parse_json(&raw)
                .context("failed to parse SCANNER_CONFIG_JSON")?;
            return Ok((parsed, ScannerConfigSource::EnvInline));
        }

        if let Some(path) = Self::find_default_file() {
            let config = Self::load_from_file(&path)?;
            return Ok((config, ScannerConfigSource::File(path)));
        }

        Ok((Self::default(), ScannerConfigSource::Default))
    }

    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path).with_context(|| {
            format!("failed to read scanner config from {}", path.display())
        })?;

        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Self::parse_json(&contents).with_context(|| {
                format!("invalid scanner config {}", path.display())
            }),
            Some("toml") | Some("tml") => {
                toml::from_str(&contents).map_err(|err| {
                    anyhow!(
                        "invalid scanner config {}: {}",
                        path.display(),
                        err
                    )
                })
            }
            _ => Self::parse_from_str(&contents, &path.display().to_string()),
        }
    }

    pub fn parse_from_str(
        contents: &str,
        origin: &str,
    ) -> anyhow::Result<Self> {
        // Try TOML first, then JSON for convenience.
        toml::from_str(contents).or_else(|toml_err| {
            serde_json::from_str(contents).map_err(|json_err| {
                anyhow!(
                    "failed to parse scanner config {}: toml error: {}; json error: {}",
                    origin,
                    toml_err,
                    json_err
                )
            })
        })
    }

    pub fn parse_json(raw: &str) -> anyhow::Result<Self> {
        serde_json::from_str(raw)
            .map_err(|err| anyhow!("invalid scanner config json: {err}"))
    }

    fn find_default_file() -> Option<PathBuf> {
        const CANDIDATES: &[&str] = &[
            "scanner.toml",
            "scanner.json",
            "config/scanner.toml",
            "config/scanner.json",
        ];

        CANDIDATES
            .iter()
            .map(Path::new)
            .find(|path| path.exists())
            .map(|path| path.to_path_buf())
    }
}
