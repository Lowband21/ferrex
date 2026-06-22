use anyhow::{Context, anyhow, bail};
use ferrex_model::scan::{
    orchestration::config::OrchestratorConfig, scanner::settings,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
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
    /// File extensions treated as video assets by the scanner.
    /// Defaults mirror the core's built-in allow-list; values are normalized to
    /// lowercase without a leading dot.
    #[serde(default = "default_video_extensions")]
    pub video_extensions: Vec<String>,
    /// File extensions to ignore even if they appear in a library folder.
    /// Useful for noisy network mounts that emit temporary sidecar files.
    #[serde(default)]
    pub ignored_extensions: Vec<String>,
    /// Path fragments to document operator ignore policy. These are exposed in
    /// config responses for observability while extension filters are the
    /// scanner-enforced filter surface today.
    #[serde(default)]
    pub ignored_path_patterns: Vec<String>,
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
        if orchestrator.budget.transcript_extraction_limit
            < orchestrator.queue.max_parallel_transcript_extract
        {
            orchestrator.budget.transcript_extraction_limit =
                orchestrator.queue.max_parallel_transcript_extract;
        }

        Self {
            orchestrator,
            // Should make this num_cpus?
            library_actor_max_outstanding_jobs: 32,
            quiescence_window_ms: 5_000,
            video_extensions: default_video_extensions(),
            ignored_extensions: Vec::new(),
            ignored_path_patterns: Vec::new(),
        }
    }
}

fn normalize_extensions(
    field: &str,
    values: &[String],
    allow_empty: bool,
) -> anyhow::Result<Vec<String>> {
    if values.is_empty() && !allow_empty {
        bail!("{field} must include at least one extension");
    }

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let ext = value.trim().trim_start_matches('.').to_ascii_lowercase();
        if ext.is_empty() {
            bail!("{field} entries must not be empty");
        }
        if ext.contains('/') || ext.contains('\\') {
            bail!("{field} entries must be extensions, not paths: `{value}`");
        }
        if ext.contains('*') {
            bail!("{field} entries must not contain wildcards: `{value}`");
        }
        if seen.insert(ext.clone()) {
            normalized.push(ext);
        }
    }
    Ok(normalized)
}

fn normalize_path_patterns(
    field: &str,
    values: &[String],
) -> anyhow::Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let pattern = value.trim();
        if pattern.is_empty() {
            bail!("{field} entries must not be empty");
        }
        if seen.insert(pattern.to_string()) {
            normalized.push(pattern.to_string());
        }
    }
    Ok(normalized)
}

fn ensure_nonzero_u64(field: &str, value: u64) -> anyhow::Result<()> {
    if value == 0 {
        bail!("{field} must be greater than 0");
    }
    Ok(())
}

