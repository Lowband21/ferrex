use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Datelike;
use rand::Rng;

use crate::providers::TmdbApiProvider;
use crate::Result;

use super::fs::InMemoryFs;

/// A generated node in a folder structure plan.
#[derive(Debug, Clone)]
pub enum GeneratedNode {
    Dir(PathBuf),
    File { path: PathBuf, len: u64 },
}

/// A complete plan describing directories and files to create.
#[derive(Debug, Clone, Default)]
pub struct StructurePlan {
    pub nodes: Vec<GeneratedNode>,
}

impl StructurePlan {
    pub fn push_dir<P: Into<PathBuf>>(&mut self, p: P) {
        self.nodes.push(GeneratedNode::Dir(p.into()));
    }
    pub fn push_file<P: Into<PathBuf>>(&mut self, p: P, len: u64) {
        self.nodes.push(GeneratedNode::File { path: p.into(), len });
    }
}

/// Strategy for naming folders and files.
pub trait NamingStrategy: Send + Sync {
    fn movie_folder_name(&self, title: &str, year: Option<i32>) -> String;
    fn movie_file_name(&self, title: &str, year: Option<i32>, ext: &str) -> String;
    fn series_folder_name(&self, title: &str) -> String;
    fn season_folder_name(&self, season_number: u8) -> String;
    fn episode_file_name(
        &self,
        title: &str,
        season_number: u8,
        episode_number: u16,
        ext: &str,
    ) -> String;
}

pub struct DefaultNamingStrategy;

impl DefaultNamingStrategy {
    fn sanitize(&self, s: &str) -> String {
        // Replace characters that are problematic on common filesystems and trim whitespace
        let mut out = s
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
                _ => c,
            })
            .collect::<String>();
        // Collapse multiple spaces
        out = out.split_whitespace().collect::<Vec<_>>().join(" ");
        out.trim().to_string()
    }
}

impl NamingStrategy for DefaultNamingStrategy {
    fn movie_folder_name(&self, title: &str, year: Option<i32>) -> String {
        let name = self.sanitize(title);
        match year {
            Some(y) => format!("{} ({})", name, y),
            None => name,
        }
    }

    fn movie_file_name(&self, title: &str, year: Option<i32>, ext: &str) -> String {
        let base = self.movie_folder_name(title, year);
        format!("{}.{}", base, ext.trim_start_matches('.'))
    }

    fn series_folder_name(&self, title: &str) -> String {
        self.sanitize(title)
    }

    fn season_folder_name(&self, season_number: u8) -> String {
        format!("Season {:02}", season_number)
    }

    fn episode_file_name(
        &self,
        title: &str,
        season_number: u8,
        episode_number: u16,
        ext: &str,
    ) -> String {
        let name = self.sanitize(title);
        format!("{} S{:02}E{:02}.{}", name, season_number, episode_number, ext.trim_start_matches('.'))
    }
}

/// TMDB-backed generator for creating folder structures using real titles.
pub struct TmdbFolderGenerator {
    tmdb: Arc<TmdbApiProvider>,
    naming: Arc<dyn NamingStrategy>,
    video_ext: String,
}

impl TmdbFolderGenerator {
    pub fn new(tmdb: Arc<TmdbApiProvider>) -> Self {
        Self {
            tmdb,
            naming: Arc::new(DefaultNamingStrategy),
            video_ext: "mkv".to_string(),
        }
    }

    pub fn with_naming(mut self, naming: Arc<dyn NamingStrategy>) -> Self {
        self.naming = naming;
        self
    }

    pub fn with_video_ext(mut self, ext: impl Into<String>) -> Self {
        self.video_ext = ext.into();
        self
    }

