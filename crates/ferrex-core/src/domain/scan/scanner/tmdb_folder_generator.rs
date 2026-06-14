use std::collections::HashSet;
use std::fmt;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Datelike;
use rand::Rng;
use rand::seq::SliceRandom;

use crate::{
    error::{MediaError, Result},
    infra::media::providers::TmdbApiProvider,
};

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
        self.nodes.push(GeneratedNode::File {
            path: p.into(),
            len,
        });
    }
}

/// Strategy for naming folders and files.
pub trait NamingStrategy: Send + Sync {
    fn movie_folder_name(&self, title: &str, year: Option<i32>) -> String;
    fn movie_file_name(
        &self,
        title: &str,
        year: Option<i32>,
        ext: &str,
    ) -> String;
    fn series_folder_name(&self, title: &str, year: Option<i32>) -> String;
    fn season_folder_name(&self, season_number: u8) -> String;
    fn episode_file_name(
        &self,
        title: &str,
        season_number: u8,
        episode_number: u16,
        ext: &str,
    ) -> String;
}

#[derive(Debug, Default, Clone, Copy)]
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

    fn movie_file_name(
        &self,
        title: &str,
        year: Option<i32>,
        ext: &str,
    ) -> String {
        let base = self.movie_folder_name(title, year);
        format!("{}.{}", base, ext.trim_start_matches('.'))
    }

    fn series_folder_name(&self, title: &str, year: Option<i32>) -> String {
        self.movie_folder_name(title, year)
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
        format!(
            "{} S{:02}E{:02}.{}",
            name,
            season_number,
            episode_number,
            ext.trim_start_matches('.')
        )
    }
}

/// TMDB-backed generator for creating folder structures using real titles.
pub struct TmdbFolderGenerator {
    tmdb: Arc<TmdbApiProvider>,
    naming: Arc<dyn NamingStrategy>,
    video_ext: String,
}