fn ensure_nonzero_usize(field: &str, value: usize) -> anyhow::Result<()> {
    if value == 0 {
        bail!("{field} must be greater than 0");
    }
    Ok(())
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
                let parsed = toml::from_str(&contents).map_err(|err| {
                    anyhow!(
                        "invalid scanner config {}: {}",
                        path.display(),
                        err
                    )
                })?;
                Self::finalize(parsed)
            }
            _ => Self::parse_from_str(&contents, &path.display().to_string()),
        }
    }

    pub fn parse_from_str(
        contents: &str,
        origin: &str,
    ) -> anyhow::Result<Self> {
        // Try TOML first, then JSON for convenience.
        let parsed = toml::from_str(contents).or_else(|toml_err| {
            serde_json::from_str(contents).map_err(|json_err| {
                anyhow!(
                    "failed to parse scanner config {}: toml error: {}; json error: {}",
                    origin,
                    toml_err,
                    json_err
                )
            })
        })?;
        Self::finalize(parsed)
    }

    pub fn parse_json(raw: &str) -> anyhow::Result<Self> {
        let parsed = serde_json::from_str(raw)
            .map_err(|err| anyhow!("invalid scanner config json: {err}"))?;
        Self::finalize(parsed)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        ensure_nonzero_u64(
            "scanner.quiescence_window_ms",
            self.quiescence_window_ms,
        )?;
        ensure_nonzero_usize(
            "scanner.library_actor_max_outstanding_jobs",
            self.library_actor_max_outstanding_jobs,
        )?;

        let watch = &self.orchestrator.watch;
        ensure_nonzero_u64(
            "scanner.orchestrator.watch.debounce_window_ms",
            watch.debounce_window_ms,
        )?;
        ensure_nonzero_usize(
            "scanner.orchestrator.watch.max_batch_events",
            watch.max_batch_events,
        )?;
        ensure_nonzero_u64(
            "scanner.orchestrator.watch.poll_interval_ms",
            watch.poll_interval_ms,
        )?;
        ensure_nonzero_u64(
            "scanner.orchestrator.watch.poll_backoff_max_ms",
            watch.poll_backoff_max_ms,
        )?;
        if watch.poll_backoff_max_ms < watch.poll_interval_ms {
            bail!(
                "scanner.orchestrator.watch.poll_backoff_max_ms must be greater than or equal to poll_interval_ms"
            );
        }

        let maintenance = &self.orchestrator.maintenance;
        ensure_nonzero_u64(
            "scanner.orchestrator.maintenance.tick_interval_ms",
            maintenance.tick_interval_ms,
        )?;
        ensure_nonzero_usize(
            "scanner.orchestrator.maintenance.max_jobs_per_library",
            maintenance.max_jobs_per_library,
        )?;
        ensure_nonzero_usize(
            "scanner.orchestrator.maintenance.max_root_entries_per_library",
            maintenance.max_root_entries_per_library,
        )?;
        ensure_nonzero_u64(
            "scanner.orchestrator.maintenance.error_backoff_ms",
            maintenance.error_backoff_ms,
        )?;
        ensure_nonzero_u64(
            "scanner.orchestrator.maintenance.run_stall_timeout_ms",
            maintenance.run_stall_timeout_ms,
        )?;

        if self.video_extensions.is_empty() {
            bail!(
                "scanner.video_extensions must include at least one media extension"
            );
        }

        Ok(())
    }

    fn finalize(mut config: Self) -> anyhow::Result<Self> {
        config.video_extensions = normalize_extensions(
            "scanner.video_extensions",
            &config.video_extensions,
            false,
        )?;
        config.ignored_extensions = normalize_extensions(
            "scanner.ignored_extensions",
            &config.ignored_extensions,
            true,
        )?;
        config.ignored_path_patterns = normalize_path_patterns(
            "scanner.ignored_path_patterns",
            &config.ignored_path_patterns,
        )?;
        config.validate()?;
        Ok(config)
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

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_model::scan::orchestration::config::WatchStrategy;

    #[test]
    fn partial_watch_config_uses_defaults_and_normalizes_filters() {
        let config = ScannerConfig::parse_from_str(
            r#"
                video_extensions = [".MKV", "mp4", "mkv"]
                ignored_extensions = [".tmp", "PART"]
                ignored_path_patterns = ["**/.staging/**"]

                [orchestrator.watch]
                strategy = "poll"
                poll_interval_ms = 45000
            "#,
            "test",
        )
        .expect("scanner config should parse");

        assert_eq!(config.orchestrator.watch.strategy, WatchStrategy::Poll);
        assert_eq!(config.orchestrator.watch.poll_interval_ms, 45_000);
        assert_eq!(config.video_extensions, vec!["mkv", "mp4"]);
        assert_eq!(config.ignored_extensions, vec!["tmp", "part"]);
        assert_eq!(
            config.ignored_path_patterns,
            vec!["**/.staging/**".to_string()]
        );
    }

    #[test]
    fn invalid_poll_interval_fails_fast() {
        let err = ScannerConfig::parse_from_str(
            r#"
                [orchestrator.watch]
                poll_interval_ms = 0
            "#,
            "test",
        )
        .expect_err("zero poll interval should be rejected");

        assert!(
            err.to_string()
                .contains("scanner.orchestrator.watch.poll_interval_ms"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn invalid_extension_filter_fails_fast() {
        let err = ScannerConfig::parse_from_str(
            r#"
                video_extensions = ["mkv", "bad/path"]
            "#,
            "test",
        )
        .expect_err("path-like extension should be rejected");

        assert!(
            err.to_string().contains("scanner.video_extensions"),
            "unexpected error: {err}"
        );
    }
}
