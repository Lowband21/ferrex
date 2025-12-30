use std::collections::HashMap;
use std::sync::Arc;

use ferrex_core::player_prelude::{LibraryId, SeriesBundleResponse, SeriesID};
use rkyv::{rancor::Error, util::AlignedVec};
use uuid::Uuid;

use crate::infra::repository::{RepositoryError, RepositoryResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesBundleKey {
    pub library_id: LibraryId,
    pub series_id: SeriesID,
}

impl SeriesBundleKey {
    pub fn new(library_id: LibraryId, series_id: SeriesID) -> Self {
        Self {
            library_id,
            series_id,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SeriesBundleSeasonLocator {
    key: SeriesBundleKey,
    index: u32,
}

#[derive(Debug, Clone, Copy)]
struct SeriesBundleEpisodeLocator {
    key: SeriesBundleKey,
    index: u32,
}

#[derive(Debug, Default)]
pub struct SeriesBundleInstallOutcome {
    pub series_indexed: usize,
    pub seasons_indexed: usize,
    pub episodes_indexed: usize,
    pub items_replaced_from_runtime_overlay: usize,
    pub series_id: Uuid,
    pub season_ids: Vec<Uuid>,
    pub episode_ids: Vec<Uuid>,
}

#[derive(Debug, Default)]
pub struct SeriesBundleOverlay {
    bundles: HashMap<SeriesBundleKey, Arc<AlignedVec>>,
    series_by_id: HashMap<Uuid, SeriesBundleKey>,
    seasons_by_id: HashMap<Uuid, SeriesBundleSeasonLocator>,
    episodes_by_id: HashMap<Uuid, SeriesBundleEpisodeLocator>,
    bundle_season_ids: HashMap<SeriesBundleKey, Vec<Uuid>>,
    bundle_episode_ids: HashMap<SeriesBundleKey, Vec<Uuid>>,
}

impl SeriesBundleOverlay {
    pub fn clear(&mut self) {
        self.bundles.clear();
        self.series_by_id.clear();
        self.seasons_by_id.clear();
        self.episodes_by_id.clear();
        self.bundle_season_ids.clear();
        self.bundle_episode_ids.clear();
    }

    pub fn series_len(&self) -> usize {
        self.series_by_id.len()
    }

    pub fn seasons_len(&self) -> usize {
        self.seasons_by_id.len()
    }

    pub fn episodes_len(&self, lib_id: &LibraryId) -> usize {
        self.episodes_by_id
            .iter()
            .filter(|(_uuid, series_loc)| series_loc.key.library_id == *lib_id)
            .collect::<Vec<(&Uuid, &SeriesBundleEpisodeLocator)>>()
            .len()
    }

    pub fn series_ids_for_library(&self, library_id: &LibraryId) -> Vec<Uuid> {
        self.series_by_id
            .iter()
            .filter_map(|(uuid, key)| {
                (key.library_id == *library_id).then_some(*uuid)
            })
            .collect()
    }

    pub fn get_series_cart(
        &self,
        series_uuid: &Uuid,
    ) -> Option<Arc<AlignedVec>> {
        let key = self.series_by_id.get(series_uuid)?;
        self.bundles.get(key).map(Arc::clone)
    }

    pub fn get_season_locator(
        &self,
        season_uuid: &Uuid,
    ) -> Option<(Arc<AlignedVec>, u32)> {
        let locator = self.seasons_by_id.get(season_uuid)?;
        let cart = self.bundles.get(&locator.key)?;
        Some((Arc::clone(cart), locator.index))
    }

    pub fn get_episode_locator(
        &self,
        episode_uuid: &Uuid,
    ) -> Option<(Arc<AlignedVec>, u32)> {
        let locator = self.episodes_by_id.get(episode_uuid)?;
        let cart = self.bundles.get(&locator.key)?;
        Some((Arc::clone(cart), locator.index))
    }

    pub fn upsert_bundle(
        &mut self,
        expected_key: SeriesBundleKey,
        bytes: AlignedVec,
    ) -> RepositoryResult<(Uuid, Vec<Uuid>, Vec<Uuid>)> {
        let buffer = Arc::new(bytes);
        let archived = rkyv::access::<
            rkyv::Archived<SeriesBundleResponse>,
            Error,
        >(&buffer)
        .map_err(|e| RepositoryError::DeserializationError(e.to_string()))?;

        let actual_library_id = archived.library_id.as_uuid();
        if actual_library_id != expected_key.library_id.to_uuid() {
            return Err(RepositoryError::UpdateFailed(format!(
                "Series bundle payload library_id mismatch: expected {} got {}",
                expected_key.library_id, actual_library_id
            )));
        }

        let actual_series_uuid = archived.series_id.to_uuid();
        if actual_series_uuid != expected_key.series_id.to_uuid() {
            return Err(RepositoryError::UpdateFailed(format!(
                "Series bundle payload series_id mismatch: expected {} got {}",
                expected_key.series_id, actual_series_uuid
            )));
        }

        if let Some(old_key) = self.series_by_id.remove(&actual_series_uuid) {
            if old_key == expected_key {
                if let Some(old_seasons) =
                    self.bundle_season_ids.remove(&old_key)
                {
                    for season_id in old_seasons {
                        if let Some(locator) =
                            self.seasons_by_id.get(&season_id)
                            && locator.key == old_key
                        {
                            self.seasons_by_id.remove(&season_id);
                        }
                    }
                }

                if let Some(old_episodes) =
                    self.bundle_episode_ids.remove(&old_key)
                {
                    for episode_id in old_episodes {
                        if let Some(locator) =
                            self.episodes_by_id.get(&episode_id)
                            && locator.key == old_key
                        {
                            self.episodes_by_id.remove(&episode_id);
                        }
                    }
                }

                self.bundles.remove(&old_key);
            } else {
                self.series_by_id.insert(actual_series_uuid, old_key);
            }
        }

        self.bundles.insert(expected_key, Arc::clone(&buffer));
        self.series_by_id.insert(actual_series_uuid, expected_key);

        let mut season_ids = Vec::with_capacity(archived.seasons.len());
        for (idx, season) in archived.seasons.iter().enumerate() {
            let season_uuid = season.id.to_uuid();
            season_ids.push(season_uuid);
            self.seasons_by_id.insert(
                season_uuid,
                SeriesBundleSeasonLocator {
                    key: expected_key,
                    index: idx as u32,
                },
            );
        }

        let mut episode_ids = Vec::with_capacity(archived.episodes.len());
        for (idx, episode) in archived.episodes.iter().enumerate() {
            let episode_uuid = episode.id.to_uuid();
            episode_ids.push(episode_uuid);
            self.episodes_by_id.insert(
                episode_uuid,
                SeriesBundleEpisodeLocator {
                    key: expected_key,
                    index: idx as u32,
                },
            );
        }

        self.bundle_season_ids
            .insert(expected_key, season_ids.clone());
        self.bundle_episode_ids
            .insert(expected_key, episode_ids.clone());

        Ok((actual_series_uuid, season_ids, episode_ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ferrex_model::{
        details::ExternalIds,
        files::MediaFile,
        image::MediaImages,
        numbers::{EpisodeNumber, SeasonNumber},
        titles::SeriesTitle,
        urls::{EpisodeURL, SeasonURL, SeriesURL},
    };

    use ferrex_core::player_prelude::{
        EnhancedSeriesDetails, EpisodeDetails, EpisodeReference, LibraryId,
        MediaID, SeasonDetails, SeasonReference, Series,
    };

    use std::path::PathBuf;

    fn stub_series_details(id: u64, name: &str) -> EnhancedSeriesDetails {
        EnhancedSeriesDetails {
            id,
            name: name.to_string(),
            original_name: None,
            overview: None,
            first_air_date: None,
            last_air_date: None,
            number_of_seasons: None,
            number_of_episodes: None,
            available_seasons: None,
            available_episodes: None,
            vote_average: None,
            vote_count: None,
            popularity: None,
            content_rating: None,
            content_ratings: Vec::new(),
            release_dates: Vec::new(),
            genres: Vec::new(),
            networks: Vec::new(),
            origin_countries: Vec::new(),
            spoken_languages: Vec::new(),
            production_companies: Vec::new(),
            production_countries: Vec::new(),
            homepage: None,
            status: None,
            tagline: None,
            in_production: None,
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            primary_poster_iid: None,
            primary_backdrop_iid: None,
            images: MediaImages::default(),
            cast: Vec::new(),
            crew: Vec::new(),
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            episode_groups: Vec::new(),
            recommendations: Vec::new(),
            similar: Vec::new(),
        }
    }

    fn stub_season_details(id: u64, season_number: u16) -> SeasonDetails {
        SeasonDetails {
            id,
            season_number,
            name: format!("Season {}", season_number),
            overview: None,
            air_date: None,
            episode_count: 1,
            poster_path: None,
            primary_poster_iid: None,
            runtime: None,
            external_ids: ExternalIds::default(),
            images: MediaImages::default(),
            videos: Vec::new(),
            keywords: Vec::new(),
            translations: Vec::new(),
        }
    }

    fn stub_episode_details(
        id: u64,
        season_number: u16,
        episode_number: u16,
    ) -> EpisodeDetails {
        EpisodeDetails {
            id,
            episode_number,
            season_number,
            name: format!("S{:02}E{:02}", season_number, episode_number),
            overview: None,
            air_date: None,
            runtime: None,
            still_path: None,
            primary_still_iid: None,
            vote_average: None,
            vote_count: None,
            production_code: None,
            external_ids: ExternalIds::default(),
            images: MediaImages::default(),
            videos: Vec::new(),
            keywords: Vec::new(),
            translations: Vec::new(),
            guest_stars: Vec::new(),
            crew: Vec::new(),
            content_ratings: Vec::new(),
        }
    }

    #[test]
    fn upsert_and_locate_seasons_and_episodes() {
        let library_id = LibraryId::new();
        let series_uuid = Uuid::now_v7();
        let series_id = SeriesID(series_uuid);
        let season_uuid = Uuid::now_v7();
        let season_id = ferrex_core::player_prelude::SeasonID(season_uuid);
        let episode_uuid = Uuid::now_v7();
        let episode_id = ferrex_core::player_prelude::EpisodeID(episode_uuid);

        let series = Series {
            id: series_id,
            library_id,
            tmdb_id: 1,
            title: SeriesTitle::new("Test Series".to_string())
                .expect("valid series title"),
            details: stub_series_details(1, "Test Series"),
            endpoint: SeriesURL::from_string(format!(
                "/series/{}",
                series_uuid
            )),
            discovered_at: ferrex_model::chrono::Utc::now(),
            created_at: ferrex_model::chrono::Utc::now(),
            theme_color: None,
        };

        let season = SeasonReference {
            id: season_id,
            library_id,
            season_number: SeasonNumber::new(1),
            series_id,
            tmdb_series_id: 1,
            details: stub_season_details(10, 1),
            endpoint: SeasonURL::from_string(format!("/media/{}", season_uuid)),
            discovered_at: ferrex_model::chrono::Utc::now(),
            created_at: ferrex_model::chrono::Utc::now(),
            theme_color: None,
        };

        let file = MediaFile {
            id: Uuid::now_v7(),
            media_id: MediaID::Episode(episode_id),
            path: PathBuf::from("/tmp/test.mkv"),
            filename: "test.mkv".to_string(),
            size: 123,
            discovered_at: ferrex_model::chrono::Utc::now(),
            created_at: ferrex_model::chrono::Utc::now(),
            media_file_metadata: None,
            library_id,
        };

        let episode = EpisodeReference {
            id: episode_id,
            library_id,
            episode_number: EpisodeNumber::new(1),
            season_number: SeasonNumber::new(1),
            season_id,
            series_id,
            tmdb_series_id: 1,
            details: stub_episode_details(100, 1, 1),
            endpoint: EpisodeURL::from_string("/stream/file".to_string()),
            file,
            discovered_at: ferrex_model::chrono::Utc::now(),
            created_at: ferrex_model::chrono::Utc::now(),
        };

        let bundle = SeriesBundleResponse {
            library_id,
            series_id,
            series,
            seasons: vec![season],
            episodes: vec![episode],
        };

        let bytes = rkyv::to_bytes::<Error>(&bundle).expect("serialize");
        let key = SeriesBundleKey::new(library_id, series_id);
        let mut overlay = SeriesBundleOverlay::default();
        overlay
            .upsert_bundle(key, bytes)
            .expect("upsert series bundle");

        let cart = overlay
            .get_series_cart(&series_uuid)
            .expect("series cart present");
        let archived =
            rkyv::access::<rkyv::Archived<SeriesBundleResponse>, Error>(&cart)
                .expect("access archived");
        assert_eq!(archived.series_id.to_uuid(), series_uuid);

        let (season_cart, season_index) = overlay
            .get_season_locator(&season_uuid)
            .expect("season locator");
        let archived = rkyv::access::<
            rkyv::Archived<SeriesBundleResponse>,
            Error,
        >(&season_cart)
        .expect("access archived");
        assert_eq!(
            archived.seasons[season_index as usize].id.to_uuid(),
            season_uuid
        );

        let (episode_cart, episode_index) = overlay
            .get_episode_locator(&episode_uuid)
            .expect("episode locator");
        let archived = rkyv::access::<
            rkyv::Archived<SeriesBundleResponse>,
            Error,
        >(&episode_cart)
        .expect("access archived");
        assert_eq!(
            archived.episodes[episode_index as usize].id.to_uuid(),
            episode_uuid
        );
    }
}
