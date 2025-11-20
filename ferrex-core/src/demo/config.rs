use crate::demo::policy::DemoPolicy;
use crate::types::library::LibraryType;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// High-level options describing how the demo media tree should be generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DemoSeedOptions {
    /// Optional explicit root directory. When omitted a temp directory is used
    /// by the server bootstrapper.
    pub root: Option<PathBuf>,
    /// Per-library configuration.
    pub libraries: Vec<DemoLibraryOptions>,
    /// Whether to intentionally introduce structural deviations.
    pub allow_deviations: bool,
    /// Approximate rate of deviations to introduce when enabled.
    pub deviation_rate: f32,
    /// Skip expensive metadata probes (FFmpeg) for demo files.
    pub skip_metadata_probe: bool,
    /// Allow zero-length files without failing validation.
    pub allow_zero_length_files: bool,
}

impl Default for DemoSeedOptions {
    fn default() -> Self {
        Self {
            root: None,
            libraries: vec![
                DemoLibraryOptions {
                    library_type: LibraryType::Movies,
                    name: Some("Demo Movies".into()),
                    movie_count: Some(12),
                    series_count: None,
                    seasons_per_series: None,
                    episodes_per_season: None,
                    allow_deviations: None,
                },
                DemoLibraryOptions {
                    library_type: LibraryType::Series,
                    name: Some("Demo Series".into()),
                    movie_count: None,
                    series_count: Some(3),
                    seasons_per_series: Some((1, 2)),
                    episodes_per_season: Some((4, 6)),
                    allow_deviations: None,
                },
            ],
            allow_deviations: false,
            deviation_rate: 0.15,
            skip_metadata_probe: true,
            allow_zero_length_files: true,
        }
    }
}

impl DemoSeedOptions {
    /// Load options from JSON encoded environment variable. Falls back to
    /// per-field environment overrides and finally defaults.
    pub fn from_env() -> Self {
        if let Ok(raw) = std::env::var("FERREX_DEMO_OPTIONS") {
            if let Ok(parsed) = serde_json::from_str::<DemoSeedOptions>(&raw) {
                return parsed;
            }
        }

        let mut opts = DemoSeedOptions::default();

        if let Ok(path) = std::env::var("FERREX_DEMO_ROOT") {
            if !path.is_empty() {
                opts.root = Some(PathBuf::from(path));
            }
        }

        if let Ok(flag) = std::env::var("FERREX_DEMO_ALLOW_DEVIATIONS") {
            opts.allow_deviations = matches_ignore_ascii_case(&flag, ["1", "true", "yes"]);
        }

        if let Ok(rate) = std::env::var("FERREX_DEMO_DEVIATION_RATE") {
            if let Ok(val) = rate.parse::<f32>() {
                opts.deviation_rate = val.clamp(0.0, 1.0);
            }
        }

        if let Ok(flag) = std::env::var("FERREX_DEMO_SKIP_METADATA") {
            opts.skip_metadata_probe = matches_ignore_ascii_case(&flag, ["1", "true", "yes"]);
        }

        if let Ok(flag) = std::env::var("FERREX_DEMO_ZERO_LENGTH") {
            opts.allow_zero_length_files = matches_ignore_ascii_case(&flag, ["1", "true", "yes"]);
        }

        if let Ok(count) = std::env::var("FERREX_DEMO_MOVIE_COUNT") {
            if let Ok(parsed) = count.parse::<usize>() {
                if let Some(first) = opts
                    .libraries
                    .iter_mut()
                    .find(|lib| lib.library_type == LibraryType::Movies)
                {
                    first.movie_count = Some(parsed.max(1));
                }
            }
        }

        if let Ok(series) = std::env::var("FERREX_DEMO_SERIES_COUNT") {
            if let Ok(parsed) = series.parse::<usize>() {
                if let Some(first) = opts
                    .libraries
                    .iter_mut()
                    .find(|lib| lib.library_type == LibraryType::Series)
                {
                    first.series_count = Some(parsed.max(1));
                }
            }
        }

        opts
    }

    pub fn policy(&self) -> DemoPolicy {
        DemoPolicy {
            allow_zero_length_files: self.allow_zero_length_files,
            skip_metadata_probe: self.skip_metadata_probe,
        }
    }
}

/// Configuration for a single demo library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DemoLibraryOptions {
    pub library_type: LibraryType,
    pub name: Option<String>,
    pub movie_count: Option<usize>,
    pub series_count: Option<usize>,
    /// Inclusive range expressed as (min, max).
    pub seasons_per_series: Option<(u8, u8)>,
    /// Inclusive range expressed as (min, max).
    pub episodes_per_season: Option<(u16, u16)>,
    /// Override per-library deviations.
    pub allow_deviations: Option<bool>,
}

impl Default for DemoLibraryOptions {
    fn default() -> Self {
        Self {
            library_type: LibraryType::Movies,
            name: None,
            movie_count: Some(8),
            series_count: None,
            seasons_per_series: Some((1, 1)),
            episodes_per_season: Some((4, 6)),
            allow_deviations: None,
        }
    }
}

impl DemoLibraryOptions {
    pub fn effective_deviation_flag(&self, global: bool) -> bool {
        self.allow_deviations.unwrap_or(global)
    }
}

fn matches_ignore_ascii_case(value: &str, options: impl IntoIterator<Item = &'static str>) -> bool {
    let value_lower = value.trim().to_ascii_lowercase();
    options
        .into_iter()
        .any(|candidate| value_lower == candidate)
}