    /// Generate a movie library structure with N movie folders, each containing one video file.
    pub async fn generate_movies(
        &self,
        root: &Path,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
    ) -> Result<StructurePlan> {
        let mut plan = StructurePlan::default();
        plan.push_dir(root.to_path_buf());

        let mut collected: Vec<(String, Option<i32>)> = Vec::new();
        let mut page: u32 = 1;
        while collected.len() < count {
            let page_res = self
                .tmdb
                .list_popular_movies(Some(page), language.map(|l| l.to_string()), region.map(|r| r.to_string()))
                .await
                .map_err(|e| crate::MediaError::Internal(format!("TMDB popular movies error: {}", e)))?;
            for m in page_res.results {
                let title = m.inner.title;
                let year = m.inner.release_date.map(|d| d.year());
                collected.push((title, year));
                if collected.len() >= count {
                    break;
                }
            }
            if page as u64 >= page_res.total_pages {
                break; // no more pages
            }
            page += 1;
        }

        // Use up to requested count; if less available, generate as many as possible
        for (title, year) in collected.into_iter().take(count) {
            let folder_name = self.naming.movie_folder_name(&title, year);
            let folder_path = root.join(folder_name);
            plan.push_dir(folder_path.clone());

            // Make a plausible video file size between 700MB and 2.5GB
            let size = rand::thread_rng().gen_range(700_u64..=2500_u64) * 1024 * 1024;
            let file_name = self
                .naming
                .movie_file_name(&title, year, &self.video_ext);
            plan.push_file(folder_path.join(file_name), size);
        }

        Ok(plan)
    }

    /// Generate a series library structure with count series; for each, generate seasons and episodes within the given ranges.
    pub async fn generate_series(
        &self,
        root: &Path,
        count: usize,
        language: Option<&str>,
        seasons_range: std::ops::RangeInclusive<u8>,
        episodes_per_season_range: std::ops::RangeInclusive<u16>,
    ) -> Result<StructurePlan> {
        let mut plan = StructurePlan::default();
        plan.push_dir(root.to_path_buf());

        let mut collected: Vec<String> = Vec::new();
        let mut page: u32 = 1;
        while collected.len() < count {
let page_res = self
                .tmdb
                .list_popular_tvshows(Some(page), language.map(|l| l.to_string()))
                .await
                .map_err(|e| crate::MediaError::Internal(format!("TMDB popular TV error: {}", e)))?;
            for s in page_res.results {
                let title = s.inner.name;
                collected.push(title);
                if collected.len() >= count {
                    break;
                }
            }
            if page as u64 >= page_res.total_pages {
                break;
            }
            page += 1;
        }

        let mut rng = rand::thread_rng();
        for title in collected.into_iter().take(count) {
            let series_folder = self.naming.series_folder_name(&title);
            let series_path = root.join(series_folder);
            plan.push_dir(series_path.clone());

            let seasons = if seasons_range.start() == seasons_range.end() {
                *seasons_range.start()
            } else {
                rng.gen_range(seasons_range.clone())
            };

            for season_idx in 1..=seasons {
                let season_folder = self.naming.season_folder_name(season_idx);
                let season_path = series_path.join(season_folder);
                plan.push_dir(season_path.clone());

                let episodes = if episodes_per_season_range.start() == episodes_per_season_range.end()
                {
                    *episodes_per_season_range.start()
                } else {
                    rng.gen_range(episodes_per_season_range.clone())
                };

                for ep_idx in 1..=episodes {
                    let fname = self
                        .naming
                        .episode_file_name(&title, season_idx, ep_idx, &self.video_ext);
                    let size = rng.gen_range(300_u64..=1600_u64) * 1024 * 1024; // 300MB - 1.6GB
                    plan.push_file(season_path.join(fname), size);
                }
            }
        }

        Ok(plan)
    }
}

/// Apply a structure plan to an InMemoryFs instance.
pub fn apply_plan_to_inmemory_fs(fs: &mut InMemoryFs, plan: &StructurePlan) {
    for node in &plan.nodes {
        match node {
            GeneratedNode::Dir(p) => fs.add_dir(p),
            GeneratedNode::File { path, len } => fs.add_file(path, *len),
        }
    }
}