impl fmt::Debug for TmdbFolderGenerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let naming_type = std::any::type_name_of_val(self.naming.as_ref());

        f.debug_struct("TmdbFolderGenerator")
            .field("tmdb", &self.tmdb)
            .field("naming_strategy", &naming_type)
            .field("video_ext", &self.video_ext)
            .finish()
    }
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

    async fn collect_movies(
        &self,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
        forbidden_folder_names: &std::collections::HashSet<String>,
    ) -> Result<Vec<(String, Option<i32>)>> {
        let current_year = chrono::Utc::now().year();
        let min_year = 1980_i32.min(current_year - 1);
        let max_year = current_year.max(min_year);

        let mut years: Vec<i32> = (min_year..=max_year).collect();
        {
            let mut rng = rand::rng();
            years.shuffle(&mut rng);
        }
        let mut year_idx: usize = 0;

        let mut used_tmdb_ids: std::collections::HashSet<u64> =
            std::collections::HashSet::new();
        let mut used_folder_names = forbidden_folder_names.clone();
        let mut exhausted_years: std::collections::HashSet<i32> =
            std::collections::HashSet::new();
        let mut next_page_by_year: std::collections::HashMap<i32, u32> =
            std::collections::HashMap::new();
        let mut total_pages_by_year: std::collections::HashMap<i32, u32> =
            std::collections::HashMap::new();

        let mut collected: Vec<(String, Option<i32>)> = Vec::new();
        let max_attempts = count.saturating_mul(3);
        let mut attempts = 0usize;

        while collected.len() < count && attempts < max_attempts {
            if exhausted_years.len() >= years.len() {
                break;
            }

            let year = years[year_idx];
            year_idx = (year_idx + 1) % years.len();
            if exhausted_years.contains(&year) {
                continue;
            }

            let page = next_page_by_year.get(&year).copied().unwrap_or(1);
            if let Some(total_pages) = total_pages_by_year.get(&year)
                && page > *total_pages
            {
                exhausted_years.insert(year);
                continue;
            }

            attempts += 1;

            let page_res = self
                .tmdb
                .discover_movies_by_year(year, page, language, region)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "TMDB discover movie error (year={}, page={}, lang={:?}, region={:?}): {}",
                        year, page, language, region, e
                    ))
                })?;

            total_pages_by_year.insert(year, page_res.total_pages);
            next_page_by_year.insert(year, page.saturating_add(1));

            if page_res.results.is_empty() {
                exhausted_years.insert(year);
                continue;
            }

            if page >= *total_pages_by_year.get(&year).unwrap_or(&1) {
                exhausted_years.insert(year);
            }

            for m in page_res.results {
                if collected.len() >= count {
                    break;
                }

                if m.title.trim().is_empty() {
                    continue;
                }

                if !used_tmdb_ids.insert(m.id) {
                    continue;
                }

                let year = m.release_date.map(|d| d.year());
                let folder_name = self.naming.movie_folder_name(&m.title, year);
                if !used_folder_names.insert(folder_name) {
                    continue;
                }

                collected.push((m.title, year));
            }
        }

        if collected.len() < count {
            return Err(MediaError::Internal(format!(
                "unable to collect enough movies via TMDB discover (wanted={}, got={}, attempts={})",
                count,
                collected.len(),
                attempts
            )));
        }

        Ok(collected)
    }

    async fn collect_series(
        &self,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
        forbidden_folder_names: &std::collections::HashSet<String>,
    ) -> Result<Vec<(String, Option<i32>)>> {
        let current_year = chrono::Utc::now().year();
        let min_year = 1970_i32.min(current_year);
        let max_year = current_year.max(min_year);

        let mut years: Vec<i32> = (min_year..=max_year).collect();
        {
            let mut rng = rand::rng();
            years.shuffle(&mut rng);
        }
        let mut year_idx: usize = 0;

        let region_upper = region.map(|r| r.trim().to_ascii_uppercase());
        let mut used_tmdb_ids: std::collections::HashSet<u64> =
            std::collections::HashSet::new();
        let mut used_folder_names = forbidden_folder_names.clone();
        let mut exhausted_years: std::collections::HashSet<i32> =
            std::collections::HashSet::new();
        let mut next_page_by_year: std::collections::HashMap<i32, u32> =
            std::collections::HashMap::new();
        let mut total_pages_by_year: std::collections::HashMap<i32, u32> =
            std::collections::HashMap::new();

        let mut collected: Vec<(String, Option<i32>)> = Vec::new();
        let max_attempts = count.saturating_mul(35).clamp(25, 2500);
        let mut attempts = 0usize;
        let relax_after = max_attempts / 2;

        while collected.len() < count && attempts < max_attempts {
            if exhausted_years.len() >= years.len() {
                break;
            }

            let year = years[year_idx];
            year_idx = (year_idx + 1) % years.len();
            if exhausted_years.contains(&year) {
                continue;
            }

            let page = next_page_by_year.get(&year).copied().unwrap_or(1);
            if let Some(total_pages) = total_pages_by_year.get(&year)
                && page > *total_pages
            {
                exhausted_years.insert(year);
                continue;
            }

            attempts += 1;
            let relax_region_filter =
                region_upper.is_some() && attempts >= relax_after;

            let page_res = self
                .tmdb
                .discover_tv_by_year(year, page, language)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "TMDB discover TV error (year={}, page={}, lang={:?}): {}",
                        year, page, language, e
                    ))
                })?;

            total_pages_by_year
                .insert(year, page_res.total_pages.clamp(1, 500));
            next_page_by_year.insert(year, page.saturating_add(1));

            if page_res.results.is_empty() {
                exhausted_years.insert(year);
                continue;
            }

            if page >= *total_pages_by_year.get(&year).unwrap_or(&1) {
                exhausted_years.insert(year);
            }

            for s in page_res.results {
                if collected.len() >= count {
                    break;
                }

                if s.name.trim().is_empty() {
                    continue;
                }

                if !used_tmdb_ids.insert(s.id) {
                    continue;
                }

                let mut origin_countries = s
                    .origin_country
                    .iter()
                    .map(|c| c.trim().to_ascii_uppercase())
                    .filter(|c| !c.is_empty())
                    .collect::<Vec<_>>();
                origin_countries.sort();
                origin_countries.dedup();

                let matches_region = region_upper
                    .as_ref()
                    .map(|target| origin_countries.iter().any(|c| c == target))
                    .unwrap_or(true);

                if !matches_region && !relax_region_filter {
                    continue;
                }

                let year = s.first_air_date.map(|d| d.year());
                let folder_name = self.naming.series_folder_name(&s.name, year);
                if !used_folder_names.insert(folder_name) {
                    continue;
                }

                collected.push((s.name, year));
            }
        }

        if collected.len() < count {
            return Err(MediaError::Internal(format!(
                "unable to collect enough series via TMDB discover (wanted={}, got={}, attempts={})",
                count,
                collected.len(),
                attempts
            )));
        }

        Ok(collected)
    }

    /// Generate a movie library structure with N movie folders, each containing one video file.
    pub async fn generate_movies(
        &self,
        root: &Path,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
    ) -> Result<StructurePlan> {
        self.generate_movies_excluding(
            root,
            count,
            language,
            region,
            &std::collections::HashSet::new(),
        )
        .await
    }

    pub async fn generate_movies_excluding(
        &self,
        root: &Path,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
        forbidden_folder_names: &std::collections::HashSet<String>,
    ) -> Result<StructurePlan> {
        let mut plan = StructurePlan::default();
        plan.push_dir(root.to_path_buf());

        let collected = self
            .collect_movies(count, language, region, forbidden_folder_names)
            .await?;

        for (title, year) in collected.into_iter().take(count) {
            let folder_name = self.naming.movie_folder_name(&title, year);
            let folder_path = root.join(folder_name);
            plan.push_dir(folder_path.clone());

            // Make a plausible video file size between 700MB and 2.5GB
            let size =
                rand::rng().random_range(700_u64..=2500_u64) * 1024 * 1024;
            let file_name =
                self.naming.movie_file_name(&title, year, &self.video_ext);
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
        region: Option<&str>,
        seasons_range: std::ops::RangeInclusive<u8>,
        episodes_per_season_range: std::ops::RangeInclusive<u16>,
    ) -> Result<StructurePlan> {
        self.generate_series_excluding(
            root,
            count,
            language,
            region,
            seasons_range,
            episodes_per_season_range,
            &std::collections::HashSet::new(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn generate_series_excluding(
        &self,
        root: &Path,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
        seasons_range: RangeInclusive<u8>,
        episodes_per_season_range: RangeInclusive<u16>,
        forbidden_folder_names: &HashSet<String>,
    ) -> Result<StructurePlan> {
        let mut plan = StructurePlan::default();
        plan.push_dir(root.to_path_buf());

        let collected = self
            .collect_series(count, language, region, forbidden_folder_names)
            .await?;

        for (title, year) in collected.into_iter().take(count) {
            let series_folder = self.naming.series_folder_name(&title, year);
            let series_path = root.join(series_folder);
            plan.push_dir(series_path.clone());

            let seasons = if seasons_range.start() == seasons_range.end() {
                *seasons_range.start()
            } else {
                rand::rng().random_range(seasons_range.clone())
            };

            for season_idx in 1..=seasons {
                let season_folder = self.naming.season_folder_name(season_idx);
                let season_path = series_path.join(season_folder);
                plan.push_dir(season_path.clone());

                let episodes = if episodes_per_season_range.start()
                    == episodes_per_season_range.end()
                {
                    *episodes_per_season_range.start()
                } else {
                    rand::rng().random_range(episodes_per_season_range.clone())
                };

                for ep_idx in 1..=episodes {
                    let fname = self.naming.episode_file_name(
                        &title,
                        season_idx,
                        ep_idx,
                        &self.video_ext,
                    );
                    let size = rand::rng().random_range(300_u64..=1600_u64)
                        * 1024
                        * 1024; // 300MB - 1.6GB
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_folder_names_include_year_when_provided() {
        let naming = DefaultNamingStrategy;
        assert_eq!(
            naming.series_folder_name("The Office", Some(2005)),
            "The Office (2005)"
        );
    }
}
